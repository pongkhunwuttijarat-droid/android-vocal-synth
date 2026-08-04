#!/bin/bash
# Audio test harness — renders a battery of test sounds through the engine
# and validates each output (duration/peak/nonzero + audiocompare vs golden
# references when available).
#
# Usage: ./test-audio.sh [--verbose]
# Requires: cargo (PATH=$HOME/.cargo/bin:$PATH), built libworldline.so
#
# Outputs land in native/test-data/output/audio-test/ — listen to any of
# them; each file name says what it tests.

set -euo pipefail
cd "$(dirname "$0")/.."
NATIVE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

SO="${SO:-$NATIVE_DIR/build/build-linux/libworldline.so}"
TETO="$NATIVE_DIR/../test/golden/teto-english/library"
OUT="$NATIVE_DIR/test-data/output/audio-test"
mkdir -p "$OUT"

VERBOSE=0
[[ "${1:-}" == "--verbose" ]] && VERBOSE=1

PASS=0
FAIL=0

check_wav() {  # $1=file  $2=min_duration_ms  $3=label
  local f="$1" min="$2" label="$3"
  if [[ ! -s "$f" ]]; then
    echo "  ✗ $label: MISSING ($f)"; FAIL=$((FAIL+1)); return 1
  fi
  local stats
  stats=$(python3 - "$f" <<'PY'
import sys, wave, array
w = wave.open(sys.argv[1])
n, rate = w.getnframes(), w.getframerate()
s = array.array('h', w.readframes(n))
dur = n/rate*1000
peak = max(abs(v) for v in s)/32768 if s else 0
nz = sum(1 for v in s if abs(v) > 100)/len(s)*100 if s else 0
print(f"{dur:.0f}ms peak={peak:.3f} nonzero={nz:.0f}%")
PY
)
  local dur
  dur=$(echo "$stats" | grep -oE '^[0-9]+')
  if (( dur < min )); then
    echo "  ✗ $label: too short ($stats)"; FAIL=$((FAIL+1)); return 1
  fi
  echo "  ✓ $label: $stats"
  PASS=$((PASS+1))
}

render_note() {  # $1=phoneme $2=tone $3=dur_ms $4=out_name
  cargo run -q -p synth-cli -- synth-note \
    --voicebank "$TETO" --so "$SO" --phoneme "$1" --tone "$2" \
    --duration-ms "$3" --out "$OUT/$4.wav" >/dev/null 2>&1 || {
      echo "  ✗ render $4 failed"; FAIL=$((FAIL+1)); return 1; }
  return 0
}

echo "=== Audio test battery ($(date +%H:%M:%S)) ==="
echo "voicebank: $TETO"
echo "output:    $OUT"
echo ""

echo "--- 1. Single notes (phoneme A, different pitches) ---"
render_note A 60 500 "note-A-C4-500ms"
check_wav "$OUT/note-A-C4-500ms.wav" 400 "A C4 500ms"
render_note A 72 500 "note-A-C5-500ms"
check_wav "$OUT/note-A-C5-500ms.wav" 400 "A C5 500ms (octave up)"
render_note A 48 500 "note-A-C3-500ms"
check_wav "$OUT/note-A-C3-500ms.wav" 400 "A C3 500ms (octave down)"

echo "--- 2. Duration behavior ---"
render_note A 60 1000 "note-A-1000ms"
check_wav "$OUT/note-A-1000ms.wav" 900 "A 1000ms (double length)"
render_note A 60 200 "note-A-200ms"
check_wav "$OUT/note-A-200ms.wav" 150 "A 200ms (short note)"

echo "--- 3. Consonant aliases ---"
render_note "e d" 60 500 "note-ed-500ms"
check_wav "$OUT/note-ed-500ms.wav" 400 "e d (consonant pair)"
render_note "3 A" 60 500 "note-3A-500ms"
check_wav "$OUT/note-3A-500ms.wav" 400 "3 A (vowel pair)"

echo "--- 4. Full song (demo-song.ustx, 4 notes) ---"
cargo run -q -p synth-cli -- render \
  --project "$NATIVE_DIR/test-data/demo-song.ustx" \
  --voicebank "$TETO" --so "$SO" --out "$OUT/demo-song.wav" >/dev/null 2>&1 || {
    echo "  ✗ render demo-song failed"; FAIL=$((FAIL+1)); }
check_wav "$OUT/demo-song.wav" 2500 "demo-song (4 notes)"

echo "--- 5. Golden song (4x A, for audiocompare) ---"
cargo run -q -p synth-cli -- render \
  --project "$NATIVE_DIR/../test/golden/reference/golden-song.ustx" \
  --voicebank "$TETO" --so "$SO" --out "$OUT/golden-song-ours.wav" >/dev/null 2>&1 || {
    echo "  ✗ render golden-song failed"; FAIL=$((FAIL+1)); }
check_wav "$OUT/golden-song-ours.wav" 2500 "golden-song (4x A)"

echo ""
echo "=== Summary: $PASS passed, $FAIL failed ==="
exit $FAIL
