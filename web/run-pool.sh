#!/usr/bin/env bash
# Verify RenderPool (web/pool.js): per-chunk sample loading, the chunk cache,
# and incremental re-render after an edit.
#
# Checks that matter:
#   * every render is bit-identical to a single-threaded full render,
#   * an unchanged project renders 0 chunks (pure re-mix),
#   * editing one note re-renders only the chunks whose hash changed,
#   * a worker loads only the samples its chunks reference.
#
# Needs web/fixture to be a self-contained bank:
#   web/fixture/bank/voice/oto.ini + *.wav + *_wav.frq
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
native="$here/../native"
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> building engine-wasm (release, wasm SIMD)"
( cd "$native" && RUSTFLAGS="-C target-feature=+simd128" \
    cargo build --release --target wasm32-unknown-unknown -p engine-wasm )

echo "==> generating JS bindings"
rm -rf "$here/pkg"
mkdir -p "$here/pkg"
wasm-bindgen --target web --out-dir "$here/pkg" \
  "$native/target/wasm32-unknown-unknown/release/engine_wasm.wasm"

out="$native/target/pool-result.txt"
rm -f "$out"
python3 "$here/bench-server.py" > "$out" 2>&1 &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT
sleep 2
port="$(grep -m1 '^PORT' "$out" | awk '{print $2}')"
[ -n "$port" ] || { echo "server did not start" >&2; exit 1; }

echo "==> running headless chromium (port $port)"
prof="$(mktemp -d)"
timeout 400 chromium-browser --headless=new --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --no-zygote --no-first-run \
  --no-default-browser-check --user-data-dir="$prof" \
  "http://127.0.0.1:$port/pool-test.html?run=1" >/dev/null 2>&1 || true
sleep 3

sed -n '/^RESULT$/,/^RESULT: /p' "$out" | head -40

if grep -q "RESULT: PASS" "$out" && ! grep -q "MISMATCH" "$out"; then
  echo "==> OK"
else
  echo "==> FAILED" >&2
  exit 1
fi
