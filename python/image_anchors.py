#!/usr/bin/env python3
"""Find recognizable motifs in still images and record them as anchors.

An anchor is a labelled box, in image-normalized coordinates, that the renderer can draw a
recognition square around and attach lines to. Detection is open-vocabulary: the prompts come
from `vocab/anchors.json`, so the same step serves any set of images and any vocabulary.

Each image yields a machine-written `<stem>.detected.json`. A person adjusts the result in a
separate `<stem>.anchors.json` (hide, rename, add), which is never overwritten; `merge()` combines
the two. The detector's noise floor is measured per image from prompts that should not match, and
only detections above margin times that floor are kept.

Library use: `load_config`, `detect`, `merge`. Command-line use goes through tools/anchors.py.
"""

import hashlib
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

SCHEMA_VERSION = 1
DEFAULT_CONFIG = Path(__file__).resolve().parent.parent / "vocab" / "anchors.json"
TOP_KEYS = {"backend", "backends", "device", "download", "cache_dir", "prompts", "negative_prompts",
            "calibration", "nms_iou", "max_boxes", "min_side", "max_side", "min_opaque"}


def fail(msg: str):
    print(f"[!] Error: {msg}", file=sys.stderr)
    sys.exit(1)


def _keys(where, d, required):
    if not isinstance(d, dict):
        fail(f"{where} must be an object")
    missing, unknown = required - d.keys(), d.keys() - required
    if missing or unknown:
        fail(f"{where}: missing {sorted(missing)}, unknown {sorted(unknown)}")


def load_config(path=DEFAULT_CONFIG) -> dict:
    """Read and validate the anchors configuration; unknown keys and out-of-range values are refused."""
    cfg = json.loads(Path(path).read_text(encoding="utf-8"))
    _keys("anchors", cfg, TOP_KEYS)
    if cfg["backend"] not in cfg["backends"]:
        fail(f"backend {cfg['backend']!r} is not among {sorted(cfg['backends'])}")
    for name, b in cfg["backends"].items():
        if not isinstance(b, dict) or not isinstance(b.get("model"), str):
            fail(f"backends.{name} needs a model id")
    if cfg["device"] not in ("auto", "cpu", "mps", "cuda"):
        fail("device must be auto, cpu, mps or cuda")
    if not isinstance(cfg["download"], bool):
        fail("download must be true or false")
    for k in ("prompts", "negative_prompts"):
        v = cfg[k]
        if not (isinstance(v, list) and all(isinstance(s, str) and s for s in v)):
            fail(f"{k} must be a list of non-empty strings")
    if not cfg["prompts"]:
        fail("prompts must not be empty")
    if set(cfg["prompts"]) & set(cfg["negative_prompts"]):
        fail("a prompt cannot also be a negative prompt")
    _keys("calibration", cfg["calibration"], {"margin", "floor"})
    if not cfg["calibration"]["margin"] >= 1 or not 0 <= cfg["calibration"]["floor"] < 1:
        fail("calibration needs margin >= 1 and floor in [0, 1)")
    if not 0 < cfg["nms_iou"] <= 1:
        fail("nms_iou must be in (0, 1]")
    if not (isinstance(cfg["max_boxes"], int) and cfg["max_boxes"] > 0):
        fail("max_boxes must be a positive integer")
    if not 0 <= cfg["min_side"] < cfg["max_side"] <= 1:
        fail("need 0 <= min_side < max_side <= 1")
    if not 0 <= cfg["min_opaque"] <= 1:
        fail("min_opaque must be in [0, 1]")
    return cfg


def config_hash(cfg: dict) -> str:
    return hashlib.sha256(json.dumps(cfg, sort_keys=True).encode()).hexdigest()[:12]


def file_sha256(path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load_image(path):
    """Return (rgb PIL image composited on black, alpha array in 0..1 or None)."""
    im = Image.open(path)
    if "A" in im.getbands() or im.mode == "P":
        rgba = im.convert("RGBA")
        alpha = np.asarray(rgba.split()[3], dtype=np.float32) / 255.0
        bg = Image.new("RGB", rgba.size, (0, 0, 0))
        bg.paste(rgba, mask=rgba.split()[3])
        return bg, alpha
    return im.convert("RGB"), None


def pick_device(name: str) -> str:
    import torch
    if name != "auto":
        return name
    if torch.backends.mps.is_available():
        return "mps"
    return "cuda" if torch.cuda.is_available() else "cpu"


class Backend:
    """Scores every prompt against one image. Returns (label, score, [x0, y0, x1, y1] normalized)."""

    def __init__(self, name, spec, cfg):
        self.name, self.spec = name, spec
        self.device = pick_device(cfg["device"])
        self.cache = str(Path(cfg["cache_dir"]).expanduser())
        self.offline = not cfg["download"]
        self._load()

    def _kw(self):
        return {"cache_dir": self.cache, "local_files_only": self.offline}

    def _load(self):
        import torch
        from transformers import AutoProcessor
        self.torch = torch
        self.processor = AutoProcessor.from_pretrained(self.spec["model"], **self._kw())
        if self.name == "owlv2":
            from transformers import Owlv2ForObjectDetection as M
        elif self.name == "grounding_dino":
            from transformers import AutoModelForZeroShotObjectDetection as M
        else:
            fail(f"no backend implementation for {self.name!r}")
        self.model = M.from_pretrained(self.spec["model"], **self._kw()).to(self.device).eval()

    def run(self, image, prompts):
        torch, (w, h) = self.torch, image.size
        if self.name == "owlv2":
            inp = self.processor(text=[prompts], images=image, return_tensors="pt").to(self.device)
            with torch.no_grad():
                out = self.model(**inp)
            side = max(w, h)  # the model pads to a square at the bottom and right
            res = self.processor.post_process_grounded_object_detection(out, threshold=0.01, target_sizes=[(side, side)])[0]
            boxes = res["boxes"].cpu().numpy() / np.array([w, h, w, h])
            return [(prompts[int(l)], float(s), np.clip(b, 0, 1).tolist())
                    for l, s, b in zip(res["labels"], res["scores"], boxes)]
        text = ". ".join(prompts) + "."
        inp = self.processor(text=text, images=image, return_tensors="pt").to(self.device)
        with torch.no_grad():
            out = self.model(**inp)
        res = self.processor.post_process_grounded_object_detection(
            out, inp["input_ids"], threshold=0.05, text_threshold=self.spec.get("text_threshold", 0.2),
            target_sizes=[(h, w)])[0]
        labels = res.get("text_labels", res.get("labels"))
        boxes = res["boxes"].cpu().numpy() / np.array([w, h, w, h])
        found = []
        for l, s, b in zip(labels, res["scores"], boxes):
            hit = [p for p in prompts if p in str(l)]
            if hit:
                found.append((max(hit, key=len), float(s), np.clip(b, 0, 1).tolist()))
        return found


def iou(a, b):
    ix = max(0.0, min(a[2], b[2]) - max(a[0], b[0]))
    iy = max(0.0, min(a[3], b[3]) - max(a[1], b[1]))
    inter = ix * iy
    union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter
    return inter / union if union > 0 else 0.0


def nms(dets, thr):
    keep = []
    for d in sorted(dets, key=lambda d: -d["score"]):
        if all(iou(d["box"], k["box"]) < thr for k in keep):
            keep.append(d)
    return keep


def detect(path, cfg, backend: Backend) -> dict:
    """Detect anchors in one image and return the sidecar record."""
    image, alpha = load_image(path)
    w, h = image.size
    raw = backend.run(image, cfg["prompts"] + cfg["negative_prompts"])
    neg = set(cfg["negative_prompts"])
    noise = max([s for l, s, _ in raw if l in neg], default=0.0)
    cal = cfg["calibration"]
    thr = max(cal["floor"], cal["margin"] * noise)
    dets = []
    for l, s, b in raw:
        if l in neg or s < thr:
            continue
        bw, bh = b[2] - b[0], b[3] - b[1]
        if max(bw, bh) < cfg["min_side"] or max(bw, bh) > cfg["max_side"]:
            continue
        if alpha is not None:
            x0, x1, y0, y1 = int(b[0] * w), max(int(b[2] * w), int(b[0] * w) + 1), int(b[1] * h), max(int(b[3] * h), int(b[1] * h) + 1)
            if float((alpha[y0:y1, x0:x1] > 0.5).mean()) < cfg["min_opaque"]:
                continue
        dets.append({"label": l, "score": round(s, 4), "box": [round(v, 4) for v in b]})
    dets = nms(dets, cfg["nms_iou"])[: cfg["max_boxes"]]
    for i, d in enumerate(dets):
        d["id"] = f"a{i}"
    return {"schema": SCHEMA_VERSION, "image": Path(path).name, "sha256": file_sha256(path), "size": [w, h],
            "backend": backend.name, "model": backend.spec["model"], "config": config_hash(cfg),
            "noise_floor": round(noise, 4), "threshold": round(thr, 4), "boxes": dets}


def detected_path(adir: Path, image: Path) -> Path:
    return adir / f"{image.stem}.detected.json"


def edits_path(adir: Path, image: Path) -> Path:
    return adir / f"{image.stem}.anchors.json"


def is_current(adir: Path, image: Path, cfg: dict) -> bool:
    p = detected_path(adir, image)
    if not p.exists():
        return False
    d = json.loads(p.read_text())
    return d.get("sha256") == file_sha256(image) and d.get("config") == config_hash(cfg) and d.get("schema") == SCHEMA_VERSION


def merge(adir: Path, image: Path) -> list:
    """Machine detections with the person's edits applied: hide ids, rename labels, add boxes."""
    anchors = []
    dp = detected_path(adir, image)
    if dp.exists():
        anchors = json.loads(dp.read_text())["boxes"]
    ep = edits_path(adir, image)
    if ep.exists():
        e = json.loads(ep.read_text())
        _keys(str(ep), e, {"hide", "rename", "add"})
        hide, rename = set(e["hide"]), e["rename"]
        anchors = [dict(a, label=rename.get(a["id"], a["label"])) for a in anchors if a["id"] not in hide]
        for i, a in enumerate(e["add"]):
            _keys(f"{ep} add[{i}]", a, {"label", "box"})
            if not (len(a["box"]) == 4 and 0 <= a["box"][0] < a["box"][2] <= 1 and 0 <= a["box"][1] < a["box"][3] <= 1):
                fail(f"{ep} add[{i}]: box must be [x0, y0, x1, y1] inside 0..1")
            anchors.append({"id": f"h{i}", "label": a["label"], "score": 1.0, "box": a["box"]})
    return anchors
