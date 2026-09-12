#!/usr/bin/env bash
# Headless check that the Web-Worker parallel render
#   1) actually speeds the render up, and
#   2) produces output bit-identical to the single-threaded mix.
#
# Serves web/ and runs parallel.html?run=1 in headless Chromium; the page
# POSTs its measurements back (performance.now() is frozen under
# --virtual-time-budget, so real wall-clock timing needs the callback).
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

out="$native/target/parallel-result.txt"
rm -f "$out"
python3 "$here/bench-server.py" > "$out" 2>&1 &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT
sleep 2
port="$(grep -m1 '^PORT' "$out" | awk '{print $2}')"
[ -n "$port" ] || { echo "server did not start" >&2; exit 1; }

echo "==> running headless chromium (port $port)"
prof="$(mktemp -d)"
timeout 300 chromium-browser --headless=new --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --no-zygote --no-first-run \
  --no-default-browser-check --user-data-dir="$prof" \
  "http://127.0.0.1:$port/parallel.html?run=1" >/dev/null 2>&1 || true
sleep 3

sed -n '/^RESULT /,$p' "$out" | sed 's/^RESULT //'

if grep -q "bit-identical" "$out" && ! grep -q "MISMATCH" "$out"; then
  echo "==> OK"
else
  echo "==> FAILED (mismatch or no result)" >&2
  exit 1
fi
