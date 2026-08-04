#!/bin/bash
# Run the golden reference renderer (see build.sh first).
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOTNET="${DOTNET_ROOT:-$HOME/dotnet}/dotnet"
cd "$SCRIPT_DIR"
mkdir -p /tmp/golden-renderer-cache/OpenUtau /tmp/golden-renderer-data
exec "$DOTNET" run -c Release --no-build -- "$@"
