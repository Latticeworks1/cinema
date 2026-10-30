#!/usr/bin/env python3
"""Audit a NeuralNote transcription against audio and export a reviewable MIDI.

The spectral checks are triage signals, not an accuracy score. MuScriptor does
not predict velocity, and a mixed recording cannot identify independent stem
gains, so CC7 levels are explicit playback choices.
"""

from __future__ import annotations

import argparse
from collections import defaultdict
import hashlib
import json
import math
from pathlib import Path
import subprocess

import mido
import numpy as np
from scipy.signal import fftconvolve, find_peaks, stft

from transcribe import transcribe

SAMPLE_RATE = 16000
HOP = 256
WINDOW = 4096


def decode_audio(path: Path) -> np.ndarray:
    result = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-vn", "-ac", "1",
         "-ar", str(SAMPLE_RATE), "-f", "f32le", "-"],
        capture_output=True, check=True,
    )
    audio = np.frombuffer(result.stdout, dtype="<f4")
    if not len(audio):
        raise ValueError("ffmpeg decoded no audio samples")
    return audio


def validate_notes(data: dict, audio_seconds: float) -> list[dict]:
    notes = data.get("notes")
    if not isinstance(notes, list) or not notes:
        raise ValueError("notes.json contains no notes")
    for i, note in enumerate(notes):
        start, end = float(note["onset"]), float(note["offset"])
        pitch = int(note["pitch"])
        if not (math.isfinite(start) and math.isfinite(end) and
                0 <= start < end <= audio_seconds + .05 and 0 <= pitch <= 127):
            raise ValueError(f"invalid timing or pitch in note {i}")
    return notes


def estimate_grid(notes: list[dict], bpm_min: float, bpm_max: float) -> dict:
    """Fit melodic onsets to an eighth-note grid, then refine by regression."""
    onsets = np.array(sorted({round(float(n["onset"]), 2) for n in notes
                              if not n.get("is_drum")}), dtype=float)
    if len(onsets) < 5:
        raise ValueError("need at least five distinct melodic onsets; pass --bpm")
    best = None
    for bpm in np.arange(bpm_min, bpm_max + .05, .1):
        step = 30.0 / bpm
        phase = (np.angle(np.exp(2j * np.pi * onsets / step).sum())
                 * step / (2 * np.pi)) % step
        residual = (onsets - phase + step / 2) % step - step / 2
        candidate = (float(np.mean(abs(residual))), step, phase)
        if best is None or candidate[0] < best[0]:
            best = candidate
    _, step, phase = best
    indices = np.round((onsets - phase) / step)
    slope, intercept = np.polyfit(indices, onsets, 1)
    residual_ms = abs(onsets - (slope * indices + intercept)) * 1000
    return {
        "bpm": round(float(30 / slope), 3),
        "phase_seconds": round(float(intercept), 5),
        "median_grid_error_ms": round(float(np.median(residual_ms)), 1),
        "max_grid_error_ms": round(float(np.max(residual_ms)), 1),
        "onsets_used": len(onsets),
    }


def find_audio_repeat(audio: np.ndarray) -> dict | None:
    """Find a strong repeated waveform span without writing an audio copy."""
    downsampled = audio[::2].astype(np.float64)
    rate = SAMPLE_RATE // 2
    count = len(downsampled)
    first = int(3 * rate)
    last = min(int(10 * rate), int(count * .7))
    if last <= first:
        return None
    correlation = fftconvolve(downsampled, downsampled[::-1], mode="full")[count - 1:]
    energy = np.r_[0, np.cumsum(downsampled ** 2)]
    lags = np.arange(first, last)
    norm = np.sqrt(energy[count - lags] * (energy[count] - energy[lags]))
    similarity = correlation[lags] / np.maximum(norm, 1e-12)
    winner = int(np.argmax(similarity))
    return {"period_seconds": round(float(lags[winner] / rate), 6),
            "waveform_correlation": round(float(similarity[winner]), 3)}


def spectral_audit(audio: np.ndarray, notes: list[dict]) -> dict:
    freqs, times, spectrum = stft(
        audio, fs=SAMPLE_RATE, nperseg=WINDOW,
        noverlap=WINDOW - HOP, boundary=None,
    )
    magnitude = abs(spectrum)
    active = (freqs >= 80) & (freqs <= 5000)
    flux = np.maximum(0, np.diff(np.log1p(magnitude[active] * 1000), axis=1)).sum(axis=0)
    flux = np.r_[flux[0], flux]
    peaks, _ = find_peaks(flux, distance=5, prominence=np.percentile(flux, 55))
    peak_times = times[peaks]
    onsets = sorted({round(float(n["onset"]), 2) for n in notes})
    checked = [t for t in onsets if t >= .1]  # STFT cannot test the clip's opening edge.
    close = [float(np.min(abs(peak_times - t))) if len(peaks) else math.inf
             for t in checked]

    def harmonic_score(note: dict, pitch: int) -> float:
        start, end = float(note["onset"]), float(note["offset"])
        lo = start + min(.08, (end - start) * .25)
        hi = min(end - .02, start + .5)
        frames = np.flatnonzero((times >= lo) & (times <= hi))
        if not len(frames):
            frames = np.array([np.argmin(abs(times - (start + end) / 2))])
        spec = np.median(magnitude[:, frames], axis=1)
        base = 440 * 2 ** ((pitch - 69) / 12)
        score = 0.0
        for harmonic in range(1, 7):
            hz = base * harmonic
            if hz > 5000:
                break
            bin_index = int(np.argmin(abs(freqs - hz)))
            score += float(np.max(spec[max(0, bin_index - 1):bin_index + 2])) / harmonic ** .7
        return score

    support = defaultdict(list)
    flagged = []
    for note in notes:
        if note.get("is_drum"):
            continue
        pitch = int(note["pitch"])
        reference = harmonic_score(note, pitch)
        nearby = max(harmonic_score(note, pitch + delta)
                     for delta in (-2, -1, 1, 2))
        ratio = reference / (nearby + 1e-12)
        name = note.get("instrument", f"program_{note['program']}")
        support[name].append(ratio)
        if ratio < .7:
            flagged.append({"instrument": name, "pitch": pitch,
                            "onset": note["onset"], "support_ratio": round(ratio, 2)})

    bands = {}
    for name, low, high in (("sub_bass", 25, 120), ("low_mid", 120, 500),
                            ("mid", 500, 2000), ("high", 2000, 7000)):
        chosen = (freqs >= low) & (freqs < high)
        power = float(np.mean(np.sum(magnitude[chosen] ** 2, axis=0)))
        bands[name] = round(10 * math.log10(power + 1e-20), 1)
    return {
        "spectral_flux_peaks": len(peaks),
        "spectral_flux_peak_times_seconds": [round(float(value), 3) for value in peak_times],
        "onsets_after_opening": len(checked),
        "onsets_near_flux_peak_100ms": sum(delta <= .1 for delta in close),
        "pitch_support": {name: {"notes": len(values),
                                 "preferred_over_nearby": sum(v >= 1 for v in values),
                                 "median_ratio": round(float(np.median(values)), 2)}
                          for name, values in support.items()},
        "weak_pitch_support": flagged,
        "spectral_band_power_db": bands,
    }


def write_midi(path: Path, notes: list[dict], seconds: float,
               bpm: float, shift: float, gains: dict[str, int],
               quantization: tuple[float, float] | None = None) -> None:
    tempo = round(60_000_000 / bpm)
    ticks_per_beat = 960

    def ticks(time: float) -> int:
        return round(mido.second2tick(max(0, time), ticks_per_beat, tempo))

    def aligned(time: float) -> float:
        if quantization is not None:
            step, phase = quantization
            return max(0, round((time - phase) / step) * step + phase)
        return max(0, time - shift)

    midi = mido.MidiFile(type=1, ticks_per_beat=ticks_per_beat)
    conductor = mido.MidiTrack([
        mido.MetaMessage("track_name", name="Tempo and timing", time=0),
        mido.MetaMessage("set_tempo", tempo=tempo, time=0),
        mido.MetaMessage("end_of_track", time=ticks(seconds)),
    ])
    midi.tracks.append(conductor)
    groups = defaultdict(list)
    for note in notes:
        groups[(int(note["program"]), note.get("instrument", "unknown"))].append(note)
    channels = [channel for channel in range(16) if channel != 9]
    for i, ((program, name), group) in enumerate(sorted(groups.items())):
        channel = 9 if program == 128 else channels[i % len(channels)]
        track = mido.MidiTrack([
            mido.MetaMessage("track_name", name=name, time=0),
            mido.Message("control_change", channel=channel, control=7,
                         value=gains.get(name, 100), time=0),
        ])
        if program != 128:
            track.append(mido.Message("program_change", channel=channel,
                                      program=program, time=0))
        events = []
        for note in group:
            start = ticks(aligned(float(note["onset"])))
            end = max(start + 1, ticks(aligned(float(note["offset"]))))
            pitch = int(note["pitch"])
            events.extend(((start, 1, pitch), (end, 0, pitch)))
        last = 0
        for tick, on, pitch in sorted(events):
            track.append(mido.Message("note_on" if on else "note_off",
                                      channel=channel, note=pitch,
                                      velocity=80 if on else 0, time=tick - last))
            last = tick
        track.append(mido.MetaMessage("end_of_track", time=max(0, ticks(seconds) - last)))
        midi.tracks.append(track)
    midi.save(path)


def parse_gain(text: str) -> tuple[str, int]:
    name, sep, value = text.partition("=")
    if not sep or not name or not value.isdigit() or not 0 <= int(value) <= 127:
        raise argparse.ArgumentTypeError("gain must be INSTRUMENT=0..127")
    return name, int(value)


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("audio", type=Path)
    parser.add_argument("--notes", type=Path, help="existing notes.json; defaults to OUTPUT/notes.json")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--transcribe", action="store_true", help="run local MuScriptor if notes are absent")
    parser.add_argument("--bpm", type=float, help="override the estimated BPM")
    parser.add_argument("--bpm-range", nargs=2, type=float, default=(60, 200), metavar=("MIN", "MAX"))
    parser.add_argument("--shift-ms", type=float, help="override automatic onset correction")
    parser.add_argument("--gain", action="append", type=parse_gain, default=[], metavar="INSTRUMENT=CC7")
    args = parser.parse_args()

    if not 0 < args.bpm_range[0] < args.bpm_range[1]:
        parser.error("--bpm-range must contain positive MIN below MAX")

    audio = args.audio.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    notes_path = args.notes.resolve() if args.notes else output / "notes.json"
    if not notes_path.exists():
        if not args.transcribe:
            parser.error(f"{notes_path} is absent; pass --transcribe to run the model")
        transcribe(audio, notes_path, export_midi=False)
    samples = decode_audio(audio)
    seconds = len(samples) / SAMPLE_RATE
    data = json.loads(notes_path.read_text())
    notes = validate_notes(data, seconds)
    instrument_names = {n.get("instrument", "unknown") for n in notes}
    unknown_gains = set(dict(args.gain)) - instrument_names
    if unknown_gains:
        parser.error(f"gain names absent from the transcription: {', '.join(sorted(unknown_gains))}")
    if abs(float(data.get("seconds", seconds)) - seconds) > .05:
        raise ValueError("notes.json duration differs from decoded audio by over 50 ms")
    grid = estimate_grid(notes, *args.bpm_range) if args.bpm is None else None
    repeat = find_audio_repeat(samples)
    bpm = args.bpm if args.bpm is not None else grid["bpm"]
    tempo_source = "override" if args.bpm is not None else "midi_onset_grid"
    if grid and repeat and repeat["waveform_correlation"] >= .75:
        choices = [60 * 4 * bars / repeat["period_seconds"] for bars in (1, 2, 4, 8)]
        closest = min(choices, key=lambda candidate: abs(candidate - grid["bpm"]))
        if abs(closest - grid["bpm"]) / grid["bpm"] <= .03:
            bpm = round(closest, 3)
            tempo_source = "audio_waveform_repeat"
    if not math.isfinite(bpm) or bpm <= 0:
        raise ValueError("BPM must be positive and finite")
    spectral = spectral_audit(samples, notes)
    quantization = None
    if args.shift_ms is not None:
        shift = args.shift_ms / 1000
    elif tempo_source == "audio_waveform_repeat" and grid["max_grid_error_ms"] <= 70:
        step = 30 / bpm
        peaks = np.array(spectral["spectral_flux_peak_times_seconds"])
        melodic_onsets = np.array([float(note["onset"]) for note in notes
                                   if not note.get("is_drum")])
        matched_peaks = np.array([peak for peak in peaks
                                  if np.min(abs(melodic_onsets - peak)) <= .1])
        if len(matched_peaks) >= 4:
            phase = (np.angle(np.exp(2j * np.pi * matched_peaks / step).sum())
                     * step / (2 * np.pi)) % step
        else:
            phase = grid["phase_seconds"] % step
        quantization = (step, float(phase))
        shift = 0.0
    elif grid and grid["max_grid_error_ms"] <= 30 and 0 <= grid["phase_seconds"] <= .06:
        shift = grid["phase_seconds"]
    else:
        shift = 0.0
    gains = dict(args.gain)
    midi_path = output / "refined.mid"
    write_midi(midi_path, notes, seconds, bpm, shift, gains, quantization)
    midi = mido.MidiFile(midi_path)
    note_count = sum(message.type == "note_on" and message.velocity > 0
                     for track in midi.tracks for message in track)
    if note_count != len(notes) or abs(midi.length - seconds) > .005:
        raise RuntimeError("MIDI note count or duration failed validation")
    report = {
        "source_audio": str(audio),
        "source_sha256": file_sha256(audio),
        "notes": str(notes_path),
        "audio_seconds": round(seconds, 6),
        "midi_seconds": round(midi.length, 6),
        "note_count": note_count,
        "tempo_bpm": bpm,
        "tempo_source": tempo_source,
        "tempo_grid": grid,
        "audio_repeat": repeat,
        "onset_shift_ms": round(shift * 1000, 2),
        "quantization": ({"step_seconds": round(quantization[0], 6),
                          "phase_seconds": round(quantization[1], 6)}
                         if quantization else None),
        "track_cc7": {name: gains.get(name, 100) for _, name in sorted(
            {(int(n["program"]), n.get("instrument", "unknown")) for n in notes})},
        "velocity": "80 for all notes; MuScriptor does not predict velocity",
        "spectral_audit": spectral,
        "interpretation": "Mixed-audio spectra flag candidates for review; they do not prove instrument identity or independent stem gain.",
    }
    (output / "audit.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"MIDI: {midi_path}")
    print(f"Audit: {output / 'audit.json'}")
    print(f"{note_count} notes, {bpm:.2f} BPM, {seconds:.3f} s; "
          f"{spectral['onsets_near_flux_peak_100ms']}/{spectral['onsets_after_opening']} onsets near spectral changes")


if __name__ == "__main__":
    main()
