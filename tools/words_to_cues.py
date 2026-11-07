#!/usr/bin/env python3
"""Turn timed words from tools/whisper_words.py into lyric cues in a packet.

    tools/words_to_cues.py WORDS.json PACKET.json [--gap 0.8] [--max-words 9] [--lead 0.05] [--fixes FIXES.json]

FIXES.json is a list of {"from": "where your", "to": "the way your"} corrections applied to the
decoded words before lines are built. A match is on the cleaned lowercase word sequence. When the
replacement has a different number of words, they share the matched span's time in proportion to
their lengths, so the first replaced word keeps the original onset.

A line ends at a pause longer than `--gap` seconds, at sentence-final punctuation, or at
`--max-words`; commas inside a line become the " / " sublines the lyric layer draws on separate
rows. Each cue starts `--lead` seconds before its first word and carries `word_times`, the real
onset of every word in reading order, so the line is revealed in time with the singing. Palette,
layout and camera are left to the planner. The packet's `set.direction.cues` is replaced.
"""

import argparse
import json
import re
from pathlib import Path


def clean(word: str) -> str:
    return re.sub(r"[^\w'’-]", "", word).lower()


def apply_fixes(words: list[dict], fixes: list[dict]) -> list[dict]:
    for fx in fixes:
        src, dst = fx["from"].lower().split(), fx["to"].lower().split()
        i = 0
        while i + len(src) <= len(words):
            if [clean(w["w"]) for w in words[i:i + len(src)]] != src:
                i += 1
                continue
            t0, t1 = words[i]["t0"], words[i + len(src) - 1]["t1"]
            total = sum(len(x) for x in dst)
            new, t = [], t0
            for x in dst:
                d = (t1 - t0) * len(x) / total
                new.append({"w": x, "t0": round(t, 3), "t1": round(t + d, 3), "p": 1.0})
                t += d
            words[i:i + len(src)] = new
            i += len(dst)
    return words


def split_lines(words: list[dict], gap: float, max_words: int) -> list[list[dict]]:
    lines, cur = [], []
    for i, w in enumerate(words):
        cur.append(w)
        nxt = words[i + 1] if i + 1 < len(words) else None
        end = (nxt is None or nxt["t0"] - w["t1"] > gap or len(cur) >= max_words
               or w["w"].endswith((".", "?", "!")))
        if end:
            lines.append(cur)
            cur = []
    return lines


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("words")
    ap.add_argument("packet")
    ap.add_argument("--gap", type=float, default=0.8)
    ap.add_argument("--max-words", type=int, default=9)
    ap.add_argument("--lead", type=float, default=0.05)
    ap.add_argument("--fixes")
    a = ap.parse_args()

    words = [w for w in json.loads(Path(a.words).read_text())["words"] if clean(w["w"])]
    if a.fixes:
        words = apply_fixes(words, json.loads(Path(a.fixes).read_text()))
    cues = []
    for line in split_lines(words, a.gap, a.max_words):
        subs, cur = [], []
        for w in line:
            cur.append(clean(w["w"]))
            if w["w"].endswith(",") and len(cur) >= 2:
                subs.append(" ".join(cur))
                cur = []
        if cur:
            subs.append(" ".join(cur))
        cues.append({"t": round(max(line[0]["t0"] - a.lead, 0.0), 3),
                     "label": " ".join(clean(w["w"]) for w in line[:3]),
                     "text": " / ".join(subs),
                     "word_times": [round(w["t0"], 3) for w in line]})

    path = Path(a.packet)
    packet = json.loads(path.read_text())
    packet.setdefault("set", {}).setdefault("direction", {})["cues"] = cues
    path.write_text(json.dumps(packet, indent=2) + "\n")
    print(f"{len(cues)} cues from {len(words)} words -> {path}")
    for c in cues:
        print(f"  {c['t']:7.2f}  {c['text']}")


if __name__ == "__main__":
    main()
