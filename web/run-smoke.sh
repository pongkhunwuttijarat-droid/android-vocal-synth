#!/usr/bin/env bash
# Headless browser smoke test for the wasm engine.
#
# Builds the wasm module + JS bindings, serves this directory over HTTP
# (WebAssembly + ES modules need a real origin), and drives a Chromium page
# that loads a voicebank, renders the demo song, and checks the output.
#
# Expected result: RESULT: PASS with 132066 samples, peak 0.9884 — the same
# numbers the native build produces (see native/tools/world-oracle).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
native="$here/../native"
port="${PORT:-8901}"

export PATH="$HOME/.cargo/bin:$PATH"

echo "==> building engine-wasm (release, wasm SIMD)"
# `+simd128` lets rustfft take its f64 SIMD path (the module requires a
# SIMD-capable browser, which is every current one). Drop RUSTFLAGS to build
# the scalar fallback; the engine reports which one it got via fft_backend().
( cd "$native" && RUSTFLAGS="-C target-feature=+simd128" \
    cargo build --release --target wasm32-unknown-unknown -p engine-wasm )

echo "==> generating JS bindings"
rm -rf "$here/pkg"
mkdir -p "$here/pkg"
wasm-bindgen --target web --out-dir "$here/pkg" \
  "$native/target/wasm32-unknown-unknown/release/engine_wasm.wasm"

echo "==> serving $here on 127.0.0.1:$port"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$here" >/dev/null 2>&1 &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT

# Wait for the server to accept connections.
for _ in $(seq 1 40); do
  if curl -sf "http://127.0.0.1:$port/smoke.html" >/dev/null; then break; fi
  sleep 0.25
done

echo "==> running headless chromium"
prof="$(mktemp -d)"
out="$(timeout 240 chromium-browser \
  --headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage \
  --no-zygote --single-process --no-first-run --no-default-browser-check \
  --user-data-dir="$prof" --virtual-time-budget=180000 \
  --dump-dom "http://127.0.0.1:$port/smoke.html" 2>/dev/null || true)"

# The DOM dump also contains the page's own <script> source; only lines the
# page emitted carry the SMOKE| prefix.
echo "$out" | grep -oE "SMOKE\| [^<]*" | sed 's/^SMOKE| //' || true

if echo "$out" | grep -q "SMOKE| RESULT: PASS"; then
  echo "==> OK"
else
  echo "==> FAILED" >&2
  exit 1
fi
