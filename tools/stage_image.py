#!/usr/bin/env python3
"""Put a whole image into the packet's media folder under a plain name, for a collage plate.

    tools/stage_image.py SRC NAME [--max 1920] [--media media]

Both renderers load every file in the media folder when they start, so only images a packet
uses should be staged, and each is limited to `--max` pixels on its long side (never enlarged).
The result is `NAME.png`, which a plate names in its `image` field. A transparent background is kept.
"""

import argparse
from pathlib import Path

from PIL import Image


def stage(src, name: str, max_side: int = 1920, media: str = "media") -> Path:
    """Write `src` as `media/NAME.png`, limited to `max_side` pixels on its long side."""
    im = Image.open(src)
    im = im.convert("RGBA" if "A" in im.getbands() else "RGB")
    w, h = im.size
    k = min(1.0, max_side / max(w, h))
    if k < 1.0:
        im = im.resize((round(w * k), round(h * k)), Image.LANCZOS)
    media = Path(media)
    media.mkdir(parents=True, exist_ok=True)
    out = media / f"{name}.png"
    im.save(out)
    print(f"{src} {w}x{h} -> {out} {im.size[0]}x{im.size[1]}")
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("name")
    ap.add_argument("--max", type=int, default=1920)
    ap.add_argument("--media", default="media")
    a = ap.parse_args()
    stage(a.src, a.name, a.max, a.media)


if __name__ == "__main__":
    main()
