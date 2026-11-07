"""The camera is moved by events that decay, never by a free-running periodic signal."""
import json
import unittest
from pathlib import Path

root = Path(__file__).resolve().parents[1]
CAMERA = {"roll", "tilt_x", "tilt_y", "aim_x", "aim_y", "zoom"}
PERIODIC = ("phase", "bar_phase")


class CameraMotionTests(unittest.TestCase):
    def routes(self):
        return json.loads((root / "vocab/matrix.json").read_text())["routes"]

    def test_no_camera_route_reads_a_phase_or_uses_a_sine_curve(self):
        for r in self.routes():
            if r["to"] in CAMERA:
                self.assertNotEqual(r.get("curve"), "sine", r)
                self.assertFalse(r["from"].endswith(PERIODIC), r)

    def test_the_check_can_fail(self):
        bad = {"from": "master.bar_phase", "to": "tilt_y", "curve": "sine"}
        self.assertTrue(bad["from"].endswith(PERIODIC) and bad["to"] in CAMERA)

    def test_camera_tilt_has_event_sources(self):
        sources = {r["from"] for r in self.routes() if r["to"] in {"roll", "tilt_x", "tilt_y"}}
        self.assertTrue(any(s.endswith(".hit") or s.endswith(".leap") for s in sources))


if __name__ == "__main__":
    unittest.main()
