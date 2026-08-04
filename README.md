# Android Voice Synth

> Singing synthesis engine for Android based on OpenUtau

---

## Overview

This project implements a singing synthesis engine for Android, based on the OpenUtau reference implementation. It uses the WORLD vocoder for audio synthesis and supports OpenUtau voicebank format.

---

## Documentation

- [Documentation Index](docs/README.md) — Complete documentation
- [Architecture Overview](docs/architecture/README.md) — System design
- [Engine](docs/architecture/engine.md) — Core domain logic
- [Renderer](docs/architecture/renderer.md) — Synthesis backends
- [Runtime](docs/architecture/runtime.md) — Orchestration
- [Data Contracts](docs/architecture/data-contracts.md) — Data structures

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  UI LAYER (Flutter)                                              │
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
│  RENDERER (Pluggable)                                            │
│  ├─ Voicebank (oto, wav, config)                                 │
│  ├─ Synthesis Backend (WORLD, DiffSinger, Classic)               │
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
│  AUDIO SERVICE                                                   │
│  ├─ Playback (AudioTrack/AAudio)                                 │
│  └─ Export (WAV writer)                                          │
└──────────────────────────────────────────────────────────────────┘
```

---

## Reference

OpenUtau reference implementation located in `ref(openutau+openutau mobile)/`:

- `core/OpenUtau.Core/` — C# core engine
- `desktop-ref/cpp/worldline/` — WORLD vocoder C++ source
- `mobile/OpenUtauMobile/` — Mobile UI (MAUI)

---

## WORLD Renderer

The WORLD renderer uses the WORLD vocoder (https://github.com/mmorise/World) for audio synthesis. Source files located in `ref(openutau+openutau mobile)/desktop-ref/cpp/worldline/`.

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

## Build

### Prerequisites
- Android NDK
- CMake 3.18+
- Kotlin
- Flutter

### Build Steps
1. Build WORLD vocoder C++ library
2. Build JNI bridge
3. Build Kotlin engine
4. Build Flutter UI

---

## License

Based on OpenUtau (MIT License)
