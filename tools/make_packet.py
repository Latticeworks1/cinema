#!/usr/bin/env python3
"""Write a packet for any audio file, under any name and in any format ffmpeg reads.

    tools/make_packet.py AUDIO [--notes NOTES.json] [--name NAME] [--force] [--packets DIR]

The packet extends packets/default.json and names the audio by its absolute path, so the file stays
wherever it is. With a notes file the run is filmed from both; without one the packet runs
tools/transcribe.py on the audio first. The run name defaults to the file's name with anything other
than letters and digits turned into underscores, and the packet is written to packets/NAME.json.
"""

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("audio")
    ap.add_argument("--notes")
    ap.add_argument("--name")
    ap.add_argument("--force", action="store_true")
    ap.add_argument("--packets", default=str(ROOT / "packets"), help="folder to write the packet into")
    a = ap.parse_args()
    audio = Path(a.audio).expanduser().resolve()
    if not audio.is_file():
        sys.exit(f"[!] Error: {audio} is not a file")
    name = a.name or re.sub(r"[^a-z0-9]+", "_", audio.stem.lower()).strip("_")
    if not name or name.startswith("_") or name == "default":
        sys.exit(f"[!] Error: {name!r} is not usable as a run name; pass --name")
    out = Path(a.packets) / f"{name}.json"
    if out.exists() and not a.force:
        sys.exit(f"[!] Error: {out} exists; pass --force to replace it")
    doc = {"extends": "default.json", "name": name, "inputs": {"audio": str(audio), "notes": None}}
    if a.notes:
        notes = Path(a.notes).expanduser().resolve()
        if not notes.is_file():
            sys.exit(f"[!] Error: {notes} is not a file")
        doc["inputs"]["notes"] = str(notes)
    else:
        doc["transcriber"] = ["python3", "tools/transcribe.py", "{audio}", "{notes}"]
    out.write_text(json.dumps(doc, indent=2) + "\n")
    print(out)


if __name__ == "__main__":
    main()
