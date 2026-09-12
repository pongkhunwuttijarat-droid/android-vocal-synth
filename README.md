# Lilt / Android Voice Synth

> Singing-synthesis engine and pen-first editor, derived from the OpenUtau
> architecture and targeting native/Android as well as a static WASM web app.

---

## Overview

The repository now contains three connected product paths:

- a Rust engine/runtime with pluggable renderers and OpenUtau-compatible project
  and voicebank handling;
- a Flutter tablet client that talks to the native synth server; and
- a static browser target whose pure-Rust WORLDLINE-R engine runs in-process as
  WebAssembly, without a server or native `.so`.

The active development branch is currently moving the proven native engine into
the WASM editor while keeping the native render paths available for neural engines.

---

## Documentation

- [Documentation Index](docs/README.md) — Complete documentation
- [Architecture Overview](docs/architecture/README.md) — System design
- [Engine](docs/architecture/engine.md) — Core domain logic
- [Renderer](docs/architecture/renderer.md) — Synthesis backends
- [Runtime](docs/architecture/runtime.md) — Orchestration
- [Data Contracts](docs/architecture/data-contracts.md) — Data structures
- [Current Roadmap](docs/planning/roadmap/ROADMAP.md) — Milestones and current status
- [WASM Migration Status](docs/planning/wasm-migration-status.md) — Browser migration checkpoint
- [Web UI Design](docs/planning/web-ui-design.md) — Editor implementation status and remaining work
- [Disk Hygiene](docs/DISK-HYGIENE.md) — คุมขนาด build cache / ล้างพื้นที่ดิสก์

---

## Current architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  CLIENTS                                                         │
│  ├─ Flutter tablet app ──LAN/HTTP──┐                             │
│  └─ Static HTML/Canvas editor ─────┼─ in-process WASM            │
└──────────────────────────┬───────────────────────────────────────┘
                           │
┌──────────────────────────▼───────────────────────────────────────┐
│  ENGINE (Domain)                                                 │
│  ├─ Project Model (state, commands, undo/redo)                   │
│  ├─ Phonemizer (lyric → phoneme)                                 │
│  └─ Feed (phoneme → render input)                                │
└──────────────────────────┬───────────────────────────────────────┘
                           │
┌──────────────────────────▼───────────────────────────────────────┐
│  RENDERERS (Pluggable)                                           │
│  ├─ Voicebank (oto, wav, config)                                 │
│  ├─ Native: WORLDLINE R1/R2, DiffSinger, MoE                     │
│  ├─ Pure Rust/WASM: WORLDLINE-R + Classic resampler              │
│  └─ Audio Output                                                 │
└──────────────────────────┬───────────────────────────────────────┘
                           │
┌──────────────────────────▼───────────────────────────────────────┐
│  RUNTIME (Orchestration)                                         │
│  ├─ Scheduler (job queue, cancel, progress)                      │
│  ├─ Chunker (phones → batches)                                   │
│  ├─ Cache (hash-based)                                           │
│  ├─ Worker Pool (parallel render)                                │
│  └─ Mixer (combine audio)                                        │
└──────────────────────────┬───────────────────────────────────────┘
                           │
┌──────────────────────────▼───────────────────────────────────────┐
│  HOST OUTPUT                                                     │
│  ├─ Native synth-server / Flutter playback                       │
│  └─ Browser Web Audio / WAV export                               │
└──────────────────────────────────────────────────────────────────┘
```

## Progress checkpoint (2026-09-12)

- MS1 engine core is complete.
- MS2 native WORLDLINE integration, CLI/server, Android device render, and
  golden comparison are complete at the current acceptance level.
- MS3 is in progress: R1/R2 and the DiffSinger tensor-to-WAV path work; classic
  Rust parity is proven; Vogen and full cross-renderer/device acceptance remain.
- MS4 is in progress on two fronts: the Flutter tablet client is functional, and
  the browser editor now has a tested vertical slice for note editing, undo/redo,
  rendering, playback, and viewport interaction.
- The WASM engine and Web Worker render pool pass parity/incremental tests. The
  browser editor still renders on the main thread; wiring the pool, OPFS project
  flow, multi-track mix, expression lanes, PWA/offline packaging, and neural WASM
  engines remain.

See the [roadmap](docs/planning/roadmap/ROADMAP.md) for the evidence and blockers
behind these labels.

---

## Reference

OpenUtau reference implementation located in `ref(openutau+openutau mobile)/`:

- `core/OpenUtau.Core/` — C# core engine
- `desktop-ref/cpp/worldline/` — WORLD vocoder C++ source
- `mobile/OpenUtauMobile/` — Mobile UI (MAUI)

---

## Render engine

The original native renderer uses the
[WORLD vocoder](https://github.com/mmorise/World) through the OpenUtau worldline
code. A pure-Rust port now provides the browser-compatible `r1-rs` path while the
native `.so` path remains available for parity and neural renderers.

### Key Files
- `worldline.h/cpp` — C API
- `phrase_synth.h/cpp` — Phrase synthesis
- `synth_request.h` — Request structure
- `model/` — WORLD model + effects
- `f0/` — F0 estimation
- `classic/` — Classic resampler

### External Dependencies
- WORLD vocoder (mmorise/World)
- libpyin (pYIN F0 estimator)
- libgvps (graph-based VPS)
- spline (spline interpolation)
- libnpy (numpy-like C++)
- xxhash (hash function)

---

## Build and test entry points

- Rust workspace: `cd native && cargo test`
- WASM engine: `cd native && cargo build --release --target wasm32-unknown-unknown -p engine-wasm`
- Browser smoke test: `./web/run-smoke.sh`
- Browser render-pool test: `./web/run-pool.sh`
- Flutter client: `cd app && flutter test`

Native Android builds additionally require the Android NDK/CMake; browser builds
require `wasm-bindgen` and a Chromium-compatible browser for the headless scripts.

---

## License

Based on OpenUtau (MIT License)
