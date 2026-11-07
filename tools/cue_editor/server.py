#!/usr/bin/env python3
"""Local server for the cue/lyric timeline editor (tools/cue_editor/).

Edits one thing: a packet's `set.direction.cues` (each cue's `t` and `word_times`). Everything
else in the packet file is round-tripped untouched. There is no custom backup mechanism --
this is a git repo, so a save's undo is `git diff` / `git checkout` on the packet file, same as
every other edit in this project.

Why a cue's boundary needs a server-computed "real" position, not just the value you dragged it
to: `crates/direct/src/scenes.rs::candidates()` silently moves most cue boundaries onto the
nearest kick/snare onset within `snap_window_beats` beats, before the scene list is even built.
Measured against this song's real drum onsets, four of the sixteen cues already land up to 210ms
from their authored `t`. The browser side needs that same computation live, while dragging, or
the editor becomes exactly the kind of "trust the display" problem this tool exists to fix.
"""

import hashlib
import json
import re
import socketserver
import subprocess
import sys
from functools import lru_cache
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

ROOT = Path(__file__).resolve().parent.parent.parent
PACKETS_DIR = ROOT / "packets"
VOCAB_DIR = ROOT / "vocab"
STATIC_DIR = Path(__file__).resolve().parent

# Mirrors `impl Default for Lyric` in crates/direct/src/settings.rs: `lyric.min_s` is read from
# the packet/vocab chain like anything else, but when nothing in that chain sets it (as with this
# packet, which only overrides `lyric.size`), the real value is this Rust literal, not anything
# found in a JSON file. Only the one field the subline-cut preview needs is mirrored here.
RUST_LYRIC_DEFAULT = {"min_s": 0.8}

NAME_RE = re.compile(r"^[A-Za-z0-9_-]+$")
CONTENT_TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8"}


def packet_file(name: str) -> Path:
    if not NAME_RE.match(name):
        raise ValueError(f"bad packet name: {name!r}")
    p = PACKETS_DIR / f"{name}.json"
    if not p.is_file():
        raise ValueError(f"no such packet: {p}")
    return p


def load_packet_chain(name: str) -> list[dict]:
    """The packet and every packet it `extends`, leaf first, outermost last."""
    chain = []
    seen = set()
    cur = name
    while cur is not None:
        if cur in seen:
            raise ValueError(f"extends loop at {cur!r}")
        seen.add(cur)
        data = json.loads(packet_file(cur).read_text(encoding="utf-8"))
        chain.append(data)
        ext = data.get("extends")
        cur = Path(ext).stem if ext else None
    return chain


def direction_setting(chain: list[dict], key: str, default):
    """A scalar/array direction field: the first packet in the chain (leaf first) that sets it
    wins outright, matching the packet "replace" merge rule in the README."""
    for data in chain:
        v = data.get("set", {}).get("direction", {}).get(key)
        if v is not None:
            return v
    return default


def direction_dict_setting(chain: list[dict], key: str, vocab_default: dict) -> dict:
    """A dict-valued direction field (e.g. `priority`): merges key by key across the whole
    chain, outermost packet first so the leaf's own keys win, matching the "objects merge key
    by key" rule in the README -- a partial override must not blank out the rest of the dict."""
    result = dict(vocab_default)
    for data in reversed(chain):
        v = data.get("set", {}).get("direction", {}).get(key)
        if isinstance(v, dict):
            result.update(v)
    return result


@lru_cache(maxsize=1)
def vocab_direction_defaults() -> dict:
    return json.loads((VOCAB_DIR / "direction.json").read_text(encoding="utf-8"))


@lru_cache(maxsize=1)
def cargo_target_dir() -> Path:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT, capture_output=True, check=True, text=True,
    )
    return Path(json.loads(out.stdout)["target_directory"])


def packet_binary_path(key: str, packet_name: str) -> Path:
    """Shell out to the `packet` binary for a resolved output path, the way tools/render.sh does."""
    binary = cargo_target_dir() / "release" / "packet"
    if not binary.is_file():
        raise ValueError(f"{binary} not built; run tools/render.sh once first")
    out = subprocess.run(
        [str(binary), "path", str(packet_file(packet_name)), key],
        cwd=ROOT, capture_output=True, check=True, text=True,
    )
    return Path(out.stdout.strip())


def cue_words(text: str) -> list[str]:
    words = []
    for sub in text.split(" / "):
        words.extend(sub.split())
    return words


def validate_cues(cues: list[dict]) -> list[str]:
    """The exact invariants `Direction::parse` enforces in crates/direct/src/scenes.rs -- nothing
    stricter, nothing invented. A cue outside these is a packet the renderer will refuse to load."""
    errors = []
    for i, c in enumerate(cues):
        t = c.get("t")
        if not isinstance(t, (int, float)) or t < 0.0:
            errors.append(f"cue {i}: t must be >= 0.0, got {t!r}")
        wt = c.get("word_times")
        if wt is None:
            continue
        expected = len(cue_words(c.get("text") or ""))
        if len(wt) != expected:
            errors.append(f"cue {i}: word_times has {len(wt)} entries, text has {expected} words")
            continue
        if any(wt[k + 1] < wt[k] for k in range(len(wt) - 1)):
            errors.append(f"cue {i}: word_times is not non-decreasing")
        if wt and wt[0] < 0.0:
            errors.append(f"cue {i}: word_times[0] must be >= 0.0, got {wt[0]!r}")
    return errors


def _close(a, b, eps=1e-9) -> bool:
    return abs(a - b) < eps


def merge_cues(old_cues: list[dict], new_cues: list[dict]) -> list[dict]:
    """Take only `t` and `word_times` from the client, and only where they actually changed --
    everything else (every other cue field, and any `t`/`word_times` value the client round-
    tripped unchanged) comes back as the exact object parsed from the file on disk.

    This exists because the editor is a browser: JSON.stringify prints a whole-number float like
    14.0 as "14", so a client payload that re-sends every cue reformats every whole-number time
    in the file, not just the one the user actually dragged. Diffing at the value level, against
    a fresh read of the file instead of trusting the client's copy, keeps a one-cue edit a one-
    cue diff regardless of what the browser's own number formatting does.
    """
    if len(old_cues) != len(new_cues):
        raise ValueError(f"cue count changed ({len(old_cues)} -> {len(new_cues)}); the editor does not add or remove cues")
    merged = []
    for i, (old, new) in enumerate(zip(old_cues, new_cues)):
        out = dict(old)
        old_t, new_t = old.get("t"), new.get("t")
        if new_t is None or not _close(float(old_t), float(new_t)):
            out["t"] = float(new_t)

        old_wt, new_wt = old.get("word_times"), new.get("word_times")
        if new_wt is None:
            out.pop("word_times", None)
        elif old_wt is None or len(old_wt) != len(new_wt) or any(not _close(a, float(b)) for a, b in zip(old_wt, new_wt)):
            out["word_times"] = [float(x) for x in new_wt]
        merged.append(out)
    return merged


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        sys.stderr.write(f"[cue_editor] {fmt % args}\n")

    def _json(self, obj, status=200):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _error(self, status, message):
        self._json({"error": message}, status=status)

    def _static(self, rel_path: str):
        path = (STATIC_DIR / rel_path).resolve()
        if STATIC_DIR not in path.parents or not path.is_file():
            self._error(404, "not found")
            return
        body = path.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", CONTENT_TYPES.get(path.suffix, "application/octet-stream"))
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        parsed = urlparse(self.path)
        qs = parse_qs(parsed.query)
        name = qs.get("packet", ["default"])[0]

        if parsed.path in ("/", "/index.html"):
            self._static("index.html")
        elif parsed.path in ("/editor.js", "/editor.css"):
            self._static(parsed.path.lstrip("/"))
        elif parsed.path == "/api/state":
            self._get_state(name)
        elif parsed.path == "/api/audio":
            self._get_audio(name)
        else:
            self._error(404, "not found")

    def do_POST(self):
        parsed = urlparse(self.path)
        qs = parse_qs(parsed.query)
        name = qs.get("packet", ["default"])[0]
        if parsed.path != "/api/save":
            self._error(404, "not found")
            return
        self._post_save(name)

    def _get_state(self, name):
        try:
            chain = load_packet_chain(name)
            packet = chain[0]
            cues = packet.get("set", {}).get("direction", {}).get("cues", [])

            analysis_path = packet_binary_path("analysis", name)
            if not analysis_path.is_file():
                self._error(409, f"{analysis_path} does not exist yet; run tools/render.sh {name} once first")
                return
            analysis = json.loads(analysis_path.read_text(encoding="utf-8"))

            audio_rel = packet["inputs"]["audio"]
            audio_path = (packet_file(name).parent / audio_rel).resolve()
            actual_hash = hashlib.sha256(audio_path.read_bytes()).hexdigest()
            expected_hash = analysis["source"]["audio_sha256"]

            vocab_default = vocab_direction_defaults()
            bpm = analysis["tempo"]["bpm"]
            beat_s = 60.0 / bpm
            snap_to = direction_setting(chain, "snap_to", vocab_default.get("snap_to", []))
            snap_window_beats = direction_setting(chain, "snap_window_beats", vocab_default.get("snap_window_beats", 0.0))
            snap_window_s = snap_window_beats * beat_s
            priority = direction_dict_setting(chain, "priority", vocab_default.get("priority", {}))
            priority_start = priority.get("start")
            lyric = direction_dict_setting(chain, "lyric", {**RUST_LYRIC_DEFAULT, **vocab_default.get("lyric", {})})
            lyric_min_s = lyric.get("min_s", RUST_LYRIC_DEFAULT["min_s"])

            onsets = []
            for stem in snap_to:
                for note in analysis.get("stems", {}).get(stem, []):
                    onsets.append([note["onset"], stem])
            onsets.sort(key=lambda o: o[0])

            self._json({
                "packet": name,
                "cues": cues,
                "duration_s": analysis["source"]["audio_duration_s"],
                "bpm": bpm,
                "drop_t": analysis["structure"]["drop_t"],
                "snap_to": snap_to,
                "snap_window_s": snap_window_s,
                "snap_onsets": onsets,
                "snap_exempt_priority": priority_start,
                "default_cue_priority": 8,
                "lyric_min_s": lyric_min_s,
                "waveform": analysis["waveform"],
                "audio_hash_ok": actual_hash == expected_hash,
            })
        except Exception as e:
            self._error(400, str(e))

    def _get_audio(self, name):
        try:
            packet = json.loads(packet_file(name).read_text(encoding="utf-8"))
            audio_rel = packet["inputs"]["audio"]
            audio_path = (packet_file(name).parent / audio_rel).resolve()
            body = audio_path.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "audio/wav")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception as e:
            self._error(400, str(e))

    def _post_save(self, name):
        try:
            length = int(self.headers.get("Content-Length", 0))
            incoming = json.loads(self.rfile.read(length))

            path = packet_file(name)
            data = json.loads(path.read_text(encoding="utf-8"))
            old_cues = data.get("set", {}).get("direction", {}).get("cues", [])
            merged_cues = merge_cues(old_cues, incoming["cues"])

            errors = validate_cues(merged_cues)
            if errors:
                self._error(422, "; ".join(errors))
                return

            data.setdefault("set", {}).setdefault("direction", {})["cues"] = merged_cues
            path.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
            self._json({"ok": True})
        except Exception as e:
            self._error(400, str(e))


class ThreadingServer(socketserver.ThreadingMixIn, HTTPServer):
    daemon_threads = True


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8740
    server = ThreadingServer(("127.0.0.1", port), Handler)
    print(f"[cue_editor] serving on http://127.0.0.1:{port}")
    server.serve_forever()


if __name__ == "__main__":
    main()
