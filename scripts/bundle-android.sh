#!/bin/bash
# Bundle engine artifacts from native/ into the Flutter app (single source
# of truth = native/). Run BEFORE `flutter build` / `flutter run`.
#
# Copies:
#   native/target/aarch64-linux-android/release/synth-server
#     → app/android/app/src/main/jniLibs/arm64-v8a/libsynthserver.so
#   native/build/build-android/arm64-v8a/libworldline.so
#     → app/android/app/src/main/jniLibs/arm64-v8a/libworldline.so
#   native/test-data/demo-song.ustx + test/golden/teto-english/library/{character.txt,voice/*}
#     → app/android/app/src/main/assets/engine/
#
# Idempotent: safe to run repeatedly.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NATIVE="$ROOT/native"
APP="$ROOT/app"
JNILIBS="$APP/android/app/src/main/jniLibs/arm64-v8a"
ASSETS="$APP/android/app/src/main/assets/engine"

echo "== bundling engine artifacts =="

# 1. Rust server binary (must be built for android first)
SRV="$NATIVE/target/aarch64-linux-android/release/synth-server"
if [[ ! -f "$SRV" ]]; then
  echo "!! $SRV missing — building..."
  export PATH="$HOME/.cargo/bin:$PATH"
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=/home/seal/Android/Sdk/ndk/28.2.13676358/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang
  (cd "$NATIVE" && cargo build -p synth-server --target aarch64-linux-android --release)
fi
mkdir -p "$JNILIBS"
cp "$SRV" "$JNILIBS/libsynthserver.so"
echo "  ✓ libsynthserver.so ($(stat -c%s "$JNILIBS/libsynthserver.so") B)"

# 2. Renderer .so (arm64 build)
SO="$NATIVE/build/build-android/arm64-v8a/libworldline.so"
if [[ ! -f "$SO" ]]; then
  echo "!! $SO missing — run: $NATIVE/build/worldline-android.sh"
  exit 1
fi
cp "$SO" "$JNILIBS/libworldline.so"
echo "  ✓ libworldline.so ($(stat -c%s "$JNILIBS/libworldline.so") B)"

# 2b. Mixer FX plugin (arm64 build) — optional but bundled
MXSO="$NATIVE/build/build-android/arm64-v8a/libmixerfx.so"
if [[ ! -f "$MXSO" ]]; then
  echo "!! $MXSO missing — run: $NATIVE/build/mixerfx-android.sh"
  exit 1
fi
cp "$MXSO" "$JNILIBS/libmixerfx.so"
echo "  ✓ libmixerfx.so ($(stat -c%s "$JNILIBS/libmixerfx.so") B)"

# 3. Demo project
mkdir -p "$ASSETS"
cp "$NATIVE/test-data/demo-song.ustx" "$ASSETS/demo-song.ustx"
echo "  ✓ demo-song.ustx"

# 4. FULL voicebank (558 wavs + frq, ~93 MB) — user requested the full
#    model in the demo so ANY lyric/phoneme the editor produces resolves.
#    (The old 29-wav subset only covered the demo song's phonemes and
#    made non-demo lyrics fail with "partial oto mapping".)
VB="$ROOT/test/golden/teto-english/library"
VBDST="$ASSETS/voicebanks/teto-english"
mkdir -p "$VBDST/voice"
cp "$VB/character.txt" "$VBDST/character.txt"
cp "$VB/voice/oto.ini" "$VBDST/voice/oto.ini"
cp "$VB/voice/"*.wav "$VBDST/voice/"
cp "$VB/voice/"*_wav.frq "$VBDST/voice/" 2>/dev/null || true
N_WAV=$(ls "$VBDST/voice/"*.wav | wc -l)
N_FRQ=$(ls "$VBDST/voice/"*_wav.frq 2>/dev/null | wc -l)
echo "  ✓ voicebank FULL (character.txt + oto.ini + $N_WAV wavs + $N_FRQ frq)"
echo "  ✓ voicebank size: $(du -sh "$VBDST" | cut -f1)"

echo "== done — app is ready for flutter build/run =="
