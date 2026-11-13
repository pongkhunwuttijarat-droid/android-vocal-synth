#!/usr/bin/env bash
# Real-file test: render an actual .ustx with the actual voicebank through the
# wasm engine in headless Chromium, and leave the result WAV at
# /tmp/wasm-real.wav so it can be compared with the native binary:
#
#   native/target/release/synth-cli render --engine r1-rs \
#       --project <same .ustx> --voicebank <same bank> --out /tmp/real-native.wav
#
# The pool render is also compared sample-by-sample with the main-thread render
# inside the page, so a mismatch fails here without needing the native binary.
#
#   PROJECT=/test/golden/sakura-test.ustx WORKERS=4 ./web/run-real.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
native="$here/../native"
root="$here/.."
project="${PROJECT:-/test/golden/sakura-test.ustx}"
workers="${WORKERS:-4}"

export PATH="$HOME/.cargo/bin:$PATH"

echo "==> building engine-wasm (release, wasm SIMD)"
( cd "$native" && RUSTFLAGS="-C target-feature=+simd128" \
    cargo build --release --target wasm32-unknown-unknown -p engine-wasm )

echo "==> generating JS bindings"
rm -rf "$here/pkg"
mkdir -p "$here/pkg"
wasm-bindgen --target web --out-dir "$here/pkg" \
  "$native/target/wasm32-unknown-unknown/release/engine_wasm.wasm"

out="$native/target/real-result.txt"
rm -f "$out" /tmp/wasm-real.wav
python3 "$here/bench-server.py" "$root" > "$out" 2>&1 &
server=$!
chromium_pid=""
cleanup() {
  if [ -n "$chromium_pid" ]; then kill "$chromium_pid" 2>/dev/null || true; fi
  kill "$server" 2>/dev/null || true
}
trap cleanup EXIT

# NB: with `set -o pipefail`, `port="$(grep … | awk …)"` aborts the whole
# script on the first miss (grep exits 1), so the probe is guarded instead.
port=""
for _ in $(seq 1 40); do
  if grep -q '^PORT' "$out" 2>/dev/null; then
    port="$(awk '/^PORT/ {print $2; exit}' "$out")"
    break
  fi
  sleep 0.25
done
if [ -z "$port" ]; then echo "server did not start" >&2; cat "$out" >&2; exit 1; fi

extra=""
if [ "${SKIP_SEQ:-0}" = "1" ]; then extra="$extra&skipSeq=1"; fi
if [ "${ALLOW_SKIPPED:-0}" = "1" ]; then extra="$extra&allowSkipped=1"; fi
url="http://127.0.0.1:$port/web/real-test.html?project=$project&workers=$workers$extra"
echo "==> headless chromium (port $port) — $project"

prof="$(mktemp -d)"
chromium-browser --headless=new --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --no-zygote --no-first-run \
  --no-default-browser-check --user-data-dir="$prof" "$url" >/dev/null 2>&1 &
chromium_pid=$!

# Wait for the page to POST its result. Real time, not --virtual-time-budget,
# so the timings the page reports are wall-clock numbers.
for _ in $(seq 1 600); do
  if grep -q "^RESULT" "$out" 2>/dev/null; then break; fi
  if ! kill -0 "$chromium_pid" 2>/dev/null; then break; fi
  sleep 0.5
done
sleep 1

sed -n '/^RESULT: /,$p' "$out" | head -60

if grep -q "RESULT: PASS" "$out"; then
  echo "==> OK (wav: /tmp/wasm-real.wav)"
else
  echo "==> FAILED" >&2
  tail -30 "$out" >&2
  exit 1
fi
