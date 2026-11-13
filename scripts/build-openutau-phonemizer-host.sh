#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
project="$root/native/tools/openutau-phonemizer-host/OpenUtauPhonemizerHost.csproj"

dotnet_cmd="${RUSTED_UTAU_DOTNET:-dotnet}"
"$dotnet_cmd" build "$project" --configuration Release

printf 'Set this before starting synth-server:\n'
printf 'export RUSTED_UTAU_OPENUTAU_PHONEMIZER_HOST="%s"\n' \
  "$root/native/tools/openutau-phonemizer-host/bin/Release/net8.0/OpenUtauPhonemizerHost.dll"
