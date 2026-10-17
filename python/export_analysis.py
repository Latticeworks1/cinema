#!/usr/bin/env python3
"""Export strict analysis.json from NeuralNote transcription and audio recording.

Validates that audio and transcription match via cryptographic SHA-256 hashes
and duration checks, preventing silent desynchronization between mismatched
stems and transcriptions.

Every choice that depends on the material (instrument names, drum pitch groups, which stem
sets the tempo and the drop, chord shapes, measurement bands) is read from a configuration
file, `vocab/export.json` by default; a project packet overrides it per run.

Usage:
    python3 export_analysis.py --notes notes.json --out analysis.json [--audio audio.wav] [--config export.json]
"""

import argparse
import hashlib
import json
import math
import sys
import wave
from pathlib import Path

import numpy as np

FPS = 30  # fixed by the renderer (fframes FPS is a compile-time constant)
SCHEMA_VERSION = 2
DEFAULT_CONFIG = Path(__file__).resolve().parent.parent / "vocab" / "export.json"
PC_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]


def fail(msg: str):
    print(f"[!] Error: {msg}", file=sys.stderr)
    sys.exit(1)


def _keys(where: str, d: dict, required: set):
    if not isinstance(d, dict):
        fail(f"{where} must be an object")
    missing, unknown = required - d.keys(), d.keys() - required
    if missing or unknown:
        fail(f"{where}: missing {sorted(missing)}, unknown {sorted(unknown)}")


def load_config(path: Path) -> dict:
    """Read and validate the export configuration; unknown keys and out-of-range values are refused."""
    cfg = json.loads(path.read_text(encoding="utf-8"))
    _keys("config", cfg, {"waveform_cols_per_s", "level_floor", "duration_tolerance_s", "stems", "tempo", "structure", "chords", "peaks"})
    if not (isinstance(cfg["waveform_cols_per_s"], int) and cfg["waveform_cols_per_s"] > 0):
        fail("waveform_cols_per_s must be a positive integer")
    if not 0 < cfg["level_floor"] < 1:
        fail("level_floor must be in (0, 1)")
    if not cfg["duration_tolerance_s"] > 0:
        fail("duration_tolerance_s must be positive")

    st = cfg["stems"]
    _keys("stems", st, {"aliases", "drum_groups", "drum_rest"})
    names = list(st["drum_groups"]) + [st["drum_rest"]]
    if len(set(names)) != len(names):
        fail("stems: drum stem names must be distinct")
    if set(st["aliases"].values()) & set(names):
        fail("stems.aliases renames an instrument to a drum stem name")
    seen: set = set()
    for g, pitches in st["drum_groups"].items():
        if seen & set(pitches):
            fail(f"stems.drum_groups.{g}: a pitch belongs to two groups")
        seen |= set(pitches)

    t = cfg["tempo"]
    _keys("tempo", t, {"bpm_range", "harmonics", "rel_step", "surrogates", "jitter_s", "min_confidence", "seed", "dedup_s", "beats_per_bar", "fallback_bpm"})
    lo, hi = t["bpm_range"]
    if not (0 < lo < hi <= 2 * lo):
        fail("tempo.bpm_range must be [lo, hi] with 0 < lo < hi <= 2 lo, so that one pulse level is named")
    if not (t["harmonics"] >= 1 and 0 < t["rel_step"] < 0.01 and t["surrogates"] >= 20 and t["jitter_s"] > 0 and 0 < t["min_confidence"] < 1
            and t["beats_per_bar"] > 0 and t["dedup_s"] >= 0):
        fail("tempo: need harmonics >= 1, 0 < rel_step < 0.01, surrogates >= 20, jitter_s > 0, 0 < min_confidence < 1, beats_per_bar > 0, dedup_s >= 0")
    if t["fallback_bpm"] is not None and not t["fallback_bpm"] > 0:
        fail("tempo.fallback_bpm must be null or positive")

    _keys("structure", cfg["structure"], {"drop_stem", "impact_stem"})

    c = cfg["chords"]
    _keys("chords", c, {"root_stem", "root_weight", "other_weight", "outside_penalty", "root_bonus", "size_bonus", "shapes"})
    if not c["shapes"]:
        fail("chords.shapes is empty")
    for sh in c["shapes"]:
        _keys("chords.shapes[]", sh, {"suffix", "intervals"})
        if not sh["intervals"] or any(not (isinstance(i, int) and 0 <= i < 12) for i in sh["intervals"]):
            fail("chords.shapes[].intervals must be integers in 0..11")

    for name, p in cfg["peaks"].items():
        _keys(f"peaks.{name}", p, {"stem", "lo_hz", "hi_hz", "window_s", "clip_to_note"})
        if not (0 < p["lo_hz"] < p["hi_hz"] and p["window_s"] > 0):
            fail(f"peaks.{name}: need 0 < lo_hz < hi_hz and window_s > 0")
    return cfg


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


def dedup(arr: list[float], eps: float) -> list[float]:
    out: list[float] = []
    for v in arr:
        if not out or abs(v - out[-1]) > eps:
            out.append(round(float(v), 4))
    return out


def comb_score(t: np.ndarray, period: float, harmonics: int) -> float:
    """Mean squared resultant length of the onset phases at the first `harmonics` multiples of 1/period."""
    s = 0.0
    for h in range(1, harmonics + 1):
        s += abs(np.exp(2j * np.pi * h * t / period).sum()) ** 2
    return s / (harmonics * len(t) ** 2)


def derive_tempo(onsets: list[float], t: dict) -> dict:
    """Estimate the beat period from the onset times of every stem together.

    The period maximising the comb score over one octave of tempi (`bpm_range`) is refined on a
    finer grid, so no inter-onset interval is quantised. Confidence is the fraction of jittered copies of the onsets whose best score
    stays below the observed one.
    """
    on = np.asarray(onsets, dtype=float)
    lo, hi = t["bpm_range"]
    if len(on) >= 8:
        periods = [60.0 / hi]
        while periods[-1] * (1 + t["rel_step"]) <= 60.0 / lo:
            periods.append(periods[-1] * (1 + t["rel_step"]))
        scores = np.array([comb_score(on, p, t["harmonics"]) for p in periods])
        period = float(periods[int(scores.argmax())])
        # refine: maximise the comb score on a grid ten times finer than the search grid, around its peak
        fine = period * (1 + t["rel_step"] / 10 * np.arange(-20, 21))
        period = float(fine[int(np.argmax([comb_score(on, p, t["harmonics"]) for p in fine]))])
        rng = np.random.default_rng(t["seed"])
        below = sum(
            max(comb_score(on + rng.uniform(-t["jitter_s"], t["jitter_s"], len(on)), p, t["harmonics"]) for p in periods) < scores.max()
            for _ in range(t["surrogates"])
        )
        confidence = below / t["surrogates"]
        if confidence >= t["min_confidence"]:
            return {
                "bpm": round(60.0 / period, 2),
                "derivation": "onset_comb_fit",
                "beat_s": round(period, 5),
                "confidence": round(confidence, 3),
                "sample_count": int(len(on)),
            }
    else:
        confidence = 0.0
    if t["fallback_bpm"] is None:
        fail(f"tempo: no pulse in {lo}-{hi} bpm stands out from jittered onsets (confidence {confidence:.2f}, need {t['min_confidence']}) and tempo.fallback_bpm is null")
    return {"bpm": float(t["fallback_bpm"]), "derivation": "fallback_bpm", "beat_s": round(60.0 / t["fallback_bpm"], 5), "confidence": round(confidence, 3), "sample_count": int(len(on))}


def detect_chords(stems: dict, root: str, c: dict, beats_per_bar: int, beat_s: float, duration_s: float) -> list[dict]:
    """Name one chord per bar from duration-weighted pitch classes of the melodic stems.

    The root stem counts at `root_weight`, the others at `other_weight`. Bars start at the first
    onset of the root stem and last `beats_per_bar` beats. Consecutive bars with the same name
    are merged.
    """
    bar = beats_per_bar * beat_s
    t0 = float(stems[root][0]["onset"])
    out: list[dict] = []
    k = 0
    while t0 + k * bar < duration_s:
        a, b = t0 + k * bar, t0 + (k + 1) * bar
        k += 1
        hist = [0.0] * 12
        root_hist = [0.0] * 12
        for name, notes in stems.items():
            w = c["root_weight"] if name == root else c["other_weight"]
            for n in notes:
                ov = min(float(n["offset"]), b) - max(float(n["onset"]), a)
                if ov > 0:
                    hist[int(n["pitch"]) % 12] += w * ov
                    if name == root:
                        root_hist[int(n["pitch"]) % 12] += ov
        if sum(hist) <= 0:
            continue
        best = None
        for r in range(12):
            for shape in c["shapes"]:
                pcs = {(r + i) % 12 for i in shape["intervals"]}
                inside = sum(hist[p] for p in pcs)
                outside = sum(hist) - inside
                score = inside - c["outside_penalty"] * outside + c["root_bonus"] * root_hist[r] + c["size_bonus"] * len(shape["intervals"]) * inside
                if best is None or score > best[0]:
                    best = (score, PC_NAMES[r] + shape["suffix"])
        if out and out[-1]["name"] == best[1]:
            continue
        out.append({"t": round(a, 3), "name": best[1]})
    return out


def split_stems(notes: list[dict], st: dict) -> tuple[dict, dict]:
    """Sort transcribed notes into stems: drum hits by General MIDI pitch group, every other
    instrument into a stem of its own, named by the transcriber's label (or by its program number
    when it gives none) after `aliases`. Returns (melodic, drums)."""
    pitch_group = {p: g for g, ps in st["drum_groups"].items() for p in ps}
    melodic: dict[str, list] = {}
    drums: dict[str, list] = {}
    for n in notes:
        if n.get("is_drum") or n.get("program") == 128:
            drums.setdefault(pitch_group.get(int(n["pitch"]), st["drum_rest"]), []).append(n)
        else:
            label = n.get("instrument") or f"program_{n.get('program', 'unknown')}"
            melodic.setdefault(st["aliases"].get(label, label), []).append(n)
    clash = set(melodic) & set(drums)
    if clash:
        fail(f"an instrument is named like a drum stem: {sorted(clash)}")
    return melodic, drums


def analyze(audio_path: Path | None, notes_path: Path, out_path: Path, cfg: dict):
    """`audio_path` is None for a track made from the transcription alone: the duration then
    comes from the notes and the loudness curve from how many notes sound in each frame."""
    if audio_path is not None and not audio_path.exists():
        fail(f"Audio file not found at {audio_path}")
    if not notes_path.exists():
        fail(f"Transcription file not found at {notes_path}")

    print(f"[*] Reading audio from: {audio_path}" if audio_path else "[*] No audio: loudness is taken from the notes")
    print(f"[*] Reading notes from: {notes_path}")

    # 1. Cryptographic hashes
    audio_sha256 = compute_sha256(audio_path) if audio_path is not None else ""
    notes_sha256 = compute_sha256(notes_path)

    # 2. Ingest Audio (absent when only the transcription is given)
    if audio_path is not None:
        try:
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
                raise ValueError(f"Unsupported audio sample width: {sampwidth} bytes")
        except Exception:
            # Fallback via ffmpeg: handles 24-bit PCM, MP3, AAC, FLAC, AIFF seamlessly
            probe_cmd = ["ffprobe", "-v", "error", "-show_entries", "stream=sample_rate,channels,duration", "-of", "json", str(audio_path)]
            probe_out = subprocess.check_output(probe_cmd)
            meta = json.loads(probe_out)["streams"][0]
            sr = int(meta["sample_rate"])
            n_channels = int(meta["channels"])
            audio_duration_s = float(meta["duration"])
            ffmpeg_cmd = ["ffmpeg", "-v", "error", "-i", str(audio_path), "-f", "f32le", "-acodec", "pcm_f32le", "-"]
            raw_bytes = subprocess.check_output(ffmpeg_cmd)
            audio_samples = np.frombuffer(raw_bytes, dtype=np.float32).reshape(-1, n_channels)

        mono = audio_samples.mean(axis=1)
        total_frames = int(round(audio_duration_s * FPS))
    else:
        sr, n_channels, mono, audio_duration_s, total_frames = 0, 0, None, None, 0

    # 3. Ingest Transcription
    notes_raw = json.loads(notes_path.read_text(encoding="utf-8"))
    if audio_duration_s is None:
        if "seconds" not in notes_raw:
            fail('a transcription without audio needs a top-level "seconds" duration')
        audio_duration_s = float(notes_raw["seconds"])
        total_frames = int(round(audio_duration_s * FPS))
    notes_duration_s = float(notes_raw.get("seconds", audio_duration_s))

    # Validation: Mismatch check
    if abs(audio_duration_s - notes_duration_s) > cfg["duration_tolerance_s"]:
        print(
            f"[!] ERROR: Audio duration ({audio_duration_s:.2f}s) and notes duration ({notes_duration_s:.2f}s) mismatch by > {cfg['duration_tolerance_s']}s!",
            file=sys.stderr,
        )
        print("[!] Aborting to prevent silent desynchronization.", file=sys.stderr)
        sys.exit(1)

    raw_notes_list = notes_raw.get("notes", [])
    raw_notes_list.sort(key=lambda n: n["onset"])

    # 4./5. Waveform envelope and loudness per video frame
    floor = cfg["level_floor"]
    cols = cfg["waveform_cols_per_s"]
    if mono is not None:
        hop_wave = max(1, sr // cols)
        n_cols = len(mono) // hop_wave
        wave_peaks = [float(np.max(np.abs(mono[i * hop_wave : (i + 1) * hop_wave]))) for i in range(n_cols)]
        max_peak = max(wave_peaks) if wave_peaks else 1.0
        waveform_norm = [round(p / max_peak, 3) for p in wave_peaks]

        hop_rms = max(1, sr // FPS)
        rms_curve_db = []
        for i in range(total_frames):
            chunk = mono[i * hop_rms : (i + 1) * hop_rms]
            rms_val = np.sqrt(np.mean(chunk**2)) if len(chunk) > 0 else floor
            db = 20.0 * np.log10(max(float(rms_val), floor))
            rms_curve_db.append(round(float(db), 1))
    else:
        waveform_norm = []
        active = np.zeros(total_frames)
        for n in raw_notes_list:
            f0 = min(int(n["onset"] * FPS), total_frames - 1)
            f1 = max(int(math.ceil(n["offset"] * FPS)), f0 + 1)
            active[f0:f1] += 1.0
        relative = active / max(float(active.max()), 1.0)
        rms_curve_db = [round(float(20.0 * np.log10(max(v, floor))), 1) for v in relative]

    loudest_idx = int(np.argmax(rms_curve_db)) if rms_curve_db else 0
    loudest_info = {
        "frame": loudest_idx,
        "t": round(loudest_idx / FPS, 3),
        "db": rms_curve_db[loudest_idx] if rms_curve_db else 0.0,
    }

    # 6. Stems: whatever the transcriber produced
    melodic, drums = split_stems(raw_notes_list, cfg["stems"])
    stems = {**melodic, **drums}
    if not stems:
        fail("the transcription has no notes")
    if not melodic:
        fail("the transcription has no melodic instrument, so there is no drop or chord root")
    print(f"[*] Stems: {sorted(stems)}")

    # 7. Tempo from the onsets of all stems together
    tempo_cfg = cfg["tempo"]
    tempo_data = derive_tempo(dedup([n["onset"] for n in raw_notes_list], tempo_cfg["dedup_s"]), tempo_cfg)
    beat_s = tempo_data["beat_s"]

    # 8. Roles. "auto" picks the melodic stem lowest in register for the drop and the chord root, and
    # the last drum hit (any stem, or any note when there are no drums) for the impact.
    def pick(where: str, name: str, pool: dict) -> str:
        if name == "auto":
            return min(pool, key=lambda k: float(np.median([n["pitch"] for n in pool[k]])))
        if name not in stems:
            fail(f"{where} is `{name}`, which the transcription does not have ({sorted(stems)})")
        return name

    drop_stem = pick("structure.drop_stem", cfg["structure"]["drop_stem"], melodic)
    root_stem = pick("chords.root_stem", cfg["chords"]["root_stem"], melodic)
    impact_pool = drums or stems
    if cfg["structure"]["impact_stem"] == "auto":
        impact_t0 = max(float(n["onset"]) for v in impact_pool.values() for n in v)
    else:
        impact_t0 = max(float(n["onset"]) for n in stems[pick("structure.impact_stem", cfg["structure"]["impact_stem"], stems)])
    structure = {
        "drop_t": round(float(stems[drop_stem][0]["onset"]), 3),
        "bar_s": round(tempo_cfg["beats_per_bar"] * beat_s, 4),
        "impact_t": round(impact_t0 + beat_s, 3),
    }
    print(f"[*] Drop stem `{drop_stem}`, chord root `{root_stem}`")

    # 9. Optional physical frequency measurements at the first note of a named stem
    peaks = {}
    for name, p in cfg["peaks"].items():
        if mono is None or not stems.get(p["stem"]):
            peaks[name] = None
            continue
        n0 = stems[p["stem"]][0]
        a, b = float(n0["onset"]), float(n0["onset"]) + p["window_s"]
        if p["clip_to_note"]:
            b = min(b, float(n0["offset"]))
        peaks[name] = round(measure_peak_hz(mono[int(a * sr) : int(b * sr)], sr, p["lo_hz"], p["hi_hz"]), 1)

    chords = detect_chords(melodic, root_stem, cfg["chords"], tempo_cfg["beats_per_bar"], beat_s, audio_duration_s)

    def clean_note_record(n: dict) -> dict:
        return {
            "onset": round(float(n["onset"]), 4),
            "offset": round(float(n["offset"]), 4),
            "duration": round(float(n["offset"] - n["onset"]), 4),
            "pitch": int(n["pitch"]),
        }

    # Assemble complete schema
    analysis_doc = {
        "schema_version": SCHEMA_VERSION,
        "source": {
            "audio_file": str(audio_path) if audio_path is not None else "",
            "level_source": "audio" if mono is not None else "notes",
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
            "peaks_hz": peaks,
            "loudest": loudest_info,
        },
        "waveform": {
            "columns_per_second": cols,
            "peaks": waveform_norm,
        },
        "rms_curve_db": rms_curve_db,
        "chords": chords,
        "structure": structure,
        "stems": {name: [clean_note_record(n) for n in v] for name, v in stems.items() if v},
    }

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(analysis_doc, indent=2), encoding="utf-8")

    print(f"[*] Successfully wrote strict analysis to {out_path}")
    print(f"    - Audio SHA256: {audio_sha256[:16]}... ({audio_duration_s:.2f}s)")
    print(f"    - Notes SHA256: {notes_sha256[:16]}... ({len(raw_notes_list)} notes)")
    print(f"    - Measured BPM: {tempo_data['bpm']} ({tempo_data['derivation']}, confidence {tempo_data['confidence']})")
    print(f"    - Measured peaks (Hz): {peaks}")
    print(f"    - Loudest point: {loudest_info['db']} dB at t={loudest_info['t']}s")


def main():
    parser = argparse.ArgumentParser(description="Export strict analysis.json with SHA-256 hash checks.")
    parser.add_argument("--audio", type=Path, default=None, help="Source audio WAV; omit to analyse the transcription alone")
    parser.add_argument("--notes", type=Path, required=True, help="NeuralNote transcription JSON")
    parser.add_argument("--out", type=Path, required=True, help="Path of the analysis.json to write")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG, help="Export configuration (default vocab/export.json)")
    args = parser.parse_args()
    analyze(args.audio, args.notes, args.out, load_config(args.config))


if __name__ == "__main__":
    main()
