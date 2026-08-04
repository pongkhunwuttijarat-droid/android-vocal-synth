#!/usr/bin/env bash
# Build libworldline.so for linux x86_64 (Plugin track P.3).
# Usage: ./worldline-linux.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NATIVE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build-linux"

cmake -S "${NATIVE_DIR}" -B "${BUILD_DIR}" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER="${CC:-cc}" \
  -DCMAKE_CXX_COMPILER="${CXX:-c++}"

cmake --build "${BUILD_DIR}" -j"$(nproc)"

SO="$(find "${BUILD_DIR}" -name 'libworldline.so' -type f | head -1)"
if [[ -z "${SO}" ]]; then
  echo "ERROR: libworldline.so not produced" >&2
  exit 1
fi
echo
echo "OK: ${SO}"
file "${SO}"
