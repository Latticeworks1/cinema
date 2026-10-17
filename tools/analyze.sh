#!/usr/bin/env bash
# Measure the recording and write data/analysis.json (hash-checked inputs: data/audio.wav, data/notes.json).
set -euo pipefail
cd "$(dirname "$0")/.."
PY="/opt/homebrew/Caskroom/miniconda/base/envs/pitch/bin/python"
[ -x "$PY" ] || PY="python3"
"$PY" python/export_analysis.py --audio data/audio.wav --notes data/notes.json --out data/analysis.json
