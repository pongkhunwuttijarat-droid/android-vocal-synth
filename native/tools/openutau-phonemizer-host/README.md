# OpenUtau phonemizer host

This .NET 8 helper runs the official OpenUtau Japanese Presamp plugin for the
native Rust render pipeline. Rust sends an immutable USTX snapshot and the
selected voicebank directory. The helper returns phoneme aliases, source note
indexes, plugin indexes, note-relative onsets, and each extender group's end
tick. Rust validates the result, applies OpenUtau's phoneme duration bounds,
then performs its timing/OTO pass before phrase construction. The helper never
saves a project or renders audio.

## Build and enable

From the repository root:

```sh
./scripts/build-openutau-phonemizer-host.sh
export RUSTED_UTAU_OPENUTAU_PHONEMIZER_HOST="$PWD/native/tools/openutau-phonemizer-host/bin/Release/net8.0/OpenUtauPhonemizerHost.dll"
```

Set `RUSTED_UTAU_DOTNET` if the .NET 8 executable is not named `dotnet` or is not on
`PATH`. Start `synth-server` from the same environment. If the host path or
runtime is unavailable, rendering a track that selects Japanese Presamp returns
an explicit error; it does not substitute the Rust Japanese VCV phonemizer.

The current bridge supports the official
`OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer` on native server/CLI builds.
Direct-WASM reports this plugin unavailable because a browser cannot launch a
.NET process. Android integration remains deferred.

## Limits

- The host accepts only protocol version 1 and the official Japanese Presamp
  plugin. It does not load arbitrary user DLLs.
- A note-level phonemizer override is not yet supported by this bridge.
- The project snapshot limit is 128 MiB and the returned phone limit is one
  million. The result is transient render input, never authoritative project
  state.
- OpenUtau's phonemizer executes once per rendered part in a short-lived .NET
  process. A persistent worker/cache is a later performance improvement.
