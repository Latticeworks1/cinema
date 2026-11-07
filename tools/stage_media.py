#!/usr/bin/env python3
"""Stage a packet's collage images from its media manifest and cut out their subjects.

    tools/stage_media.py MANIFEST [--media media] [--force]

The manifest (`data/NAME_media.json`) is the record of which pictures a packet uses and why:

    board     folder holding the source files
    stage     list of {"src": file in the board, "name": staged name, "max": long side, optional}
    present   staged names that were put in the media folder earlier and must still be there
    no_cutout staged names that stay whole pictures because their subject lift is not usable
    excluded  {"file": reason} for board images that were looked at and rejected

Every staged image gets a `cut_` file made by the Vision subject lift unless it is listed under
`no_cutout` or the lift finds no usable subject. Nothing is overwritten when the result is already
newer than its source, so running the tool again costs nothing.
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import stage_image  # noqa: E402
import subject_cutout  # noqa: E402

KEYS = {"board", "stage", "present", "no_cutout", "excluded"}


def fail(msg):
    sys.exit(f"[!] Error: {msg}")


def load(path):
    m = json.loads(Path(path).read_text())
    if set(m) != KEYS:
        fail(f"media manifest: missing {sorted(KEYS - set(m))}, unknown {sorted(set(m) - KEYS)}")
    names = [e["name"] for e in m["stage"]]
    if len(set(names)) != len(names):
        fail("media manifest: a staged name is listed twice")
    for e in m["stage"]:
        if not {"src", "name"} <= set(e) <= {"src", "name", "max"}:
            fail(f"media manifest: bad stage entry {e}")
    return m


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("manifest")
    ap.add_argument("--media", default="media")
    ap.add_argument("--force", action="store_true")
    a = ap.parse_args()
    m = load(a.manifest)
    media, board = Path(a.media), Path(m["board"]).expanduser()
    for e in m["stage"]:
        src = board / e["src"]
        out = media / f"{e['name']}.png"
        if out.exists() and out.stat().st_mtime > src.stat().st_mtime and not a.force:
            continue
        if not src.exists():
            fail(f"{src} is missing")
        stage_image.stage(src, e["name"], e.get("max", 1920), media)
    missing = [n for n in m["present"] if not (media / f"{n}.png").exists()]
    if missing:
        fail(f"staged earlier but missing from {media}: {missing}")
    keep = {e["name"] for e in m["stage"]} | set(m["present"])
    for n in sorted(keep):
        cut = media / f"cut_{n}.png"
        if n in m["no_cutout"]:
            cut.unlink(missing_ok=True)
            continue
        src = media / f"{n}.png"
        if cut.exists() and cut.stat().st_mtime > src.stat().st_mtime and not a.force:
            continue
        cut.unlink(missing_ok=True)
        print(f"{n}: {subject_cutout.cut(src, cut, 0.03, 0.85)}")


if __name__ == "__main__":
    main()
