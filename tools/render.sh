#!/usr/bin/env bash
# packet -> analysis -> plan -> flat pass -> lens pass.
# Usage: tools/render.sh PACKET.json | AUDIO_FILE [range]
# An audio file of any name or format gets a packet written for it first (tools/make_packet.py).
# Everything is written under <out>/<packet name>/ (see `packet show`).
# CINEMA_PYTHON selects the interpreter for the analysis (default python3).
set -euo pipefail
cd "$(dirname "$0")/.."
ARG="${1:?usage: tools/render.sh PACKET.json | AUDIO_FILE}"
case "$ARG" in
    *.json) export CINEMA_PACKET="$ARG" ;;
    *)      export CINEMA_PACKET="$(python3 tools/make_packet.py "$ARG" --force)" ;;
esac
mkdir -p out

RANGE="${2:-}"

# The flat pass builds its SVG from strings and the lens pass uses typed shader layers. fframes
# needs `compile-time-svgtree` for the second and cannot have it for the first, so the two
# binaries are built in separate cargo invocations.
echo "[1/5] Building"
cargo build --release -p packet -p direct
cargo build --release -p cinema-flat
cargo build --release -p cinema-lens
packet() { target_dir=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])'); "$target_dir/release/packet" "$@"; }

echo "[2/5] Preparing inputs from $CINEMA_PACKET"
packet prepare "$CINEMA_PACKET"
FLAT=$(packet path "$CINEMA_PACKET" flat)
VIDEO=$(packet path "$CINEMA_PACKET" video)

echo "[3/5] Planning scenes"
cargo run --release -q -p direct --bin plan

if [ -n "$RANGE" ]; then
    echo "[4/5] Pass 1: the stage, flat (3840x2160) [range: $RANGE]"
    cargo run --release -q -p cinema-flat -- render "$RANGE" -o "$FLAT"
    OFFSET=$(echo "$RANGE" | sed -E 's/^([0-9.]+).*/\1/')
    echo "[5/5] Pass 2: the lens [range: $RANGE, offset: ${OFFSET}s]"
    CINEMA_OFFSET_S="$OFFSET" cargo run --release -q -p cinema-lens -- render "$RANGE" -o "$VIDEO"
else
    echo "[4/5] Pass 1: the stage, flat (3840x2160)"
    cargo run --release -q -p cinema-flat -- render -o "$FLAT"
    echo "[5/5] Pass 2: the lens"
    cargo run --release -q -p cinema-lens -- render -o "$VIDEO"
fi

ffprobe -v error -show_entries stream=codec_type,width,height,nb_frames,duration -of compact "$VIDEO"
echo "Render complete: $VIDEO"
