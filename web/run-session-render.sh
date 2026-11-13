#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
native="$here/../native"
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> building engine-wasm"
( cd "$native" && RUSTFLAGS="-C target-feature=+simd128" \
    cargo build --release --target wasm32-unknown-unknown -p engine-wasm )

echo "==> generating JS bindings"
rm -rf "$here/pkg"
mkdir -p "$here/pkg"
wasm-bindgen --target web --out-dir "$here/pkg" \
  "$native/target/wasm32-unknown-unknown/release/engine_wasm.wasm"

out="$native/target/session-render-result.txt"
rm -f "$out"
python3 "$here/bench-server.py" > "$out" 2>&1 &
server=$!
chromium_pid=""
cleanup() {
  if [ -n "$chromium_pid" ]; then kill "$chromium_pid" 2>/dev/null || true; fi
  kill "$server" 2>/dev/null || true
  if [ -n "${prof:-}" ]; then rm -rf "$prof"; fi
}
trap cleanup EXIT

port=""
for _ in $(seq 1 40); do
  if grep -q '^PORT' "$out" 2>/dev/null; then
    port="$(awk '/^PORT/ {print $2; exit}' "$out")"
    break
  fi
  sleep 0.25
done
if [ -z "$port" ]; then echo "server did not start" >&2; cat "$out" >&2; exit 1; fi

echo "==> headless browser Session render adapter check (port $port)"
prof="$(mktemp -d)"
chromium-browser --headless=new --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --no-zygote --no-first-run \
  --no-default-browser-check --window-size=1100,800 \
  --user-data-dir="$prof" "http://127.0.0.1:$port/session-render-test.html" >/dev/null 2>&1 &
chromium_pid=$!

for _ in $(seq 1 240); do
  if grep -q '^RESULT:' "$out" 2>/dev/null; then break; fi
  if ! kill -0 "$chromium_pid" 2>/dev/null; then break; fi
  sleep 0.5
done
sed -n '/^PORT/,$p' "$out" | tail -40
if grep -q '^RESULT: PASS' "$out"; then
  echo "==> OK"
else
  echo "==> FAILED" >&2
  tail -40 "$out" >&2
  exit 1
fi
