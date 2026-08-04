#!/bin/bash
# Build + run the golden reference renderer (OpenUtau headless).
#
# Requirements: dotnet SDK at ~/dotnet (or $DOTNET_ROOT), OPENUTAU_REF env
# pointing at the OpenUtau source tree, and libworldline.so for the
# runtime (copied next to the built binary automatically).
#
# Usage:
#   OPENUTAU_REF="/path/to/ref(openutau+openutau mobile)" ./build.sh
#   ./run.sh demo   out.wav [song.ustx]
#   ./run.sh note   out.wav <lyric> <tone> <durTicks>

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOTNET="${DOTNET_ROOT:-$HOME/dotnet}/dotnet"

if [[ -z "${OPENUTAU_REF:-}" ]]; then
  echo "ERROR: OPENUTAU_REF must point at the OpenUtau source tree" >&2
  exit 1
fi

cd "$SCRIPT_DIR"
export OPENUTAU_REF
"$DOTNET" build -c Release
OUT_DIR="$SCRIPT_DIR/bin/Release/net8.0"
# The WORLDLINE-R renderer dlopens "libworldline.so" from the app dir.
WORLDLINE_SO="${WORLDLINE_SO:-/home/seal/project/android-voice-synth/native/build/build-linux/libworldline.so}"
if [[ -f "$WORLDLINE_SO" && ! -f "$OUT_DIR/libworldline.so" ]]; then
  cp "$WORLDLINE_SO" "$OUT_DIR/libworldline.so"
fi
# OpenUtau writes its cache under $XDG_CACHE_HOME — create it up front.
mkdir -p "$(mktemp -d)/../golden-renderer-cache/OpenUtau" 2>/dev/null || true
mkdir -p /tmp/golden-renderer-cache/OpenUtau /tmp/golden-renderer-data

echo "BUILD OK — run with: ./run.sh <demo|note> ..."
