#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
T013_POC_URL="${T013_POC_URL:-http://127.0.0.1:5199}"

dotnet run \
  --project "$SCRIPT_DIR/Host/OpenUtauBrowserWasmHost.csproj" \
  --configuration Release \
  --no-launch-profile \
  --urls "$T013_POC_URL"
