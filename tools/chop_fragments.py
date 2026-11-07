#!/usr/bin/env python3
"""Chop a source image into a rows x cols grid of native-resolution fragments for cinema.

Each fragment is written as `{prefix}_r{row}_c{col}.png` into the packet's media folder, which is
the name `direct::settings::FragmentSet` expects. Alongside, the measured character of every
crop is recorded in a metadata file kept outside the media folder (the renderers load every
file in it): `edge` is the mean gradient magnitude and `energy` the spread of luminance, both
scaled to 0..1 across the crops of one run, so a fragment can later be placed on the same two
axes as shapes, palettes and looks.

    tools/chop_fragments.py SRC PREFIX ROWS COLS [--crop x0,y0,x1,y1] [--media media] [--meta data/fragments_meta.json]
"""

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image


def measure(im: Image.Image) -> tuple[float, float]:
    g = np.asarray(im.convert("L"), dtype=np.float32) / 255.0
    gy, gx = np.gradient(g)
    return float(np.hypot(gx, gy).mean()), float(g.std())


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("prefix")
    ap.add_argument("rows", type=int)
    ap.add_argument("cols", type=int)
    ap.add_argument("--crop", help="x0,y0,x1,y1 of the part of the source to chop")
    ap.add_argument("--media", default="media")
    ap.add_argument("--meta", default="data/fragments_meta.json")
    a = ap.parse_args()

    im = Image.open(a.src).convert("RGB")
    if a.crop:
        x0, y0, x1, y1 = (int(v) for v in a.crop.split(","))
        im = im.crop((x0, y0, x1, y1))
    w, h = im.size
    media = Path(a.media)
    media.mkdir(parents=True, exist_ok=True)

    cells = []
    for r in range(a.rows):
        for c in range(a.cols):
            box = (c * w // a.cols, r * h // a.rows, (c + 1) * w // a.cols, (r + 1) * h // a.rows)
            tile = im.crop(box)
            name = f"{a.prefix}_r{r}_c{c}.png"
            tile.save(media / name)
            edge, energy = measure(tile)
            cells.append({"file": name, "source": str(a.src), "box": box, "size": tile.size,
                          "row": r, "col": c, "edge_raw": edge, "energy_raw": energy})

    def scale(key):
        v = np.array([c[key] for c in cells])
        lo, hi = v.min(), v.max()
        return ((v - lo) / (hi - lo)) if hi > lo else np.zeros_like(v)

    for c, e, n in zip(cells, scale("edge_raw"), scale("energy_raw")):
        c["edge"], c["energy"] = round(float(e), 3), round(float(n), 3)

    meta_path = Path(a.meta)
    meta = json.loads(meta_path.read_text()) if meta_path.exists() else {}
    meta[a.prefix] = {"source": str(a.src), "rows": a.rows, "cols": a.cols, "fragments": cells}
    meta_path.write_text(json.dumps(meta, indent=1))
    print(f"{a.prefix}: {len(cells)} fragments from {a.src} ({w}x{h}), "
          f"tile {cells[0]['size'][0]}x{cells[0]['size'][1]}")


if __name__ == "__main__":
    main()
