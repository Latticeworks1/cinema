#!/usr/bin/env python3
"""Find and inspect recognizable motifs in the images a packet uses.

    tools/anchors.py detect IMAGE... [--config vocab/anchors.json] [--dir data/anchors] [--backend NAME] [--force]
    tools/anchors.py show   IMAGE... [--dir data/anchors]
    tools/anchors.py sheet  IMAGE... [--dir data/anchors] --out sheet.png
    tools/anchors.py compare IMAGE... --backends owlv2,grounding_dino --out sheet.png
    tools/anchors.py prepare --direction D.json --media media --dir data/anchors --out anchors.json
    tools/anchors.py models [--config vocab/anchors.json]
    tools/anchors.py prune-models [--config vocab/anchors.json] [--keep NAME]

`detect` writes IMAGE.detected.json into the anchors folder and skips images whose file, configuration
and schema are unchanged. Hand edits live in IMAGE.anchors.json beside it (hide, rename, add) and
survive re-detection. `prepare` (run by `packet prepare`) detects what is stale for the images a\ndirection's plates use and writes the merged result the planner reads. Model weights are kept in the configuration's cache_dir; `models` lists their
size and `prune-models` deletes the ones a backend choice no longer needs.
"""

import argparse
import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "python"))
import image_anchors as ia  # noqa: E402
from PIL import Image, ImageDraw, ImageFont  # noqa: E402

TICK = 0.22


def draw_marks(im, anchors, font):
    """Recognition squares: corner brackets around each box with a small label under the top-left corner."""
    d = ImageDraw.Draw(im)
    w, h = im.size
    for a in anchors:
        x0, y0, x1, y1 = a["box"][0] * w, a["box"][1] * h, a["box"][2] * w, a["box"][3] * h
        t = max(4, min(x1 - x0, y1 - y0) * TICK)
        for (cx, cy, sx, sy) in ((x0, y0, 1, 1), (x1, y0, -1, 1), (x0, y1, 1, -1), (x1, y1, -1, -1)):
            d.line([(cx, cy), (cx + sx * t, cy)], fill=(255, 70, 50), width=2)
            d.line([(cx, cy), (cx, cy + sy * t)], fill=(255, 70, 50), width=2)
        d.text((x0 + 3, y0 + 3), f"{a['label']} {a['score']:.2f}", fill=(255, 255, 255), font=font, stroke_width=1, stroke_fill=(0, 0, 0))
    return im


def sheet(images, anchors_for, out, cell=420):
    font = ImageFont.load_default()
    tiles = []
    for p in images:
        rgb, _ = ia.load_image(p)
        k = cell / max(rgb.size)
        rgb = rgb.resize((max(1, round(rgb.size[0] * k)), max(1, round(rgb.size[1] * k))))
        tiles.append(draw_marks(rgb, anchors_for(p), font))
    cols = min(3, len(tiles))
    rows = -(-len(tiles) // cols)
    canvas = Image.new("RGB", (cols * cell, rows * cell), (20, 20, 20))
    for i, t in enumerate(tiles):
        canvas.paste(t, ((i % cols) * cell, (i // cols) * cell))
    canvas.save(out)
    print(f"wrote {out} ({len(tiles)} images)")


def backend_for(cfg, name):
    return ia.Backend(name, cfg["backends"][name], cfg)


def cmd_detect(a):
    cfg = ia.load_config(a.config)
    if a.backend:
        cfg = dict(cfg, backend=a.backend)
        if a.backend not in cfg["backends"]:
            ia.fail(f"unknown backend {a.backend!r}")
    adir = Path(a.dir)
    adir.mkdir(parents=True, exist_ok=True)
    todo = [Path(p) for p in a.images if a.force or not ia.is_current(adir, Path(p), cfg)]
    for p in a.images:
        if Path(p) not in todo:
            print(f"{p}: current")
    if not todo:
        return
    be = backend_for(cfg, cfg["backend"])
    for p in todo:
        rec = ia.detect(p, cfg, be)
        ia.detected_path(adir, p).write_text(json.dumps(rec, indent=1) + "\n")
        print(f"{p}: {len(rec['boxes'])} anchors, noise {rec['noise_floor']}, threshold {rec['threshold']}")


def cmd_prepare(a):
    """Detect (where stale) for every image the direction's plates use, then write the merged anchors."""
    cfg = ia.load_config(a.config)
    direction = json.loads(Path(a.direction).read_text())
    names = sorted({p["image"] for p in direction.get("collage", [])})
    media, adir = Path(a.media), Path(a.dir)
    missing = [n for n in names if not (media / n).exists()]
    if missing:
        ia.fail(f"plates name images absent from {media}: {missing}")
    adir.mkdir(parents=True, exist_ok=True)
    stale = [media / n for n in names if not ia.is_current(adir, media / n, cfg)]
    if stale:
        be = backend_for(cfg, cfg["backend"])
        for p in stale:
            rec = ia.detect(p, cfg, be)
            ia.detected_path(adir, p).write_text(json.dumps(rec, indent=1) + "\n")
            print(f"anchors: {p.name}: {len(rec['boxes'])} found")
    images = {}
    for n in names:
        p = media / n
        size = json.loads(ia.detected_path(adir, p).read_text())["size"]
        images[n] = {"size": size, "boxes": [{"id": b["id"], "label": b["label"], "score": b["score"], "box": b["box"]} for b in ia.merge(adir, p)]}
    Path(a.out).write_text(json.dumps({"schema": ia.SCHEMA_VERSION, "images": images}, indent=1) + "\n")
    print(f"anchors: {sum(len(v['boxes']) for v in images.values())} anchors over {len(images)} images -> {a.out}")


def cmd_show(a):
    adir = Path(a.dir)
    for p in map(Path, a.images):
        print(p.name)
        for x in ia.merge(adir, p):
            print(f"  {x['id']:>3} {x['label']:<18} {x['score']:.2f} {x['box']}")


def cmd_sheet(a):
    sheet([Path(p) for p in a.images], lambda p: ia.merge(Path(a.dir), p), a.out)


def cmd_compare(a):
    cfg = ia.load_config(a.config)
    for name in a.backends.split(","):
        c = dict(cfg, backend=name)
        be = backend_for(c, name)
        res = {}
        for p in map(Path, a.images):
            res[p] = ia.detect(p, c, be)["boxes"]
        out = Path(a.out).with_name(f"{Path(a.out).stem}_{name}.png")
        sheet([Path(p) for p in a.images], lambda p, r=res: r[p], out)
        for p, boxes in res.items():
            print(f"{name} {p.name}: " + ", ".join(f"{b['label']} {b['score']:.2f}" for b in boxes))
        del be


def cmd_models(a):
    cfg = ia.load_config(a.config)
    root = Path(cfg["cache_dir"]).expanduser()
    if not root.exists():
        print(f"{root}: empty")
        return
    for d in sorted(root.glob("models--*")):
        size = sum(f.stat().st_size for f in d.rglob("*") if f.is_file())
        print(f"{d.name.replace('models--', '').replace('--', '/')}  {size / 1e6:.0f} MB")


def cmd_prune(a):
    cfg = ia.load_config(a.config)
    keep = cfg["backends"][a.keep or cfg["backend"]]["model"].replace("/", "--")
    root = Path(cfg["cache_dir"]).expanduser()
    for d in sorted(root.glob("models--*")):
        if d.name != f"models--{keep}":
            size = sum(f.stat().st_size for f in d.rglob("*") if f.is_file())
            shutil.rmtree(d)
            print(f"removed {d.name} ({size / 1e6:.0f} MB)")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name, fn in (("detect", cmd_detect), ("prepare", cmd_prepare), ("show", cmd_show), ("sheet", cmd_sheet), ("compare", cmd_compare), ("models", cmd_models), ("prune-models", cmd_prune)):
        s = sub.add_parser(name)
        if name not in ("models", "prune-models", "prepare"):
            s.add_argument("images", nargs="+")
        if name == "prepare":
            s.add_argument("--direction", required=True)
            s.add_argument("--media", required=True)
            s.add_argument("--out", required=True)
        s.add_argument("--config", default=str(ia.DEFAULT_CONFIG))
        s.add_argument("--dir", default="data/anchors")
        if name == "detect":
            s.add_argument("--backend")
            s.add_argument("--force", action="store_true")
        if name in ("sheet", "compare"):
            s.add_argument("--out", required=True)
        if name == "compare":
            s.add_argument("--backends", default="owlv2,grounding_dino")
        if name == "prune-models":
            s.add_argument("--keep")
        s.set_defaults(fn=fn)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
