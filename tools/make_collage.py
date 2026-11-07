#!/usr/bin/env python3
"""Fill a packet's stretches with a seeded, music-driven collage of image plates.

    tools/make_collage.py PACKET [--config vocab/collage.json] [--overlay PROJECT/data/NAME_collage.json]
                          [--pinned PROJECT/data/NAME_plates.json] [--run out/NAME] [--media PROJECT/media]
                          [--pin-current] [--dry-run]

Plates are written into the packet's `set.direction.collage`, which this tool then owns: plates a
person authored live in the pinned file and are kept as written, and everything else in the array is
generated again on each run. `--pin-current` first copies the packet's existing collage into the
pinned file. Settings that belong to one packet, such as `quiet` windows, go in the overlay file,
which is laid over the shared config and may only change keys the config already has.

Two kinds of plate are generated, and the result depends only on the seed. Texture layers are large,
faint and zoomed past the frame, so only a region of an image shows; several are stacked at once, more
when the notes per second are high, and they are what lets an image source repeat without any
picture being recognised. Motifs are small cutout subjects (`cut_` files made by
tools/subject_cutout.py) with no frame, limited per image in count and spacing in time, kept out of
the middle band of the view where a scene carries a lyric and out of the packet's `quiet` windows.
"""

import argparse
import collections
import fnmatch
import json
import random
import sys
from pathlib import Path

from PIL import Image

KEYS = {"seed", "images", "rate", "pieces", "size", "beats", "spawn_chance", "rot_deg", "drift", "pulse",
        "gain", "opacity", "frame", "matte", "texture", "motif", "reveal", "fade_s", "lyric_dim", "margin", "max_overlap", "lyric_band", "tries", "quiet", "cutout"}
VIEW_ASPECT = 16 / 9


def fail(msg):
    sys.exit(f"[!] Error: {msg}")


def merged(base, over, where="collage overlay"):
    """Lay `over` on `base`: nested objects merge key by key, anything else replaces. A key the base lacks is refused."""
    out = dict(base)
    for k, v in over.items():
        if k not in base:
            fail(f"{where}: unknown key {k!r}")
        out[k] = merged(base[k], v, f"{where}.{k}") if isinstance(v, dict) and isinstance(base[k], dict) else v
    return out


def load_config(path, overlay=None):
    cfg = json.loads(Path(path).read_text())
    if overlay is not None and Path(overlay).exists():
        cfg = merged(cfg, json.loads(Path(overlay).read_text()))
    if set(cfg) != KEYS:
        fail(f"collage config: missing {sorted(KEYS - set(cfg))}, unknown {sorted(set(cfg) - KEYS)}")
    for k, spec in (("rate", {"sparse", "busy"}), ("pieces", {"sparse", "busy"}), ("size", {"min", "max"}),
                    ("beats", {"min", "max"}), ("gain", {"min", "max"}),
                    ("reveal", {"initial", "by"}), ("cutout", {"prefix"}), ("opacity", {"min", "max"}),
                    ("texture", {"layers_sparse", "layers_busy", "zoom_min", "zoom_max", "visible", "opacity_min", "opacity_max", "rot_deg", "drift", "beats_min", "beats_max"}),
                    ("motif", {"max", "gap_s"})):
        if not isinstance(cfg[k], dict) or set(cfg[k]) != spec:
            fail(f"collage config {k}: needs exactly {sorted(spec)}")
    if not cfg["rate"]["sparse"] < cfg["rate"]["busy"]:
        fail("rate.sparse must be below rate.busy")
    if not 0 < cfg["size"]["min"] <= cfg["size"]["max"] <= 1 or not 1 <= cfg["beats"]["min"] <= cfg["beats"]["max"]:
        fail("size and beats ranges are inconsistent")
    lo, hi = cfg["lyric_band"]
    if not isinstance(cfg["images"], list) or not cfg["images"] or not all(isinstance(g, str) for g in cfg["images"]):
        fail("images must be a non-empty list of file patterns")
    if not 0 <= lo < hi <= 1:
        fail("lyric_band must be [lo, hi] inside 0..1")
    if not isinstance(cfg["quiet"], list) or any(not (isinstance(w, list) and len(w) == 2 and w[0] < w[1]) for w in cfg["quiet"]):
        fail("quiet must be a list of [t0, t1] windows")
    return cfg


def overlap(a, b):
    ix = max(0.0, min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0]))
    iy = max(0.0, min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1]))
    return ix * iy / min(a[2] * a[3], b[2] * b[3])


def note_rate(stems, t0, t1):
    n = sum(1 for notes in stems.values() for x in notes if t0 <= x["onset"] < t1)
    return n / max(t1 - t0, 1e-6)


def generate(cfg, scenes, stems, rec, images, cutouts=None):
    """Two kinds of plate. Texture layers are large, faint, zoomed far past the frame so that only a
    region of the picture shows, and several are stacked at once; they carry the image sources, which
    can therefore repeat without any one picture being recognised. Motifs are small cutout subjects
    with no frame, each limited to `motif.max` appearances at least `motif.gap_s` apart."""
    rng = random.Random(cfg["seed"])
    beat = 60.0 / rec["bpm"]
    drop = rec["drop_t"]
    out = []
    # Images are held back: `reveal.initial` are available from the start and the rest unlock one
    # at a time, evenly, until `reveal.by` of the way through the recording, so later scenes still
    # have something new to show. The first `initial` keep their sorted order; the rest are shuffled
    # with the seed so that images from different sources are interleaved as they unlock.
    rv = cfg["reveal"]
    head, tail = list(images[:rv["initial"]]), list(images[rv["initial"]:])
    random.Random(cfg["seed"] + 1).shuffle(tail)
    images = head + tail
    rest = max(len(images) - rv["initial"], 1)
    unlock = {n: (0.0 if i < rv["initial"] else (i - rv["initial"] + 1) / rest * rv["by"] * rec["duration_s"])
              for i, (n, _) in enumerate(images)}
    a, b = cfg["rate"]["sparse"], cfg["rate"]["busy"]
    tx, mo = cfg["texture"], cfg["motif"]
    motif_times = {}  # source image -> start times of its motif appearances
    for sc in scenes:
        t0, t1 = sc["t0"], sc["t1"]
        u = min(1.0, max(0.0, (note_rate(stems, t0, t1) - a) / (b - a)))
        cap = round(cfg["pieces"]["sparse"] + u * (cfg["pieces"]["busy"] - cfg["pieces"]["sparse"]))
        layers = round(tx["layers_sparse"] + u * (tx["layers_busy"] - tx["layers_sparse"]))
        lyric = bool(sc.get("text"))
        tex, pieces = [], []  # (end, image)  and  (end, rect, image)
        k = int((t0 - drop) // beat) + 1
        while True:
            t = drop + k * beat
            k += 1
            if t >= t1 - 2 * beat:
                break
            tex = [x for x in tex if x[0] > t]
            pieces = [x for x in pieces if x[0] > t]
            seen = [im for im in images if unlock[im[0]] <= t] or images[:1]
            used = collections.Counter(q["image"] for q in out)

            def weight(im):
                n = used.get(im[0], 0) + used.get((cutouts or {}).get(im[0], ("",))[0], 0)
                return 1.0 / (1 + n) ** 2

            for _ in range(3):  # a scene fills up within a couple of beats
                if len(tex) >= layers or rng.random() > cfg["spawn_chance"]:
                    break
                free = [im for im in seen if im[0] not in {x[1] for x in tex}] or seen
                name, (iw, ih) = rng.choices(free, weights=[weight(im) for im in free])[0]
                zoom = rng.uniform(tx["zoom_min"], tx["zoom_max"])
                h = zoom
                w = h * iw / (ih * VIEW_ASPECT)
                v = tx["visible"]  # at least this much of the layer's width and height overlaps the view
                x = rng.uniform(v - w, 1 - v)
                y = rng.uniform(v - h, 1 - v)
                end = min(t + beat * rng.randint(tx["beats_min"], tx["beats_max"]), t1)
                out.append({"image": name, "t0": round(t, 3), "t1": round(end, 3), "x": round(x, 4), "y": round(y, 4),
                            "w": round(w, 4), "h": round(h, 4), "space": "view", "fit": "meet",
                            "rot": round(rng.uniform(-tx["rot_deg"], tx["rot_deg"]), 2),
                            "opacity": round(rng.uniform(tx["opacity_min"], tx["opacity_max"]), 2),
                            "fade_s": cfg["fade_s"] * 3, "lyric_dim": 1.0, "pulse": cfg["pulse"],
                            "drift": [round(rng.uniform(-tx["drift"], tx["drift"]), 4), round(rng.uniform(-tx["drift"], tx["drift"]), 4)],
                            "layer": "under", "tone": {"gain": round(rng.uniform(cfg["gain"]["min"], cfg["gain"]["max"]), 2)}})
                tex.append((end, name))

            if len(pieces) >= cap or rng.random() > cfg["spawn_chance"]:
                continue
            for _ in range(cfg["tries"]):
                free = [im for im in seen if im[0] not in {x[2] for x in pieces}] or seen
                if cutouts is not None:
                    # a motif needs a cutout, and an image that has been shown enough or shown recently waits
                    free = [im for im in free if im[0] in cutouts
                            and len(motif_times.get(im[0], [])) < mo["max"]
                            and all(t - x >= mo["gap_s"] for x in motif_times.get(im[0], []))]
                if not free:
                    break
                name, (iw, ih) = rng.choices(free, weights=[weight(im) for im in free])[0]
                src = name
                cut = cutouts is not None
                if cut:
                    name, (iw, ih) = cutouts[name]
                w = rng.uniform(cfg["size"]["min"], cfg["size"]["max"])
                h = w * VIEW_ASPECT * ih / iw
                if h > 0.8:
                    w, h = w * 0.8 / h, 0.8
                m = cfg["margin"]
                x, y = rng.uniform(m, 1 - m - w), rng.uniform(m, 1 - m - h)
                r = (x, y, w, h)
                if lyric and y < cfg["lyric_band"][1] and y + h > cfg["lyric_band"][0]:
                    continue
                if any(overlap(r, o[1]) > cfg["max_overlap"] for o in pieces):
                    continue
                end = min(t + beat * rng.randint(cfg["beats"]["min"], cfg["beats"]["max"]), t1)
                pieces.append((end, r, src))
                motif_times.setdefault(src, []).append(t)
                out.append({"image": name, "t0": round(t, 3), "t1": round(end, 3), "x": round(x, 4), "y": round(y, 4),
                            "w": round(w, 4), "h": round(h, 4), "space": "view", "fit": "meet",
                            "rot": round(rng.uniform(-cfg["rot_deg"], cfg["rot_deg"]), 2),
                            "opacity": round(rng.uniform(cfg["opacity"]["min"], cfg["opacity"]["max"]), 2),
                            "fade_s": cfg["fade_s"], "lyric_dim": cfg["lyric_dim"], "pulse": cfg["pulse"],
                            "drift": [round(rng.uniform(-cfg["drift"], cfg["drift"]), 4), round(rng.uniform(-cfg["drift"], cfg["drift"]), 4)],
                            "layer": "over" if rng.random() < 0.5 else "under",
                            "frame": 0.0 if cut else cfg["frame"], "matte": 0.0 if cut else cfg["matte"],
                            "tone": {"gain": round(rng.uniform(cfg["gain"]["min"], cfg["gain"]["max"]), 2)}})
                break
    return [p for p in out if p["lyric_dim"] >= 1.0 or not any(p["t0"] < b and p["t1"] > a for a, b in cfg["quiet"])]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("packet")
    ap.add_argument("--config", default=str(Path(__file__).resolve().parent.parent / "vocab" / "collage.json"))
    ap.add_argument("--overlay", help="per-packet settings laid over the config (default PROJECT/data/NAME_collage.json)")
    ap.add_argument("--pinned")
    ap.add_argument("--run")
    ap.add_argument("--media")
    ap.add_argument("--pin-current", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()

    pkt = Path(a.packet)
    doc = json.loads(pkt.read_text())
    project = pkt.resolve().parent.parent
    cfg = load_config(a.config, a.overlay or str(project / "data" / f"{doc['name']}_collage.json"))
    run = Path(a.run or f"out/{doc['name']}")
    pinned_path = Path(a.pinned or project / "data" / f"{doc['name']}_plates.json")
    direction = doc.setdefault("set", {}).setdefault("direction", {})
    if a.pin_current:
        if pinned_path.exists():
            fail(f"{pinned_path} exists; refusing to overwrite pinned plates")
        pinned_path.write_text(json.dumps(direction.get("collage", []), indent=1) + "\n")
        print(f"pinned {len(direction.get('collage', []))} plates -> {pinned_path}")
    pinned = json.loads(pinned_path.read_text()) if pinned_path.exists() else []

    plan = json.loads((run / "plan.json").read_text())
    stems = json.loads((run / "analysis.json").read_text())["stems"]
    media = Path(a.media) if a.media else project / "media"
    pre = cfg["cutout"]["prefix"]
    images = [(p.name, Image.open(p).size) for p in sorted(media.iterdir())
              if not p.name.startswith(pre) and any(fnmatch.fnmatch(p.name, g) for g in cfg["images"])]
    if not images:
        fail(f"no image in {media} matches {cfg['images']}")
    cutouts = {n: (pre + n, Image.open(media / (pre + n)).size) for n, _ in images if (media / (pre + n)).exists()}
    made = generate(cfg, plan["scenes"], stems, plan["recording"], images, cutouts or None)
    allp = sorted(pinned + made, key=lambda p: p.get("t0", 0))
    print(f"{len(pinned)} pinned + {len(made)} generated plates")
    if not a.dry_run:
        direction["collage"] = allp
        pkt.write_text(json.dumps(doc, indent=2) + "\n")


if __name__ == "__main__":
    main()
