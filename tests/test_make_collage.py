"""The collage generator is deterministic, beat-aligned, music-driven and keeps the lyric band clear."""
import importlib.util
import json
import unittest
from pathlib import Path

root = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("make_collage", root / "tools/make_collage.py")
mc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mc)

CFG = mc.load_config(root / "vocab/collage.json")
REC = {"bpm": 120.0, "drop_t": 0.0, "duration_s": 60.0}
IMAGES = [("a.png", (100, 200)), ("b.png", (300, 200)), ("c.png", (200, 200))]


def stems(rate, seconds=40):
    return {"x": [{"onset": i / rate} for i in range(int(rate * seconds))]}


class CollageTests(unittest.TestCase):
    def test_the_same_seed_gives_the_same_plates(self):
        scenes = [{"t0": 0.0, "t1": 20.0}]
        self.assertEqual(mc.generate(CFG, scenes, stems(1), REC, IMAGES), mc.generate(CFG, scenes, stems(1), REC, IMAGES))

    def test_plates_enter_on_beats_and_stay_inside_their_scene(self):
        for p in mc.generate(CFG, [{"t0": 3.3, "t1": 20.0}], stems(1), REC, IMAGES):
            self.assertAlmostEqual(p["t0"] / 0.5, round(p["t0"] / 0.5), places=2)
            self.assertLessEqual(p["t1"], 20.0)
            self.assertGreaterEqual(p["t0"], 3.3)

    def test_busy_music_gets_fewer_pieces_than_sparse_music(self):
        scene = [{"t0": 0.0, "t1": 30.0}]
        small = lambda r: len([p for p in mc.generate(CFG, scene, stems(r), REC, IMAGES) if p["lyric_dim"] < 1.0])
        self.assertGreater(small(1), 0)
        self.assertEqual(small(40), 0)

    def test_a_lyric_scene_keeps_the_middle_band_empty(self):
        lo, hi = CFG["lyric_band"]
        plates = mc.generate(CFG, [{"t0": 0.0, "t1": 40.0, "text": "hello"}], stems(1), REC, IMAGES)
        self.assertTrue(plates)
        for p in (q for q in plates if q["lyric_dim"] < 1.0):
            self.assertTrue(p["y"] + p["h"] <= lo or p["y"] >= hi, p)

    def test_some_pieces_are_blown_up_past_the_edge_of_the_view(self):
        plates = mc.generate(CFG, [{"t0": 0.0, "t1": 60.0}], stems(1, 60), REC, IMAGES)
        self.assertTrue(any(p["x"] < 0 or p["y"] < 0 or p["x"] + p["w"] > 1 or p["y"] + p["h"] > 1 for p in plates))

    def test_later_images_stay_unused_until_they_unlock(self):
        imgs = [(f"{c}.png", (100, 100)) for c in "abcdef"]
        plates = mc.generate(CFG, [{"t0": 0.0, "t1": 60.0}], stems(1, 60), REC, imgs)
        early = {p["image"] for p in plates if p["t0"] < 5}
        self.assertLessEqual(len(early), CFG["reveal"]["initial"])
        self.assertTrue({p["image"] for p in plates} - early)

    def test_the_config_refuses_unknown_keys(self):
        bad = json.loads((root / "vocab/collage.json").read_text())
        bad["surprise"] = 1
        path = Path(__file__).with_name("_bad_collage.json")
        path.write_text(json.dumps(bad))
        try:
            with self.assertRaises(SystemExit):
                mc.load_config(path)
        finally:
            path.unlink()


if __name__ == "__main__":
    unittest.main()


class QuietTests(unittest.TestCase):
    def test_framed_pieces_stay_out_of_quiet_windows(self):
        cfg = dict(CFG, quiet=[[10.0, 14.0]])
        out = mc.generate(cfg, [{"t0": 0.0, "t1": 30.0}], stems(3), REC, IMAGES)
        self.assertTrue([p for p in mc.generate(CFG, [{"t0": 0.0, "t1": 30.0}], stems(3), REC, IMAGES) if p["t0"] < 14.0 and p["t1"] > 10.0 and p["lyric_dim"] < 1.0])
        for p in out:
            if p["lyric_dim"] < 1.0:
                self.assertFalse(p["t0"] < 14.0 and p["t1"] > 10.0)


class ImageSourceTests(unittest.TestCase):
    def test_images_must_be_a_list_of_patterns(self):
        import tempfile
        cfg = dict(CFG, images="spring_*.png")
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "c.json"
            path.write_text(json.dumps(cfg))
            with self.assertRaises(SystemExit):
                mc.load_config(path)

    def test_every_source_is_used_once_the_images_have_unlocked(self):
        imgs = [(f"{p}{i}.png", (200, 200)) for p in ("s", "l") for i in range(6)]
        names = {p["image"] for p in mc.generate(CFG, [{"t0": 50.0, "t1": 100.0}], stems(3), dict(REC, duration_s=100.0), imgs)}
        self.assertTrue(any(n.startswith("s") for n in names) and any(n.startswith("l") for n in names))


class CutoutTests(unittest.TestCase):
    def test_small_pieces_are_cutouts_without_a_frame_and_other_images_are_only_large(self):
        imgs = [("a.png", (200, 200)), ("b.png", (200, 200)), ("c.png", (200, 200))]
        cuts = {"a.png": ("cut_a.png", (80, 120)), "b.png": ("cut_b.png", (150, 90))}
        out = mc.generate(CFG, [{"t0": 0.0, "t1": 60.0}], stems(3), dict(REC, duration_s=100.0), imgs, cuts)
        small = [p for p in out if p["lyric_dim"] < 1.0]
        self.assertTrue(small)
        for p in small:
            self.assertTrue(p["image"].startswith("cut_"), p)
            self.assertEqual((p["frame"], p["matte"]), (0.0, 0.0))
        self.assertTrue(all(p["image"] != "c.png" for p in small))


class TextureTests(unittest.TestCase):
    def layers(self, rate):
        return [p for p in mc.generate(CFG, [{"t0": 0.0, "t1": 40.0}], stems(rate), REC, IMAGES) if p["lyric_dim"] >= 1.0]

    def test_texture_layers_are_zoomed_past_the_frame_and_still_overlap_it(self):
        tx = CFG["texture"]
        ls = self.layers(1)
        self.assertTrue(ls)
        for p in ls:
            self.assertGreaterEqual(p["h"], tx["zoom_min"] - 1e-3)
            self.assertGreaterEqual(min(p["x"] + p["w"], 1) - max(p["x"], 0), tx["visible"] - 1e-3)
            self.assertGreaterEqual(min(p["y"] + p["h"], 1) - max(p["y"], 0), tx["visible"] - 1e-3)

    def test_busy_music_stacks_more_layers_at_once(self):
        def peak(ls):
            return max(sum(1 for q in ls if q["t0"] <= p["t0"] < q["t1"]) for p in ls)
        self.assertGreater(peak(self.layers(40)), peak(self.layers(1)))

    def test_the_same_image_shows_different_regions(self):
        by = {}
        for p in self.layers(20):
            by.setdefault(p["image"], set()).add((p["x"], p["y"], p["h"]))
        self.assertTrue(any(len(v) > 1 for v in by.values()))


class MotifBudgetTests(unittest.TestCase):
    def test_a_motif_is_limited_in_count_and_spaced_in_time(self):
        imgs = [("a.png", (200, 200)), ("b.png", (200, 200))]
        cuts = {"a.png": ("cut_a.png", (100, 100)), "b.png": ("cut_b.png", (100, 100))}
        cfg = dict(CFG, motif={"max": 2, "gap_s": 10.0}, pieces={"sparse": 6, "busy": 6})
        out = mc.generate(cfg, [{"t0": 0.0, "t1": 120.0}], stems(1, 120), dict(REC, duration_s=120.0), imgs, cuts)
        for name in ("cut_a.png", "cut_b.png"):
            ts = sorted(p["t0"] for p in out if p["image"] == name)
            self.assertLessEqual(len(ts), 2)
            self.assertTrue(all(b - a >= 10.0 for a, b in zip(ts, ts[1:])))
        self.assertTrue([p for p in out if p["image"].startswith("cut_")])


class OverlayTests(unittest.TestCase):
    def test_an_overlay_changes_only_what_it_names_and_refuses_unknown_keys(self):
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            ok = Path(d) / "o.json"
            ok.write_text(json.dumps({"quiet": [[1.0, 2.0]], "texture": {"layers_busy": 9}}))
            cfg = mc.load_config(root / "vocab/collage.json", ok)
            self.assertEqual(cfg["quiet"], [[1.0, 2.0]])
            self.assertEqual(cfg["texture"]["layers_busy"], 9)
            self.assertEqual(cfg["texture"]["zoom_min"], CFG["texture"]["zoom_min"])
            bad = Path(d) / "b.json"
            bad.write_text(json.dumps({"texture": {"surprise": 1}}))
            with self.assertRaises(SystemExit):
                mc.load_config(root / "vocab/collage.json", bad)
