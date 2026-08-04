#!/bin/bash
# Build libmixerfx.so for Linux desktop (cmake — see worldline-linux.sh).
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
BUILD_DIR="$ROOT/build/build-linux-mixerfx"
mkdir -p "$BUILD_DIR"
cmake -S "$ROOT/plugins/mixer-fx" -B "$BUILD_DIR" -DCMAKE_BUILD_TYPE=Release >/dev/null
cmake --build "$BUILD_DIR" --target mixer_fx -j"$(nproc)" >/dev/null
cp "$BUILD_DIR/libmixer_fx.so" "$ROOT/build/build-linux/libmixerfx.so"
file "$ROOT/build/build-linux/libmixerfx.so"
