#!/usr/bin/env bash
# analyze -> plan -> flat pass -> lens pass. Writes out/cinema.mp4 (1920x1080, 30 fps, with audio).
# Run from anywhere: tools/render.sh
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p out/pass

echo "[1/5] Measuring the recording (data/analysis.json)"
tools/analyze.sh

# The flat pass builds its SVG from strings and the lens pass uses typed shader layers. fframes
# needs `compile-time-svgtree` for the second and cannot have it for the first, so the two
# binaries are built in separate cargo invocations.
echo "[2/5] Building"
cargo build --release -p direct
cargo build --release -p cinema-flat
cargo build --release -p cinema-lens

echo "[3/5] Planning scenes (out/plan.json)"
cargo run --release -q -p direct --bin plan -- data/analysis.json vocab

cp data/audio.wav out/pass/music.wav

echo "[4/5] Pass 1: the stage, flat (3840x2160)"
cargo run --release -q -p cinema-flat -- render -o out/pass/flat.mp4

echo "[5/5] Pass 2: the lens"
cargo run --release -q -p cinema-lens -- render -o out/cinema.mp4

ffprobe -v error -show_entries stream=codec_type,width,height,nb_frames,duration -of compact out/cinema.mp4
echo "Render complete: out/cinema.mp4"
