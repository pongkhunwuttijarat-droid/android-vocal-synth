#!/usr/bin/env bash
# Build libworldline.so for Android arm64-v8a with the NDK (Plugin track P.3).
# Usage: ./worldline-android.sh [NDK_PATH]
# NDK_PATH defaults to $ANDROID_NDK_HOME or ~/Android/Sdk/ndk/<latest>.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NATIVE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build-android"

if [[ -n "${1:-}" ]]; then
  NDK="${1}"
elif [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
  NDK="${ANDROID_NDK_HOME}"
else
  NDK_ROOT="${HOME}/Android/Sdk/ndk"
  if [[ -d "${NDK_ROOT}" ]]; then
    NDK="$(ls -1d "${NDK_ROOT}"/*/ 2>/dev/null | sort -V | tail -1 | sed 's:/$::')"
  fi
fi

if [[ -z "${NDK:-}" || ! -f "${NDK}/build/cmake/android.toolchain.cmake" ]]; then
  echo "ERROR: NDK not found. Pass the NDK path as \$1 or set ANDROID_NDK_HOME." >&2
  exit 1
fi
echo "Using NDK: ${NDK}"

cmake -S "${NATIVE_DIR}" -B "${BUILD_DIR}" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_TOOLCHAIN_FILE="${NDK}/build/cmake/android.toolchain.cmake" \
  -DANDROID_ABI=arm64-v8a \
  -DANDROID_PLATFORM=android-24 \
  -DANDROID_STL=c++_shared \
  -DCMAKE_C_COMPILER_LAUNCHER= \
  -DCMAKE_CXX_COMPILER_LAUNCHER=

cmake --build "${BUILD_DIR}" -j"$(nproc)"

SO="$(find "${BUILD_DIR}" -name 'libworldline.so' -type f | head -1)"
if [[ -z "${SO}" ]]; then
  echo "ERROR: libworldline.so not produced" >&2
  exit 1
fi
echo
echo "OK: ${SO}"
file "${SO}"
