#!/usr/bin/env python3
"""Transcribe a vocal recording to words with start and end times, using faster-whisper.

    tools/whisper_words.py AUDIO OUT.json [--model large-v3] [--language en] [--offset 0.0]

OUT.json holds {"language", "duration", "words": [{"w", "t0", "t1", "p"}]} with times in track
seconds (`--offset` shifts a recording that starts later than the track). `p` is the word
probability the model reports, kept so a weak decode can be seen before it is trusted.
"""

import argparse
import json
from pathlib import Path

from faster_whisper import WhisperModel


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("audio")
    ap.add_argument("out")
    ap.add_argument("--model", default="large-v3")
    ap.add_argument("--language", default=None)
    ap.add_argument("--offset", type=float, default=0.0)
    a = ap.parse_args()

    model = WhisperModel(a.model, device="cpu", compute_type="int8")
    segments, info = model.transcribe(a.audio, word_timestamps=True, language=a.language,
                                      vad_filter=True, condition_on_previous_text=False)
    words = []
    for seg in segments:
        for w in seg.words or []:
            words.append({"w": w.word.strip(), "t0": round(w.start + a.offset, 3),
                          "t1": round(w.end + a.offset, 3), "p": round(w.probability, 3)})
    out = {"language": info.language, "language_probability": round(info.language_probability, 3),
           "duration": round(info.duration, 3), "model": a.model, "words": words}
    Path(a.out).write_text(json.dumps(out, indent=1) + "\n")
    print(f"{info.language} ({info.language_probability:.2f}): {len(words)} words over {info.duration:.1f} s -> {a.out}")


if __name__ == "__main__":
    main()
