#!/usr/bin/env python3
"""Align known lyrics to the timed words of a decode and write lyric cues into a packet.

    tools/align_lyrics.py LYRICS.txt WORDS.json PACKET.json [--report REPORT.json] [--lead 0.05]
                          [--couplet 2] [--min-sim 0.6]

LYRICS.txt is the sung text, one line per sung line, a blank line between sections; a line that
is only a bracketed label such as [chorus] is skipped. WORDS.json is the output of
tools/whisper_words.py. The two word sequences are aligned monotonically (Needleman-Wunsch with a
fuzzy word similarity), so repeated stanzas are told apart by order rather than by text. A lyric
word that pairs with a decoded word takes that word's real onset; a lyric word the decode missed
takes a time interpolated between its matched neighbours and is marked in the report. Lyrics
after the last decoded word have no time and are listed as untimed, not guessed. The decoded
words are the evidence for time, so every lyric word's `src` in the report says whether its time
was heard ("heard"), interpolated ("interp") or absent ("none").

Cues group `--couplet` lines (within one section) so a cue reads as one or two sublines
joined by " / ". The packet's `set.direction.cues` is replaced.
"""

import argparse
import difflib
import json
import re
from pathlib import Path

GAP_DECODED = -1.0   # a decoded word with no lyric partner (a mishear or an adlib)
GAP_LYRIC = -0.7     # a lyric word the decode did not produce
MISS = -1.0          # a decoded and a lyric word paired but unlike


def norm(w: str) -> str:
    return re.sub(r"[^a-z0-9]", "", w.lower().replace("'", "").replace("’", ""))


def sim(a: str, b: str) -> float:
    if not a or not b:
        return 0.0
    return difflib.SequenceMatcher(None, a, b).ratio()


def read_lyrics(path: Path) -> list[dict]:
    """Tokens with their line and section indices."""
    toks, line_i, sec_i, prev_blank = [], 0, 0, False
    for raw in path.read_text().splitlines():
        s = raw.strip()
        if not s:
            if toks and not prev_blank:
                sec_i += 1
            prev_blank = True
            continue
        prev_blank = False
        if re.fullmatch(r"\[[^\]]*\]", s):
            continue
        for w in s.split():
            if norm(w):
                toks.append({"w": w, "n": norm(w), "line": line_i, "sec": sec_i})
        line_i += 1
    return toks


def align(lyr: list[dict], dec: list[dict], min_sim: float) -> list[int | None]:
    """For each lyric token the index of the decoded word it pairs with, else None."""
    n, m = len(lyr), len(dec)
    NEG = -1e9
    dp = [[NEG] * (m + 1) for _ in range(n + 1)]
    bk = [[0] * (m + 1) for _ in range(n + 1)]
    dp[0][0] = 0.0
    for i in range(n + 1):
        for j in range(m + 1):
            if i == 0 and j == 0:
                continue
            best, how = NEG, 0
            if i > 0 and j > 0 and dp[i - 1][j - 1] > NEG:
                s = sim(lyr[i - 1]["n"], norm(dec[j - 1]["w"]))
                v = dp[i - 1][j - 1] + (2.0 * s if s >= min_sim else MISS)
                best, how = v, 1
            if i > 0 and dp[i - 1][j] > NEG and dp[i - 1][j] + GAP_LYRIC > best:
                best, how = dp[i - 1][j] + GAP_LYRIC, 2
            if j > 0 and dp[i][j - 1] > NEG and dp[i][j - 1] + GAP_DECODED > best:
                best, how = dp[i][j - 1] + GAP_DECODED, 3
            dp[i][j], bk[i][j] = best, how
    # The decode ends before the lyrics do: trailing lyric words cost nothing.
    i = max(range(n + 1), key=lambda k: dp[k][m])
    j, pair = m, [None] * n
    while i > 0 or j > 0:
        how = bk[i][j]
        if how == 1:
            if sim(lyr[i - 1]["n"], norm(dec[j - 1]["w"])) >= min_sim:
                pair[i - 1] = j - 1
            i, j = i - 1, j - 1
        elif how == 2:
            i -= 1
        else:
            j -= 1
    return pair


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("lyrics")
    ap.add_argument("words")
    ap.add_argument("packet")
    ap.add_argument("--report")
    ap.add_argument("--lead", type=float, default=0.05)
    ap.add_argument("--couplet", type=int, default=2)
    ap.add_argument("--min-sim", type=float, default=0.6)
    a = ap.parse_args()

    lyr = read_lyrics(Path(a.lyrics))
    dec = [w for w in json.loads(Path(a.words).read_text())["words"] if norm(w["w"])]
    pair = align(lyr, dec, a.min_sim)

    for tok, p in zip(lyr, pair):
        tok["t"], tok["src"] = (dec[p]["t0"], "heard") if p is not None else (None, "none")
    heard = [i for i, t in enumerate(lyr) if t["src"] == "heard"]
    for k, i in enumerate(heard):
        nxt = heard[k + 1] if k + 1 < len(heard) else None
        if nxt is None:
            break
        for g in range(i + 1, nxt):
            f = (g - i) / (nxt - i)
            lyr[g]["t"] = round(lyr[i]["t"] + f * (lyr[nxt]["t"] - lyr[i]["t"]), 3)
            lyr[g]["src"] = "interp"
    first = heard[0] if heard else 0
    for g in range(0, first):  # lyric words before the first heard word
        lyr[g]["src"] = "none"

    # Lines, then cues of `couplet` lines inside a section.
    lines: dict[int, list[dict]] = {}
    for t in lyr:
        lines.setdefault(t["line"], []).append(t)
    sections: dict[int, list[int]] = {}
    for li, toks in lines.items():
        sections.setdefault(toks[0]["sec"], []).append(li)

    cues, untimed = [], []
    for sec, lis in sections.items():
        for k in range(0, len(lis), a.couplet):
            group = [lines[li] for li in lis[k:k + a.couplet]]
            toks = [t for ln in group for t in ln]
            if any(t["t"] is None for t in toks):
                untimed.append(" / ".join(" ".join(norm(t["w"]) and t["w"].lower().strip(",.!?") for t in ln) for ln in group))
                continue
            text = " / ".join(" ".join(t["w"].lower().strip(",.!?;:") for t in ln) for ln in group)
            cues.append({"t": round(max(toks[0]["t"] - a.lead, 0.0), 3), "label": " ".join(text.split()[:3]),
                         "text": text, "word_times": [round(t["t"], 3) for t in toks]})
    cues.sort(key=lambda c: c["t"])

    path = Path(a.packet)
    packet = json.loads(path.read_text())
    packet.setdefault("set", {}).setdefault("direction", {})["cues"] = cues
    path.write_text(json.dumps(packet, indent=2) + "\n")

    counts = {s: sum(t["src"] == s for t in lyr) for s in ("heard", "interp", "none")}
    unheard_dec = [dec[j]["w"] for j in range(len(dec)) if j not in {p for p in pair if p is not None}]
    print(f"lyric words {len(lyr)}: heard {counts['heard']}, interpolated {counts['interp']}, untimed {counts['none']}")
    print(f"decoded words {len(dec)}: unpaired {len(unheard_dec)}: {' '.join(unheard_dec)}")
    print(f"{len(cues)} timed cues, {len(untimed)} untimed -> {path}")
    for c in cues:
        print(f"  {c['t']:7.2f}  {c['text']}")
    if untimed:
        print("untimed:")
        for u in untimed:
            print("  ", u)
    if a.report:
        Path(a.report).write_text(json.dumps({"words": [{k: t[k] for k in ("w", "line", "sec", "t", "src")} for t in lyr],
                                              "unpaired_decoded": unheard_dec}, indent=1) + "\n")


if __name__ == "__main__":
    main()
