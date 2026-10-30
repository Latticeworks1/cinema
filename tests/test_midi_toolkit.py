"""Checks for timing and MIDI export in the transcription review tool."""

import sys
import tempfile
import unittest
from pathlib import Path

import mido
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from midi_toolkit import estimate_grid, find_audio_repeat, write_midi  # noqa: E402


class MidiToolkitTests(unittest.TestCase):
    def test_audio_repeat_finds_exact_loop_period(self):
        rate = 16000
        rng = np.random.default_rng(7)
        phrase = rng.standard_normal(int(3.2 * rate)).astype(np.float32)
        audio = np.tile(phrase, 3)
        repeat = find_audio_repeat(audio)
        self.assertAlmostEqual(repeat["period_seconds"], 3.2, delta=.001)
        self.assertGreater(repeat["waveform_correlation"], .99)

    def test_estimates_eighth_note_grid_and_model_offset(self):
        # Syncopated chord changes and short lead notes on a 150 BPM grid.
        steps = [0, 7, 16, 23, 32, 39, 40, 48, 55]
        notes = [{"onset": .02 + step * .2, "is_drum": False}
                 for step in steps]
        fit = estimate_grid(notes, 100, 180)
        self.assertAlmostEqual(fit["bpm"], 150, delta=.2)
        self.assertAlmostEqual(fit["phase_seconds"], .02, delta=.005)
        self.assertLess(fit["max_grid_error_ms"], 1)

    def test_midi_retains_notes_gains_and_audio_duration(self):
        notes = [
            {"onset": .02, "offset": 1.0, "pitch": 45, "program": 33,
             "instrument": "electric_bass", "is_drum": False},
            {"onset": 1.02, "offset": 2.4, "pitch": 48, "program": 33,
             "instrument": "electric_bass", "is_drum": False},
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.mid"
            write_midi(path, notes, 2.5, 150, .02, {"electric_bass": 105})
            midi = mido.MidiFile(path)
            self.assertEqual(sum(message.type == "note_on" and message.velocity > 0
                                 for track in midi.tracks for message in track), 2)
            self.assertAlmostEqual(midi.length, 2.5, delta=.002)
            self.assertTrue(any(message.type == "control_change" and
                                message.control == 7 and message.value == 105
                                for track in midi.tracks for message in track))

    def test_quantization_moves_model_onsets_to_audio_grid(self):
        notes = [{"onset": 1.45, "offset": 1.65, "pitch": 60,
                  "program": 80, "instrument": "synth_lead", "is_drum": False}]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "quantized.mid"
            write_midi(path, notes, 2, 150, 0, {}, quantization=(.2, .01))
            midi = mido.MidiFile(path)
            elapsed = 0
            onsets = []
            for message in midi.tracks[1]:
                elapsed += message.time
                if message.type == "note_on" and message.velocity:
                    onsets.append(mido.tick2second(elapsed, midi.ticks_per_beat, 400000))
            self.assertAlmostEqual(onsets[0], 1.41, delta=.002)


if __name__ == "__main__":
    unittest.main()
