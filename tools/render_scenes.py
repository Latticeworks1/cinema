#!/usr/bin/env python3
"""Render a packet one scene at a time and stitch the scenes, so an edit costs only the scenes it touches.

    tools/render_scenes.py PACKET --list
    tools/render_scenes.py PACKET                  # render the scenes whose fingerprint changed, then stitch
    tools/render_scenes.py PACKET --scenes 10,11   # force these scenes, then stitch
    tools/render_scenes.py PACKET --no-stitch | --no-build | --prepare

Each scene is the frame range between its cut and the next cut, rendered by the flat and lens
binaries (frame-exact, 30 fps) into <run>/scenes/scene_NN.mp4. A scene's fingerprint is the hash of
everything that can change its pixels: the plan entries of the scene and both neighbours (palette
blends and whips cross a cut), its frame range, the cues and collage plates that overlap it (with
the bytes of the plate images), the rest of the packet and its vocabulary, the analysis, and the
two renderer binaries. A scene is reused when its file exists and its fingerprint matches the
manifest. The stitched video is <run>/cinema_scenes.mp4: the scene videos copied as they are,
with the packet's audio encoded once.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FPS = 30


def sh(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        sys.exit(f"[!] {' '.join(map(str, cmd))}\n{r.stdout}\n{r.stderr}")
    return r.stdout


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha_file(p: Path) -> str:
    return sha(p.read_bytes()) if p.exists() else "missing"


def packet_chain(path: Path) -> list[dict]:
    d = json.loads(path.read_text())
    chain = [d]
    if d.get("extends"):
        chain += packet_chain(path.parent / d["extends"])
    return chain


def scene_independent(chain: list[dict]) -> str:
    """The packet without its cues and collage, which are hashed per scene."""
    out = []
    for d in chain:
        d = json.loads(json.dumps(d))
        direction = d.get("set", {}).get("direction", {})
        direction.pop("cues", None)
        direction.pop("collage", None)
        out.append(d)
    return json.dumps(out, sort_keys=True)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("packet")
    ap.add_argument("--scenes", help="comma-separated scene indices to force")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--no-stitch", action="store_true")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--prepare", action="store_true", help="rerun transcription and analysis first")
    a = ap.parse_args()

    os.chdir(ROOT)
    packet = Path(a.packet).resolve()
    env = dict(os.environ, CINEMA_PACKET=str(packet))
    tdir = Path(json.loads(sh(["cargo", "metadata", "--format-version", "1", "--no-deps"]))["target_directory"]) / "release"

    if not a.no_build:
        sh(["cargo", "build", "--release", "-p", "packet", "-p", "direct"])
        sh(["cargo", "build", "--release", "-p", "cinema-flat"])
        sh(["cargo", "build", "--release", "-p", "cinema-lens"])
    pk = lambda *args: sh([str(tdir / "packet"), *args], env=env)
    if a.prepare:
        pk("prepare", str(packet))
    sh([str(tdir / "plan")], env=env)

    run_dir = Path(re.search(r"^run dir\s+(.+)$", pk("show", str(packet)), re.M).group(1).strip())
    flat_path = Path(pk("path", str(packet), "flat").strip())
    audio = run_dir / "pass" / "music.wav"
    plan = json.loads((run_dir / "plan.json").read_text())
    scenes = plan["scenes"]
    chain = packet_chain(packet)
    direction = {}
    for d in reversed(chain):
        direction.update(d.get("set", {}).get("direction", {}))
    cues, plates = direction.get("cues", []), direction.get("collage", [])
    vocab = next((packet.parent / d["vocab"] for d in chain if "vocab" in d), ROOT / "vocab")
    media = next((packet.parent / d["media"] for d in chain if "media" in d), ROOT / "media")

    base = sha("".join([
        scene_independent(chain),
        "".join(sha_file(p) for p in sorted(vocab.glob("*.json"))),
        sha_file(run_dir / "analysis.json"),
        sha_file(run_dir / "anchors.json"),
        sha_file(tdir / "cinema-flat"),
        sha_file(tdir / "cinema-lens"),
        json.dumps(plan.get("recording"), sort_keys=True),
    ]).encode())

    # Frame ranges: each scene runs from its cut to the next, so the scenes tile the video.
    starts = [round(s["t0"] * FPS) for s in scenes]
    total = round(float(plan["recording"]["duration_s"]) * FPS)
    ranges = [(starts[i], starts[i + 1] if i + 1 < len(starts) else total) for i in range(len(starts))]

    def fingerprint(i: int) -> str:
        t0, t1 = scenes[i]["t0"], scenes[i]["t1"]
        near = [scenes[j] for j in (i - 1, i, i + 1) if 0 <= j < len(scenes)]
        cs = [c for c in cues if t0 - 1.0 <= c["t"] <= t1 + 0.5]
        ps = [dict(p, _sha=sha_file(media / p["image"])) for p in plates
              if p.get("t0", 0.0) - 1.0 <= t1 and (p.get("t1") is None or p["t1"] + 1.0 >= t0)]
        return sha(json.dumps([base, near, ranges[i], cs, ps], sort_keys=True).encode())[:16]

    sdir = run_dir / "scenes"
    sdir.mkdir(exist_ok=True)
    mpath = sdir / "manifest.json"
    manifest = json.loads(mpath.read_text()) if mpath.exists() else {}
    fps_now = [fingerprint(i) for i in range(len(scenes))]
    file = lambda i: sdir / f"scene_{i:02d}.mp4"
    stale = [i for i in range(len(scenes)) if not file(i).exists() or manifest.get(str(i), {}).get("fp") != fps_now[i]]

    if a.list:
        for i, s in enumerate(scenes):
            f0, f1 = ranges[i]
            print(f"{i:3d}  {s['t0']:7.2f}-{s['t1']:7.2f}  frames {f0}..{f1}  {'CHANGED' if i in stale else 'cached '}  {s.get('text') or s['dominant']}")
        return

    todo = [int(x) for x in a.scenes.split(",")] if a.scenes else stale
    print(f"[*] {len(scenes)} scenes, {len(todo)} to render: {todo}")
    for i in todo:
        f0, f1 = ranges[i]
        e = dict(env, CINEMA_OFFSET_S=f"{f0 / FPS:.9f}")
        rng = f"{f0}..{f1}"
        sh([str(tdir / "cinema-flat"), "render", rng, "-o", str(flat_path)], env=e)
        tmp = sdir / f"scene_{i:02d}.part.mp4"
        sh([str(tdir / "cinema-lens"), "render", rng, "-o", str(tmp)], env=e)
        tmp.replace(file(i))
        manifest[str(i)] = {"fp": fps_now[i], "frames": [f0, f1]}
        mpath.write_text(json.dumps(manifest, indent=1))
        print(f"[+] scene {i} ({rng}) -> {file(i).name}")

    missing = [i for i in range(len(scenes)) if not file(i).exists()]
    if a.no_stitch or missing:
        if missing:
            print(f"[-] not stitching: scenes {missing} have no file")
        return
    lst = sdir / "concat.txt"
    lst.write_text("".join(f"file '{file(i)}'\n" for i in range(len(scenes))))
    out = run_dir / "cinema_scenes.mp4"
    sh(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", str(lst), "-i", str(audio),
        "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-ar", "44100",
        "-dn", "-movflags", "+faststart", str(out)])
    print(f"[Success] {out}")


if __name__ == "__main__":
    main()
