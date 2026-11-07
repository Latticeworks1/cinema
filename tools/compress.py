#!/usr/bin/env python3
"""Write a smaller 1080p copy of a finished render beside it.

    tools/compress.py VIDEO [--crf 27] [--out VIDEO_1080p.mp4]

The video is re-encoded as H.264 at the given constant rate factor (higher is smaller; 27 takes a
dot-grid render from about 1.9 GB to about 250 MB, 23 gives roughly twice that), scaled to 1920x1080
only when it is larger, and the audio stream is copied as it is. The source is never touched or
deleted. The copy's duration is checked against the source's before the tool reports success.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path


def probe(path):
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration,size:stream=codec_type,width,height", "-of", "json", str(path)],
                       capture_output=True, text=True, check=True)
    return json.loads(r.stdout)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("video")
    ap.add_argument("--crf", type=int, default=27)
    ap.add_argument("--out")
    a = ap.parse_args()
    src = Path(a.video)
    out = Path(a.out) if a.out else src.with_name(src.stem.replace("cinema_scenes", "cinema") + "_1080p.mp4")
    if out.resolve() == src.resolve():
        sys.exit("[!] Error: the output would overwrite the source")
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(src), "-map", "0:v:0", "-map", "0:a:0?",
                    "-vf", "scale='min(1920,iw)':-2", "-c:v", "libx264", "-preset", "medium", "-crf", str(a.crf),
                    "-pix_fmt", "yuv420p", "-c:a", "copy", "-movflags", "+faststart", str(out)], check=True)
    d0, d1 = float(probe(src)["format"]["duration"]), float(probe(out)["format"]["duration"])
    if abs(d0 - d1) > 0.2:
        sys.exit(f"[!] Error: duration {d1:.2f} s differs from the source's {d0:.2f} s")
    print(f"{out} {out.stat().st_size / 1e6:.0f} MB (source {src.stat().st_size / 1e6:.0f} MB), {d1:.1f} s")


if __name__ == "__main__":
    main()
