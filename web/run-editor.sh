#!/usr/bin/env bash
# Headless regression harness for the Web editor.
#
# This used to drive `web/editor-test.html`, the legacy editor page, which was
# deleted on 2026-10-03 when the OpenUtau clone became the only Web UI. The
# clone has its own probe (`ui-mock/react-dock/tools/drag-probe.mjs`), which
# drives real trusted input over CDP and asserts against the Rust session, so
# this script now serves the clone and runs that probe instead. It is the same
# contract the old harness had — boot, edit, undo, render, exit non-zero on
# failure — against the page that actually ships.
#
# It still rebuilds engine-wasm and the bindings first: the clone loads
# `web/pkg`, so a stale wasm there would silently test the previous engine.
#
# NB: no --single-process here. The clone renders through the Web Worker pool
# and single-process Chromium does not give module workers a reliable home.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
native="$here/../native"
clone="$here/../ui-mock/react-dock"
export PATH="$HOME/.cargo/bin:$PATH"

if [ ! -d "$clone" ]; then
  echo "the OpenUtau clone is missing at $clone" >&2
  exit 1
fi

echo "==> building engine-wasm (release, wasm SIMD)"
( cd "$native" && RUSTFLAGS="-C target-feature=+simd128" \
    cargo build --release --target wasm32-unknown-unknown -p engine-wasm )

echo "==> generating JS bindings"
rm -rf "$here/pkg"
mkdir -p "$here/pkg"
wasm-bindgen --target web --out-dir "$here/pkg" \
  "$native/target/wasm32-unknown-unknown/release/engine_wasm.wasm"

out="$native/target/editor-result.txt"
rm -f "$out"
port=4188

echo "==> serving the clone (port $port)"
# The DEV server, deliberately, not `vite preview` of a production build. The
# render preview reads a real voicebank from `web/fixture/bank/`, which lives
# outside the clone and is not copied into `dist/`, so a built bundle has no
# samples and every render/audio check fails. Serving from source is also what
# makes this a useful regression harness: it exercises the same page a developer
# is editing. `server.fs.allow` in `vite.config.js` already permits the wasm and
# fixture reads.
vite_pid=""
cleanup() {
  if [ -n "$vite_pid" ]; then kill "$vite_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT

( cd "$clone" && npx vite --host 127.0.0.1 --port "$port" --strictPort ) \
  > "$out" 2>&1 &
vite_pid=$!

for _ in $(seq 1 60); do
  if curl -sf "http://127.0.0.1:$port/" >/dev/null 2>&1; then break; fi
  if ! kill -0 "$vite_pid" 2>/dev/null; then
    echo "dev server did not start" >&2; cat "$out" >&2; exit 1
  fi
  sleep 0.5
done

echo "==> driving the clone (drag-probe)"
# The Direct WASM backend, so the harness does not need synth-server running:
# the session, the render pool and OPFS all live in the browser here.
( cd "$clone" && node tools/drag-probe.mjs "http://127.0.0.1:$port/?backend=wasm" ) 2>&1 | tee "$out"
status="${PIPESTATUS[0]}"

if [ "$status" -eq 0 ]; then
  echo "==> OK"
else
  echo "==> FAILED" >&2
fi
exit "$status"