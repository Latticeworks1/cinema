#!/usr/bin/env python3
"""Transcribe a folder of per-track recordings into one notes.json for cinema.

Each track is transcribed on its own with the NeuralNote MuScriptor engine, so a melodic
note's stem is the track it was played on instead of an instrument guessed from a mix. Drum
notes keep the engine's drum identity so the exporter can split them into kick, snare and
hat; notes that start after the audio ends are dropped. Tracks whose
signal never rises above `--floor-db` are skipped. Per-track transcriptions are cached next to the output so a rerun
only repeats the merge.

    tools/transcribe_stems.py NOTES --dir FOLDER --map MAP.json [--floor-db -60]

MAP.json maps a file stem (the wav name without extension) to a stem name. Several files
may share one stem name, in which case their notes are merged. Files absent from the map are
ignored. The stems named here are the ones the vocabulary and the lens see, and the lens has at
most six slots.
"""

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
ENGINE_DIR = ROOT / "tools" / "neuralnote"
MODEL_NAME = "muscriptor-large-f16.gguf"


def model_path() -> Path:
    for p in (ENGINE_DIR / "models" / MODEL_NAME, Path.home() / "Library" / "NeuralNote" / "models" / MODEL_NAME):
        if p.exists():
            return p
    raise FileNotFoundError(f"{MODEL_NAME} not found in tools/neuralnote/models or ~/Library/NeuralNote/models")


def to_pcm(wav: Path, pcm: Path) -> np.ndarray:
    r = subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(wav), "-vn", "-ac", "1", "-ar", "16000",
                        "-f", "f32le", str(pcm)], capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"ffmpeg failed on {wav.name}: {r.stderr}")
    return np.fromfile(pcm, dtype=np.float32)


def transcribe_track(wav: Path, out: Path, floor_db: float) -> bool:
    """Write the engine's notes for one track to `out`; False if the track is silent."""
    with tempfile.NamedTemporaryFile(suffix=".f32") as f:
        pcm = Path(f.name)
        x = to_pcm(wav, pcm)
        peak_db = 20 * np.log10(float(np.abs(x).max()) + 1e-12) if len(x) else -240.0
        if peak_db < floor_db:
            print(f"[-] {wav.name}: peak {peak_db:.1f} dB below floor {floor_db}; skipped")
            return False
        print(f"[*] {wav.name}: peak {peak_db:.1f} dB, transcribing")
        r = subprocess.run([str(ENGINE_DIR / "transcribe"), str(model_path()), str(pcm), str(out)],
                           cwd=str(ENGINE_DIR), capture_output=True, text=True)
        if r.returncode != 0 or not out.exists() or out.stat().st_size == 0:
            raise RuntimeError(f"engine failed on {wav.name}:\n{r.stdout}\n{r.stderr}")
    return True


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("notes")
    ap.add_argument("--dir", required=True)
    ap.add_argument("--map", required=True)
    ap.add_argument("--floor-db", type=float, default=-60.0)
    a = ap.parse_args()

    notes_path = Path(a.notes).resolve()
    cache = notes_path.parent / "stems"
    cache.mkdir(parents=True, exist_ok=True)
    mapping = json.loads(Path(a.map).read_text())
    folder = Path(a.dir)

    merged, seconds = [], 0.0
    for file_stem, stem in mapping.items():
        wav = folder / f"{file_stem}.wav"
        if not wav.exists():
            sys.exit(f"[!] {wav} does not exist")
        out = cache / f"{stem}__{file_stem.replace(' ', '_').replace('#', 'n')}.json"
        if not (out.exists() and out.stat().st_size > 0):
            if not transcribe_track(wav, out, a.floor_db):
                continue
        data = json.loads(out.read_text())
        seconds = max(seconds, float(data.get("seconds", 0.0)))
        end = float(data["seconds"])
        for n in data["notes"]:
            if n["onset"] >= end:
                continue
            n = dict(n)
            n["offset"] = min(n["offset"], end)
            if not (n.get("is_drum") or n.get("program") == 128):
                n["instrument"] = stem
            merged.append(n)
        print(f"[+] {file_stem} -> {stem}: {len(data['notes'])} notes")

    merged.sort(key=lambda n: (n["onset"], n["pitch"]))
    notes_path.write_text(json.dumps({"sample_rate": 16000, "seconds": seconds, "notes": merged}, indent=1) + "\n")
    print(f"[Success] {len(merged)} notes in {len({n['instrument'] for n in merged})} stems -> {notes_path}")


if __name__ == "__main__":
    main()
