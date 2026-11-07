"""A project: one recording, the packet that describes its run, and the operations on it.

Every operation runs the same binaries and tools the command line uses, against a packet file that
belongs to the project. Edits made with `set` are written into that packet's `set` section, so they
persist and every later operation, including a render started from the shell, sees them.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROJECTS = ROOT / "projects"
SECTIONS = {"feel", "vocabulary", "matrix", "direction", "signals", "export", "anchors"}
_binaries: Path | None = None


class CinemaError(Exception):
    """An operation failed; the message names the step and carries its output."""


def slug(text: str) -> str:
    name = re.sub(r"[^a-z0-9]+", "_", text.lower()).strip("_")
    if not name or name == "default":
        raise CinemaError(f"{text!r} is not usable as a project name")
    return name


def _run(cmd, env=None, cwd=ROOT) -> str:
    r = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, cwd=cwd, env=env)
    if r.returncode:
        raise CinemaError(f"{Path(str(cmd[0])).name} failed with status {r.returncode}\n{(r.stdout + r.stderr).strip()[-4000:]}")
    return r.stdout


def binaries(build: bool = True) -> Path:
    """The folder holding the release binaries, built on first use."""
    global _binaries
    if _binaries is None:
        meta = json.loads(_run(["cargo", "metadata", "--format-version", "1", "--no-deps"]))
        _binaries = Path(meta["target_directory"]) / "release"
        if build:
            _run(["cargo", "build", "--release", "-p", "packet", "-p", "direct"])
            _run(["cargo", "build", "--release", "-p", "cinema-flat"])
            _run(["cargo", "build", "--release", "-p", "cinema-lens"])
    return _binaries


def _set_path(doc: dict, dotted: str, value) -> None:
    keys = dotted.split(".")
    if keys[0] not in SECTIONS or len(keys) < 2:
        raise CinemaError(f"path {dotted!r} must start with one of {sorted(SECTIONS)} and name a field")
    node = doc.setdefault("set", {})
    for k in keys[:-1]:
        node = node.setdefault(k, {})
        if not isinstance(node, dict):
            raise CinemaError(f"path {dotted!r} goes through a value that is not an object")
    node[keys[-1]] = value


class Project:
    def __init__(self, name: str, root: Path = PROJECTS):
        self.name = name
        self.dir = Path(root) / name
        self.packet_path = self.dir / "packet.json"
        if not self.packet_path.is_file():
            raise CinemaError(f"no project named {name!r} in {root}")

    @classmethod
    def create(cls, audio, notes=None, name: str | None = None, root: Path = PROJECTS, overwrite: bool = False) -> "Project":
        """Start a project for an audio file of any name and format. With a notes file the run
        is filmed from both; without one the packet transcribes the audio first."""
        audio = Path(audio).expanduser().resolve()
        if not audio.is_file():
            raise CinemaError(f"{audio} is not a file")
        name = slug(name or audio.stem)
        d = Path(root) / name
        if (d / "packet.json").exists() and not overwrite:
            raise CinemaError(f"project {name!r} exists")
        d.mkdir(parents=True, exist_ok=True)
        doc = {"extends": str(ROOT / "packets" / "default.json"), "name": name,
               "inputs": {"audio": str(audio), "notes": None}}
        if notes:
            notes = Path(notes).expanduser().resolve()
            if not notes.is_file():
                raise CinemaError(f"{notes} is not a file")
            doc["inputs"]["notes"] = str(notes)
        else:
            doc["transcriber"] = ["python3", "tools/transcribe.py", "{audio}", "{notes}"]
        (d / "packet.json").write_text(json.dumps(doc, indent=2) + "\n")
        return cls(name, root)

    @classmethod
    def names(cls, root: Path = PROJECTS) -> list[str]:
        return sorted(p.parent.name for p in Path(root).glob("*/packet.json"))

    # -- reading ----------------------------------------------------------------------------

    @property
    def document(self) -> dict:
        return json.loads(self.packet_path.read_text())

    @property
    def run_dir(self) -> Path:
        shown = _run([binaries() / "packet", "show", self.packet_path])
        m = re.search(r"^run dir\s+(.+)$", shown, re.M)
        if not m:
            raise CinemaError("packet show did not report a run directory")
        return Path(m.group(1).strip())

    def overrides(self) -> dict:
        return self.document.get("set", {})

    def status(self) -> dict:
        run = self.run_dir
        video = run / "cinema_scenes.mp4"
        return {"name": self.name, "inputs": self.document["inputs"], "run_dir": str(run),
                "prepared": (run / "analysis.json").is_file(), "planned": (run / "plan.json").is_file(),
                "rendered": video.is_file(), "video": str(video) if video.is_file() else None,
                "overrides": self.overrides()}

    # -- operations -------------------------------------------------------------------------

    def _env(self) -> dict:
        return dict(os.environ, CINEMA_PACKET=str(self.packet_path))

    def prepare(self) -> dict:
        """Transcribe if needed, analyse, and detect image anchors. Returns the status."""
        _run([binaries() / "packet", "prepare", self.packet_path])
        return self.status()

    def plan(self) -> dict:
        """The scene plan: boundaries, palette, behaviour, look and layout of every scene."""
        _run([binaries() / "plan"], env=self._env())
        return json.loads((self.run_dir / "plan.json").read_text())

    def set(self, path: str, value) -> dict:
        """Set one field, for example set("direction.fit", 0.9). The edit is checked by planning with
        it; an invalid value is rejected and the packet keeps its previous contents."""
        before = self.packet_path.read_text()
        doc = json.loads(before)
        _set_path(doc, path, value)
        self.packet_path.write_text(json.dumps(doc, indent=2) + "\n")
        try:
            self.plan()
        except CinemaError:
            self.packet_path.write_text(before)
            raise
        return self.overrides()

    def unset(self, path: str) -> dict:
        doc = self.document
        keys = path.split(".")
        node = doc.get("set", {})
        for k in keys[:-1]:
            node = node.get(k, {})
        node.pop(keys[-1], None)
        self.packet_path.write_text(json.dumps(doc, indent=2) + "\n")
        return self.overrides()

    def frame(self, index: int, stage: str = "lens") -> bytes:
        """One frame as PNG bytes. stage is "flat" (the stage drawing) or "lens" (the filmed
        picture); the lens stage reads the flat pass video, so render once before asking for it."""
        if stage not in ("flat", "lens"):
            raise CinemaError('stage must be "flat" or "lens"')
        b = binaries()
        if stage == "lens":
            flat = Path(_run([b / "packet", "path", self.packet_path, "flat"]).strip())
            if not flat.is_file():
                raise CinemaError(f"{flat} does not exist; render the project once before previewing lens frames")
        with tempfile.TemporaryDirectory() as d:
            _run([b / f"cinema-{stage}", "frame", index, "-o", d], env=self._env())
            pngs = sorted(Path(d).glob("*.png"))
            if not pngs:
                raise CinemaError(f"cinema-{stage} produced no PNG")
            return pngs[0].read_bytes()

    def render(self, out=None, scenes=None, small: bool = False, crf: int = 27) -> Path:
        """Render the video. Only scenes whose inputs changed since the last render are drawn again.
        Returns the path of the video: the render copied to `out` when given, or with `small` a
        compressed 1080p H.264 copy (at `out`, or beside the render)."""
        cmd = [sys.executable, ROOT / "tools" / "render_scenes.py", self.packet_path]
        if scenes:
            cmd += ["--scenes", ",".join(map(str, scenes))]
        _run(cmd)
        video = self.run_dir / "cinema_scenes.mp4"
        if not video.is_file():
            raise CinemaError("the render finished without writing a video")
        target = Path(out) if out else (video.with_name("cinema_1080p_small.mp4") if small else video)
        target.parent.mkdir(parents=True, exist_ok=True)
        if small:
            _run([sys.executable, ROOT / "tools" / "compress.py", video, "--crf", crf, "--out", target])
        elif target != video:
            shutil.copyfile(video, target)
        return target

    def delete(self, outputs: bool = False) -> None:
        run = self.run_dir if outputs else None
        shutil.rmtree(self.dir)
        if run and run.exists():
            shutil.rmtree(run)
