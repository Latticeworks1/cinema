#!/usr/bin/env python3
"""Export strict analysis.json from NeuralNote transcription and audio recording.

Validates that audio and transcription match via cryptographic SHA-256 hashes
and duration checks, preventing silent desynchronization between mismatched
stems and transcriptions.

Usage:
    python3.10 music/export_analysis.py [--audio data/audio.wav] [--notes data/notes.json] [--out out/analysis.json]
"""

import argparse
import hashlib
import json
import math
import sys
import wave
from pathlib import Path

import numpy as np

FPS = 30
WAVEFORM_COLS_PER_S = 160


def compute_sha256(filepath: Path) -> str:
    """Compute the SHA-256 hex digest of a file."""
    h = hashlib.sha256()
    with open(filepath, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def measure_peak_hz(sig: np.ndarray, sr: int, lo: float, hi: float, pad: int = 1 << 16) -> float:
    """Measure the dominant physical frequency within a frequency window using Hann-windowed FFT."""
    if len(sig) == 0:
        return 0.0
    win = np.hanning(len(sig))
    spec = np.abs(np.fft.rfft(sig * win, pad))
    freqs = np.fft.rfftfreq(pad, 1.0 / sr)
    band_mask = (freqs >= lo) & (freqs <= hi)
    if not np.any(band_mask):
        return 0.0
    peak_idx = np.argmax(spec * band_mask)
    return float(freqs[peak_idx])


def derive_tempo_from_kicks(kicks: list[float]) -> dict:
    """Derive BPM and timing statistics from kick drum inter-onset intervals."""
    if len(kicks) < 4:
        return {
            "bpm": 120.0,
            "derivation": "fallback_default",
            "median_ioi_s": 0.5,
            "sample_count": 0,
        }

    diffs = np.diff(kicks)
    # Filter reasonable musical intervals between 120ms and 900ms
    valid_diffs = diffs[(diffs >= 0.12) & (diffs <= 0.90)]
    if len(valid_diffs) == 0:
        return {
            "bpm": 120.0,
            "derivation": "fallback_insufficient_intervals",
            "median_ioi_s": 0.5,
            "sample_count": 0,
        }

    median_ioi = float(np.median(valid_diffs))
    bpm = round(60.0 / median_ioi, 1)

    return {
        "bpm": bpm,
        "derivation": "median_kick_inter_onset_interval",
        "median_ioi_s": round(median_ioi, 4),
        "sample_count": int(len(valid_diffs)),
    }


PC_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
CHORD_SHAPES = [
    ("", (0, 4, 7)),
    ("m", (0, 3, 7)),
    ("m7", (0, 3, 7, 10)),
    ("maj7", (0, 4, 7, 11)),
    ("7", (0, 4, 7, 10)),
]


def detect_chords(piano: list, bass: list, bpm: float, duration_s: float) -> list[dict]:
    """Name one chord per bar from duration-weighted pitch classes, bass counted double.

    Bars start at the first bass onset and last four beats at the measured tempo.
    Consecutive bars with the same name are merged.
    """
    if not bass:
        return []
    bar = 4.0 * 60.0 / bpm
    t0 = float(bass[0]["onset"])
    out: list[dict] = []
    k = 0
    while t0 + k * bar < duration_s:
        a, b = t0 + k * bar, t0 + (k + 1) * bar
        k += 1
        hist = [0.0] * 12
        bass_hist = [0.0] * 12
        for notes, w, target in ((piano, 1.0, hist), (bass, 2.0, hist)):
            for n in notes:
                ov = min(float(n["offset"]), b) - max(float(n["onset"]), a)
                if ov > 0:
                    target[int(n["pitch"]) % 12] += w * ov
                    if notes is bass:
                        bass_hist[int(n["pitch"]) % 12] += ov
        if sum(hist) <= 0:
            continue
        best = None
        for root in range(12):
            for suffix, shape in CHORD_SHAPES:
                pcs = {(root + i) % 12 for i in shape}
                inside = sum(hist[p] for p in pcs)
                outside = sum(hist) - inside
                score = inside - 0.6 * outside + 0.5 * bass_hist[root] + 0.01 * len(shape) * inside
                if best is None or score > best[0]:
                    best = (score, PC_NAMES[root] + suffix)
        if out and out[-1]["name"] == best[1]:
            continue
        out.append({"t": round(a, 3), "name": best[1]})
    return out


def analyze(audio_path: Path, notes_path: Path, out_path: Path):
    if not audio_path.exists():
        print(f"[!] Error: Audio file not found at {audio_path}", file=sys.stderr)
        sys.exit(1)
    if not notes_path.exists():
        print(f"[!] Error: Transcription file not found at {notes_path}", file=sys.stderr)
        sys.exit(1)

    print(f"[*] Reading audio from: {audio_path}")
    print(f"[*] Reading notes from: {notes_path}")

    # 1. Cryptographic hashes
    audio_sha256 = compute_sha256(audio_path)
    notes_sha256 = compute_sha256(notes_path)

    # 2. Ingest Audio
    with wave.open(str(audio_path), "rb") as w:
        sr = w.getframerate()
        n_samples = w.getnframes()
        n_channels = w.getnchannels()
        sampwidth = w.getsampwidth()
        audio_duration_s = n_samples / sr
        raw_bytes = w.readframes(n_samples)

    if sampwidth == 2:
        audio_samples = np.frombuffer(raw_bytes, dtype="<i2").reshape(-1, n_channels) / 32768.0
    elif sampwidth == 4:
        audio_samples = np.frombuffer(raw_bytes, dtype="<i4").reshape(-1, n_channels) / 2147483648.0
    else:
        print(f"[!] Unsupported audio sample width: {sampwidth} bytes", file=sys.stderr)
        sys.exit(1)

    mono = audio_samples.mean(axis=1)
    total_frames = int(round(audio_duration_s * FPS))

    # 3. Ingest Transcription
    notes_raw = json.loads(notes_path.read_text(encoding="utf-8"))
    notes_duration_s = float(notes_raw.get("seconds", audio_duration_s))

    # Validation: Mismatch check
    if abs(audio_duration_s - notes_duration_s) > 0.5:
        print(
            f"[!] ERROR: Audio duration ({audio_duration_s:.2f}s) and notes duration ({notes_duration_s:.2f}s) mismatch by > 0.5s!",
            file=sys.stderr,
        )
        print("[!] Aborting to prevent silent desynchronization.", file=sys.stderr)
        sys.exit(1)

    raw_notes_list = notes_raw.get("notes", [])
    raw_notes_list.sort(key=lambda n: n["onset"])

    # 4. Waveform Peak Envelope (for UI timeline)
    hop_wave = max(1, sr // WAVEFORM_COLS_PER_S)
    n_cols = len(mono) // hop_wave
    wave_peaks = [
        float(np.max(np.abs(mono[i * hop_wave : (i + 1) * hop_wave])))
        for i in range(n_cols)
    ]
    max_peak = max(wave_peaks) if wave_peaks else 1.0
    waveform_norm = [round(p / max_peak, 3) for p in wave_peaks]

    # 5. RMS Curve per Video Frame (FPS = 30)
    hop_rms = max(1, sr // FPS)
    rms_curve_db = []
    for i in range(total_frames):
        chunk = mono[i * hop_rms : (i + 1) * hop_rms]
        rms_val = np.sqrt(np.mean(chunk**2)) if len(chunk) > 0 else 1e-5
        db = 20.0 * np.log10(max(float(rms_val), 1e-5))
        rms_curve_db.append(round(float(db), 1))

    loudest_idx = int(np.argmax(rms_curve_db)) if rms_curve_db else 0
    loudest_info = {
        "frame": loudest_idx,
        "t": round(loudest_idx / FPS, 3),
        "db": rms_curve_db[loudest_idx] if rms_curve_db else 0.0,
    }

    # 7. Instrument Note Categorization
    drums_notes = [n for n in raw_notes_list if n.get("instrument") == "drums" or n.get("is_drum")]
    bass_notes = [n for n in raw_notes_list if n.get("instrument") == "electric_bass"]
    piano_notes = [n for n in raw_notes_list if n.get("instrument") == "electric_piano"]

    # Extract drum hits
    kicks_raw = [n["onset"] for n in drums_notes if n["pitch"] in (35, 36)]
    claps_raw = [n["onset"] for n in drums_notes if n["pitch"] in (37, 38, 40)]
    hats_raw = [n["onset"] for n in drums_notes if n["pitch"] in (42, 44, 46)]

    def dedup(arr: list[float], eps: float = 0.015) -> list[float]:
        out = []
        for v in arr:
            if not out or abs(v - out[-1]) > eps:
                out.append(round(float(v), 4))
        return out

    kicks = dedup(kicks_raw)
    claps = dedup(claps_raw)
    hats = dedup(hats_raw)

    # 8. Tempo Estimation
    tempo_data = derive_tempo_from_kicks(kicks)

    # 9. Physical frequency measurements at detected events: the first kick and the first bass note.
    if not kicks or not bass_notes:
        print("[!] Error: transcription needs at least one kick and one bass note", file=sys.stderr)
        sys.exit(1)
    first_kick_t = kicks[0]
    kick_hz = round(measure_peak_hz(mono[int((first_kick_t + 0.0) * sr) : int((first_kick_t + 0.31) * sr)], sr, 35.0, 120.0), 1)
    b0 = bass_notes[0]
    b_end = min(float(b0["offset"]), float(b0["onset"]) + 1.3)
    bass_hz = round(measure_peak_hz(mono[int(float(b0["onset"]) * sr) : int(b_end * sr)], sr, 40.0, 160.0), 1)

    # Structure: the drop is the first bass note; the impact is the beat after the last kick.
    median_ioi = tempo_data["median_ioi_s"]
    structure = {
        "drop_t": round(float(bass_notes[0]["onset"]), 3),
        "bar_s": round(4.0 * 60.0 / tempo_data["bpm"], 4),
        "impact_t": round(kicks[-1] + median_ioi, 3),
    }

    chords = detect_chords(piano_notes, bass_notes, tempo_data["bpm"], audio_duration_s)

    def clean_note_record(n: dict, default_inst: str) -> dict:
        return {
            "onset": round(float(n["onset"]), 4),
            "offset": round(float(n["offset"]), 4),
            "duration": round(float(n["offset"] - n["onset"]), 4),
            "pitch": int(n["pitch"]),
            "instrument": n.get("instrument", default_inst),
        }

    # Assemble complete schema
    analysis_doc = {
        "schema_version": 1,
        "source": {
            "audio_file": str(audio_path),
            "audio_sha256": audio_sha256,
            "audio_duration_s": round(audio_duration_s, 4),
            "audio_sample_rate": sr,
            "audio_channels": n_channels,
            "notes_file": str(notes_path),
            "notes_sha256": notes_sha256,
            "notes_duration_s": round(notes_duration_s, 4),
            "total_frames_at_30fps": total_frames,
        },
        "tempo": tempo_data,
        "measurements": {
            "kick_hz": kick_hz,
            "bass_hz": bass_hz,
            "first_kick_t": first_kick_t,
            "loudest": loudest_info,
        },
        "waveform": {
            "columns_per_second": WAVEFORM_COLS_PER_S,
            "peaks": waveform_norm,
        },
        "rms_curve_db": rms_curve_db,
        "chords": chords,
        "structure": structure,
        "events": {
            "kicks": kicks,
            "claps": claps,
            "hats": hats,
        },
        "instruments": {
            "electric_bass": [clean_note_record(n, "electric_bass") for n in bass_notes],
            "electric_piano": [clean_note_record(n, "electric_piano") for n in piano_notes],
            "drums": [clean_note_record(n, "drums") for n in drums_notes],
        },
    }

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(analysis_doc, indent=2), encoding="utf-8")

    print(f"[*] Successfully wrote strict analysis to {out_path}")
    print(f"    - Audio SHA256: {audio_sha256[:16]}... ({audio_duration_s:.2f}s)")
    print(f"    - Notes SHA256: {notes_sha256[:16]}... ({len(raw_notes_list)} notes)")
    print(f"    - Measured BPM: {tempo_data['bpm']} ({tempo_data['derivation']})")
    print(f"    - Measured Kick: {kick_hz} Hz, Bass: {bass_hz} Hz")
    print(f"    - Loudest point: {loudest_info['db']} dB at t={loudest_info['t']}s")


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description="Export strict analysis.json with SHA-256 hash checks.")
    parser.add_argument(
        "--audio",
        type=Path,
        default=root / "data" / "audio.wav",
        help="Path to source audio WAV file (default: data/audio.wav)",
    )
    parser.add_argument(
        "--notes",
        type=Path,
        default=root / "data" / "notes.json",
        help="Path to NeuralNote transcription JSON (default: data/notes.json)",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=root / "out" / "analysis.json",
        help="Path to output analysis.json (default: out/analysis.json)",
    )
    args = parser.parse_args()

    # Fallback to out/pass/music.wav if data/audio.wav does not exist
    audio_path = args.audio
    if not audio_path.exists() and (root / "out" / "pass" / "music.wav").exists():
        audio_path = root / "out" / "pass" / "music.wav"

    notes_path = args.notes
    if not notes_path.exists() and (root / "out" / "neuralnote" / "Db2OLvdhsDk-notes.json").exists():
        notes_path = root / "out" / "neuralnote" / "Db2OLvdhsDk-notes.json"

    analyze(audio_path, notes_path, args.out)


if __name__ == "__main__":
    main()
