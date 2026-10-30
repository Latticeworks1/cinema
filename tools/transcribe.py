#!/usr/bin/env python3
"""NeuralNote / MuScriptor audio transcriber module for ff-cinema.

Transcribes an audio file (.wav, .mp3, etc.) using the Metal-accelerated NeuralNote
MuScriptor engine, outputting both the strict notes.json expected by ff-cinema and
individual stem MIDI files.
"""

from collections import defaultdict
import json
from pathlib import Path
import subprocess
import sys
import tempfile

try:
    import mido
except ImportError:
    mido = None


def export_stem_midis(notes_json_path: Path, output_dir: Path, stem_prefix: str = ""):
    """Export MuScriptor note JSON as type-1 MIDI plus individual instrument stem files."""
    if mido is None:
        print("[!] mido library not available; skipping MIDI export.")
        return

    data = json.loads(notes_json_path.read_text(encoding="utf-8"))
    limit = float(data.get("seconds", 0.0))
    groups = defaultdict(list)
    for n in data.get("notes", []):
        pitch = int(n.get("pitch", 0))
        onset = float(n.get("onset", 0.0))
        offset = float(n.get("offset", 0.0))
        if 0 <= pitch <= 127 and 0 <= onset < limit and offset > onset:
            n_copy = dict(n, offset=min(offset, limit))
            prog = 128 if n.get("is_drum") else int(n.get("program", 0))
            inst = n.get("instrument", "unknown")
            groups[(prog, inst)].append(n_copy)

    tpb = 960
    tempo = 500000  # 120 BPM timing carrier

    def ticks(seconds: float) -> int:
        return round(mido.second2tick(seconds, tpb, tempo))

    def conductor():
        return mido.MidiTrack([
            mido.MetaMessage("track_name", name="Timing: 120 BPM carrier; original seconds preserved"),
            mido.MetaMessage("set_tempo", tempo=tempo),
            mido.MetaMessage("end_of_track", time=ticks(limit)),
        ])

    def track_for(program: int, name: str, notes: list[dict], channel: int):
        tr = mido.MidiTrack([mido.MetaMessage("track_name", name=name)])
        if program != 128:
            tr.append(mido.Message("program_change", channel=channel, program=program))
        events = []
        for n in notes:
            start = ticks(float(n["onset"]))
            end = max(start + 1, ticks(float(n["offset"])))
            pitch = int(n["pitch"])
            events.append((start, 1, pitch))
            events.append((end, 0, pitch))
        last = 0
        for tick, on, pitch in sorted(events):
            tr.append(mido.Message(
                "note_on" if on else "note_off",
                channel=channel,
                note=pitch,
                velocity=80 if on else 0,
                time=tick - last,
            ))
            last = tick
        tr.append(mido.MetaMessage("end_of_track", time=max(0, ticks(limit) - last)))
        return tr

    combined = mido.MidiFile(type=1, ticks_per_beat=tpb)
    combined.tracks.append(conductor())
    channels = [c for c in range(16) if c != 9]
    summary = []
    prefix = f"{stem_prefix}-" if stem_prefix else ""

    for i, ((program, name), notes) in enumerate(sorted(groups.items())):
        channel = 9 if program == 128 else channels[i % len(channels)]
        combined.tracks.append(track_for(program, name, notes, channel))
        single = mido.MidiFile(type=1, ticks_per_beat=tpb)
        single.tracks.extend([conductor(), track_for(program, name, notes, 9 if program == 128 else 0)])
        filename = f"{prefix}{program:03d}-{name}.mid"
        single.save(output_dir / filename)
        summary.append({"instrument": name, "program": program, "notes": len(notes), "file": filename})

    combined_name = f"{prefix}full.mid"
    combined.save(output_dir / combined_name)
    report = {
        "seconds": limit,
        "note_count": sum(len(v) for v in groups.values()),
        "tracks": summary,
        "combined_midi": combined_name,
    }
    summary_path = output_dir / f"{prefix}transcription-summary.json"
    summary_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"[*] Exported {len(summary)} stem MIDI tracks to {output_dir}")


def transcribe(audio_path: Path, notes_path: Path, export_midi: bool = True):
    """Convert audio to 16kHz PCM and run the NeuralNote MuScriptor engine."""
    repo_root = Path(__file__).resolve().parent.parent
    nn_dir = repo_root / "tools" / "neuralnote"
    engine_bin = nn_dir / "transcribe"
    model_path = nn_dir / "models" / "muscriptor-large-f16.gguf"

    if not engine_bin.exists():
        raise FileNotFoundError(f"NeuralNote engine not found at {engine_bin}")
    if not model_path.exists():
        fallback = Path.home() / "Library" / "NeuralNote" / "models" / "muscriptor-large-f16.gguf"
        if fallback.exists():
            model_path = fallback
        else:
            raise FileNotFoundError(f"Model checkpoint not found at {model_path} or {fallback}")

    notes_path = notes_path.resolve()
    notes_path.parent.mkdir(parents=True, exist_ok=True)

    if notes_path.exists() and notes_path.stat().st_size > 0:
        print(f"[*] Transcription already exists at {notes_path.name}; reusing.")
        if export_midi:
            stem_prefix = audio_path.stem
            export_stem_midis(notes_path, notes_path.parent, stem_prefix=stem_prefix)
        return

    with tempfile.NamedTemporaryFile(suffix=".f32", delete=True) as pcm_file:
        pcm_path = Path(pcm_file.name)
        print(f"[*] Extracting 16kHz float32 PCM from {audio_path.name}...")
        ffmpeg_cmd = [
            "ffmpeg",
            "-v", "error",
            "-y",
            "-i", str(audio_path),
            "-vn",
            "-ac", "1",
            "-ar", "16000",
            "-f", "f32le",
            str(pcm_path),
        ]
        res = subprocess.run(ffmpeg_cmd, capture_output=True, text=True)
        if res.returncode != 0:
            raise RuntimeError(f"ffmpeg PCM conversion failed: {res.stderr}")

        print("[*] Running NeuralNote MuScriptor Metal engine...")
        nn_cmd = [
            str(engine_bin),
            str(model_path),
            str(pcm_path),
            str(notes_path),
        ]
        res = subprocess.run(nn_cmd, cwd=str(nn_dir), capture_output=True, text=True)
        if res.returncode != 0:
            raise RuntimeError(f"NeuralNote transcription failed:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}")

        print(res.stdout.strip())

    if not notes_path.exists() or notes_path.stat().st_size == 0:
        raise RuntimeError(f"NeuralNote transcription produced no output at {notes_path}")

    # Generate companion stem MIDIs
    if export_midi:
        stem_prefix = audio_path.stem
        export_stem_midis(notes_path, notes_path.parent, stem_prefix=stem_prefix)
    print(f"[Success] Notes saved to {notes_path}")


def main():
    if len(sys.argv) < 3:
        print("Usage: python3 tools/transcribe.py <audio_path> <notes_path>")
        sys.exit(2)

    audio_path = Path(sys.argv[1]).resolve()
    notes_path = Path(sys.argv[2]).resolve()

    if not audio_path.exists():
        print(f"[!] Audio file does not exist: {audio_path}", file=sys.stderr)
        sys.exit(1)

    transcribe(audio_path, notes_path)


if __name__ == "__main__":
    main()
