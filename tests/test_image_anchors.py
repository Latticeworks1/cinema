"""Anchor detection logic that does not need a model: configuration, calibration, filters and edits."""
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

from PIL import Image

root = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("image_anchors", root / "python/image_anchors.py")
ia = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ia)


class FakeBackend:
    name = "fake"
    spec = {"model": "fake/model"}

    def __init__(self, found):
        self.found = found

    def run(self, image, prompts):
        return [f for f in self.found if f[0] in prompts]


def config(**over):
    cfg = json.loads((root / "vocab/anchors.json").read_text())
    cfg.update(over)
    return cfg


class ConfigTests(unittest.TestCase):
    def write(self, cfg):
        p = Path(tempfile.mkdtemp()) / "a.json"
        p.write_text(json.dumps(cfg))
        return p

    def test_the_shipped_configuration_loads(self):
        self.assertIn(ia.load_config()["backend"], ia.load_config()["backends"])

    def test_unknown_keys_and_bad_values_are_refused(self):
        for bad in ({"surprise": 1}, {"backend": "nope"}, {"nms_iou": 0}, {"min_side": 0.9, "max_side": 0.5}, {"prompts": []}):
            with self.assertRaises(SystemExit, msg=str(bad)):
                ia.load_config(self.write(config(**bad)))


class DetectTests(unittest.TestCase):
    def image(self, alpha=None):
        d = Path(tempfile.mkdtemp())
        p = d / "x.png"
        Image.new("RGBA" if alpha is not None else "RGB", (100, 100), (200, 100, 50) + ((alpha,) if alpha is not None else ())).save(p)
        return p

    def test_the_noise_floor_from_negative_prompts_raises_the_threshold(self):
        found = [("flower", 0.30, [0.1, 0.1, 0.4, 0.4]), ("flower", 0.16, [0.5, 0.5, 0.8, 0.8]), ("car", 0.2, [0.0, 0.0, 0.3, 0.3])]
        cfg = config(calibration={"margin": 1.25, "floor": 0.05}, negative_prompts=["car"])
        rec = ia.detect(self.image(), cfg, FakeBackend(found))
        self.assertAlmostEqual(rec["noise_floor"], 0.2)
        self.assertAlmostEqual(rec["threshold"], 0.25)
        self.assertEqual([b["score"] for b in rec["boxes"]], [0.3])

    def test_size_limits_and_overlap_are_applied(self):
        found = [("flower", 0.5, [0.1, 0.1, 0.3, 0.3]), ("leaf", 0.4, [0.11, 0.11, 0.31, 0.31]), ("tree", 0.9, [0, 0, 1, 1]), ("petal", 0.9, [0.5, 0.5, 0.51, 0.51])]
        rec = ia.detect(self.image(), config(), FakeBackend(found))
        self.assertEqual([b["label"] for b in rec["boxes"]], ["flower"])
        self.assertEqual(rec["boxes"][0]["id"], "a0")

    def test_boxes_over_transparent_pixels_are_dropped(self):
        found = [("flower", 0.5, [0.1, 0.1, 0.4, 0.4])]
        self.assertEqual(ia.detect(self.image(alpha=0), config(), FakeBackend(found))["boxes"], [])
        self.assertEqual(len(ia.detect(self.image(alpha=255), config(), FakeBackend(found))["boxes"]), 1)

    def test_a_record_is_current_until_the_file_or_configuration_changes(self):
        img = self.image()
        adir = Path(tempfile.mkdtemp())
        cfg = config()
        self.assertFalse(ia.is_current(adir, img, cfg))
        ia.detected_path(adir, img).write_text(json.dumps(ia.detect(img, cfg, FakeBackend([]))))
        self.assertTrue(ia.is_current(adir, img, cfg))
        self.assertFalse(ia.is_current(adir, img, config(max_boxes=3)))


class MergeTests(unittest.TestCase):
    def test_edits_hide_rename_and_add_without_touching_the_detection(self):
        img = Path(tempfile.mkdtemp()) / "p.png"
        adir = Path(tempfile.mkdtemp())
        rec = {"boxes": [{"id": "a0", "label": "flower", "score": 0.5, "box": [0.1, 0.1, 0.3, 0.3]},
                         {"id": "a1", "label": "leaf", "score": 0.4, "box": [0.5, 0.5, 0.7, 0.7]}]}
        ia.detected_path(adir, img).write_text(json.dumps(rec))
        ia.edits_path(adir, img).write_text(json.dumps({"hide": ["a1"], "rename": {"a0": "plum blossom"}, "add": [{"label": "sun", "box": [0.8, 0.0, 1.0, 0.2]}]}))
        merged = ia.merge(adir, img)
        self.assertEqual([(a["id"], a["label"]) for a in merged], [("a0", "plum blossom"), ("h0", "sun")])
        self.assertEqual(len(json.loads(ia.detected_path(adir, img).read_text())["boxes"]), 2)

    def test_a_degenerate_added_box_is_refused(self):
        img = Path(tempfile.mkdtemp()) / "p.png"
        adir = Path(tempfile.mkdtemp())
        ia.edits_path(adir, img).write_text(json.dumps({"hide": [], "rename": {}, "add": [{"label": "x", "box": [0.5, 0.5, 0.4, 0.6]}]}))
        with self.assertRaises(SystemExit):
            ia.merge(adir, img)


if __name__ == "__main__":
    unittest.main()
