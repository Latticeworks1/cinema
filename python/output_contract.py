"""Timing shared with track::output; no audio/scientific dependencies."""
import json
import math
import struct
from pathlib import Path

CONTRACT = json.loads((Path(__file__).resolve().parents[1] / "config/output.json").read_text())
_SUPPORTED = dict(version=1, fps=30, logical_width=1920, logical_height=1080,
                  output_width=1920, output_height=1080, flat_scale=2)
for key, value in _SUPPORTED.items():
    if CONTRACT.get(key) != value:
        raise ValueError(f"unsupported output contract: {key}")
FPS = CONTRACT["fps"]


def _f32(value):
    return struct.unpack("f", struct.pack("f", value))[0]


def frame_count(duration):
    if not math.isfinite(duration) or duration <= 0:
        raise ValueError("duration must be finite and positive")
    return math.floor(_f32(_f32(duration) * FPS) + 0.5)


def sample_bounds(frame, sample_rate):
    """Integer boundaries prevent accumulated drift when sample rate / fps is fractional."""
    return frame * sample_rate // FPS, (frame + 1) * sample_rate // FPS
