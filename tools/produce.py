#!/usr/bin/env python3
"""Make a packet's finished video from its sources, in the order the steps depend on each other.

    tools/produce.py PACKET [--skip media,collage] [--small] [--crf 27]

    plan     analysis and scene plan (the collage generator reads them)
    media    stage the packet's images and cut out their subjects (PROJECT/data/NAME_media.json, if present)
    collage  generate the collage plates (vocab/collage.json, PROJECT/data/NAME_collage.json, PROJECT/data/NAME_plates.json)
    render   anchors for the plates in use, then every scene whose fingerprint changed, then the stitch
    small    a 1080p copy at a low bitrate (only with --small)

Each step is the tool that already does it, run from the repository root; nothing here has settings
of its own. A step that is already current costs almost nothing, so the sequence can be run again
after any edit.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STEPS = ["plan", "media", "collage", "render", "small"]


def run(*cmd):
    print("[>]", " ".join(map(str, cmd)), flush=True)
    r = subprocess.run([str(c) for c in cmd], cwd=ROOT)
    if r.returncode:
        sys.exit(f"[!] {cmd[1] if len(cmd) > 1 else cmd[0]} failed with status {r.returncode}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("packet")
    ap.add_argument("--skip", default="", help="comma-separated steps to leave out")
    ap.add_argument("--small", action="store_true")
    ap.add_argument("--crf", type=int, default=27)
    a = ap.parse_args()
    skip = set(filter(None, a.skip.split(",")))
    if skip - set(STEPS):
        sys.exit(f"[!] unknown step {sorted(skip - set(STEPS))}; the steps are {STEPS}")
    packet = Path(a.packet)
    name = json.loads((ROOT / packet).read_text())["name"]
    py = sys.executable
    project = (ROOT / packet).resolve().parent.parent
    media = project / "data" / f"{name}_media.json"
    if "plan" not in skip:
        run(py, "tools/render_scenes.py", packet, "--prepare", "--list")
    if "media" not in skip and media.exists():
        run(py, "tools/stage_media.py", media, "--media", project / "media")
    if "collage" not in skip:
        run(py, "tools/make_collage.py", packet)
    if "render" not in skip:
        run(py, "tools/render_scenes.py", packet, "--prepare")
    if a.small:
        run(py, "tools/compress.py", ROOT / f"out/{name}/cinema_scenes.mp4", "--crf", a.crf)


if __name__ == "__main__":
    main()
