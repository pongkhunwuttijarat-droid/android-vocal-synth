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

# 3. Demo project
mkdir -p "$ASSETS"
cp "$NATIVE/test-data/demo-song.ustx" "$ASSETS/demo-song.ustx"
echo "  ✓ demo-song.ustx"

# 4. Voicebank subset covering ALL phonemes the editor demo uses
#    (Lead Vocal: hi mi ooh ah ding; Senbonzakura ja→en translit adds
#    b O z k u r j w g). Single-phoneme aliases must exist for:
#    m i d n aI t D 3 s l g oU u A N b O z k r j w.
VB="$ROOT/test/golden/teto-english/library"
VBDST="$ASSETS/voicebanks/teto-english"
mkdir -p "$VBDST/voice"
cp "$VB/character.txt" "$VBDST/character.txt"
cp "$VB/voice/oto.ini" "$VBDST/voice/oto.ini"
for w in \
    _3_h3_3-.wav \
    _a+_ha+_a+_a+_a+-.wav \
    _ai+_hai+_ai+-.wav \
    _b3_b3_b-.wav \
    _d3_d3_d-.wav \
    _d+3_d+3_d+-.wav \
    _de+_de+_d-.wav \
    _e+_he+_e+_e+_e+-.wav \
    _f3_f3_f-.wav \
    _g3_g3_g-.wav \
    _ha+_h3_ha+_3_a+_nz-.wav \
    _i_hi_i_i_i-.wav \
    _j3_j3_j-.wav \
    _k3_k3_k-.wav \
    _l3_l3_l-.wav \
    _li_li_l-.wav \
    _m3_m3_m-.wav \
    _n+3_n+3_n+-.wav \
    _n3_n3_n-.wav \
    _o+_ho+_o+_o+_o+-.wav \
    _ou+_hou+_ou+-.wav \
    _p3_p3_p-.wav \
    _r3_r3_r-.wav \
    _s3_s3_s-.wav \
    _t3_t3_t-.wav \
    _u_hu_u_u_u-.wav \
    _v3_v3_v-.wav \
    _w3_w3_w-.wav \
    _z3_z3_z-.wav \
    ; do
  cp "$VB/voice/$w" "$VBDST/voice/$w"
  # OpenUtau .frq (per-frame f0) — the C++ FrqEstimator uses it so
  # plosive bursts (unvoiced under pyin) keep their real f0.
  if [[ -f "$VB/voice/${w%.wav}_wav.frq" ]]; then
    cp "$VB/voice/${w%.wav}_wav.frq" "$VBDST/voice/"
  fi
done
echo "  ✓ voicebank (character.txt + oto.ini + 29 wavs + frq covering all editor phonemes)"

echo "== done — app is ready for flutter build/run =="
