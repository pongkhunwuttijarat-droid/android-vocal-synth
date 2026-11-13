# T015 — Engine-owned media (audio track) + whole-project mixdown + real project render

Status: active · Owner: Buffy · Last updated: 2026-10-04T09:22:00Z

## Goal

Three requests, in the order the user gave them:

1. Record "ONNX on wasm" as a **known limitation** instead of trying to enable it,
   and make the capability-reporting surface honest about it.
2. Continue the **audio track (wave part)** work: give it an engine-owned media
   path and a render path, so it is not a native-only feature.
3. Make the attached project actually render —
   `test-ustx-example/golden-the-end-of-hutsune-miku.ustx` with
   `test-ustx-example/voicebank/TETO-OUset240323.zip`.

## Repository checkpoint

- Branch `wasm-native-rust`, base commit `e77cc1a6`.
- The working tree had ~90 pre-existing modified/untracked entries from earlier
  sessions (T003/T004/T013/T014 work). **This task's own files are listed below;
  everything else is unowned by this task.**
- Files added/changed by this task:
  - `docs/architecture/known-limitations.md` (new)
  - `docs/architecture/README.md`, `docs/README.md` (index links)
  - `native/crates/engine-wasm/tests/host_capability_limits.rs` (new)
  - `native/crates/media/` (new crate: `Cargo.toml`, `src/lib.rs`, `tests/decode.rs`)
  - `native/Cargo.toml` (workspace member `crates/media`)
  - `native/Cargo.lock` (symphonia)
  - `native/crates/runtime/src/mixer.rs` (`openutau_fader_scale`, `TrackSpec::gain`)
  - `native/tools/synth-cli/Cargo.toml` (`media` dep)
  - `native/tools/synth-cli/src/pipeline.rs` (`render_mixdown`,
    `wave_part_mix_input`, `track_spec`)
  - `native/tools/synth-cli/src/main.rs` (`EngineFiles` extraction,
    `open_engine`, `mixdown` subcommand)
  - `native/tools/synth-cli/tests/mixdown_wave_parts.rs` (new)
  - `scripts/check_render_pitch.py` (new)
- **Not committed** — the user has asked repeatedly that nothing be committed.

## What was built

### 1. ONNX on wasm → known limitation (L1) + machine guard

- `docs/architecture/known-limitations.md` is a new **register**: each entry has
  the fact, the reason, and how to check it.
- L1 states the mechanism precisely: `synth-cli`'s `r2` feature pulls in
  `ort`/`diffsinger`/`moe-train`/`burn`, `engine-wasm` depends on `synth-cli`
  with `default-features = false`, so **no ONNX runtime is compiled into the
  wasm build**. Consequence: R2/DiffSinger/MoE cannot be rendered by the browser
  engine. The ORT-Web POCs under `web/` are JS-side inference — a POC, not a
  product path, because the engine-owned-state policy forbids a second
  authority in the frontend.
- Narrow claim pinned too: `Engine::render_r2_features_chunk` (Rust-side WORLD
  feature extraction for an ORT-Web worker) still exists.
- `native/crates/engine-wasm/tests/host_capability_limits.rs` (4 tests) reads the
  manifests at test time and fails if `ort`/friends appear in `engine-wasm`, if
  `synth-cli` is pulled in with default features, or if the `r2` feature stops
  being what gates ONNX (which would make this note stale).
- L2 records the capability-reporting gap: `v2::capabilities()` is a constant
  identical on every host and does **not** over-claim (it lists no renderers),
  but it cannot tell a UI what a host can render. Adding a field is a §6 contract
  change, so it is recorded as owed rather than done.

### 2. Audio track

- New crate `media` (`native/crates/media`):
  - `MediaSource` trait + `FileMediaSource` (OpenUtau's own combine-and-normalise
    resolution, plus a `restricted` constructor) and `MemoryMediaSource`.
  - `probe` / `decode_mono_44100` via `symphonia` (MP3/WAV/FLAC/OGG/pcm) —
    **pure Rust on purpose**, so the browser and native hosts decode identically.
  - `wave_part_duration_ticks` / `wave_trim_frames` / `apply_wave_trim`: a port of
    `UWavePart.UpdateDuration`, `GetSkipMs`, `GetTrimMs`, `TrimSamples`.
  - `resample_linear`, documented as linear (exact copy when rates match).
- `runtime::mixer::openutau_fader_scale` — a port of
  `PlaybackManager.DecibelToVolume` (knee below -16 dB, 0 at/below -24 dB), which
  is *not* a plain dB conversion.
- `TrackSpec` gained a linear `gain` multiplier. **Bug found by test:** passing
  the fader *scale* into `volume_db` made every track +1 dB loud
  (`db_to_linear(1.0) = 1.122`), which is how the first mixdown came out at
  1.12× the media.
- `pipeline::render_mixdown` — OpenUtau's *Export mixdown*: per-track voice
  render (reusing the existing per-track path unchanged), plus every wave part,
  each track through its fader/pan, summed. Solo is not applied, matching the
  reference. `synth-cli mixdown` exposes it.

### 3. The attached project renders

Voicebank extracted from the 474.5 MB zip into
`test-ustx-example/voicebank/extracted/` (gitignored).

| command | result |
|---|---|
| `synth-cli render --track 0` | 20 phrases, 0 skipped, 258.9 s, peak 0.9076, **byte-identical across two runs** (`sha256 91db8bd6…`) |
| `synth-cli mixdown` | 4 voice tracks = **153 phrases**, **1 wave part**, 288.4 s @ 44.1 kHz |
| `synth-cli mixdown --mixer libmixerfx.so --mixer-params '{"gain":0.5,"clip_enabled":1}'` | peak −2.16 dBFS, **0 clipped samples** |

Project shape discovered while doing this (the earlier summary in chat was
wrong): **4 voice parts over 4 tracks, 1709 notes total, and 1 wave part**
(`【ニコカラ】　初音ミクの消失　OFFVocal.mp3`, 288.4 s, whose `relative_path`
walks out of the project folder to `/home/<user>/…` and **does exist on this
machine**).

Measured with `scripts/check_render_pitch.py` (new, validated against synthetic
sines and harmonic-rich tones: within ~5 cents, no octave drop):

- track 0: 11 of 13 sampled onsets within ±8 cents.
- **But 7 consecutive notes at 219.76–223.1 s are off by +499 … −801 cents.**

## Open finding — RETRACTED and replaced (round 2, see below)

The round-1 note said the pitch anomaly "cannot be adjudicated here: this machine
has no `dotnet`". **That was wrong** — `~/dotnet` has SDK 8.0.425. Round 2 built
`test/golden/golden-renderer` against `ref-openutau-clean` and rendered a real
OpenUtau reference for every track.

## Round 2 — real OpenUtau reference + L8 adjudicated

Files added/changed in this round:

- `test/golden/golden-renderer/Program.cs` — four harness bugs fixed (below) and a
  per-track export mode (`RenderTracks`, the same call
  `PlaybackManager.RenderToFiles` uses)
- `scripts/compare_render_pitch.py` (new) — note-by-note agreement between two
  renders of the same part
- `scripts/compare_f0_curve.py` (new) — continuous f0-trajectory agreement over a
  time window

Harness bugs found (all pre-existing, all only reachable on multi-track projects):

1. Singer bound to `tracks[0]` only → other tracks got `Singer is not loaded` on
   every phoneme → 0 phrases → 0-frame per-track WAVs
2. only part0's phonemizer was awaited
3. `UPart.Validate` ran on two threads → `Collection was modified` crash; fixed
   with a single-concurrency `TaskScheduler` used as both `PostOnUIThread` and the
   phonemizer's main scheduler
4. the new per-track writer emitted stereo-interleaved floats into a mono header
   (2x length, octave low) → now folds with `ExportAdapter.ToMono(1, 0)` semantics

Adjudication: **the round-1 pitch finding was a measurement artefact.** Measuring
83 ms notes 50 ms after onset and comparing against the notated `tone` cannot
distinguish an engine bug from a glide. OpenUtau's own render sits −800…+640 cents
off `tone` in that same window. Median per-track disagreement with OpenUtau is
4.1–8.4 cents; the residual p90 (179–384 cents) is real but the measurement is too
unstable at note onsets to call it a bug yet.

Also: the user's attached `test-ustx-example/Export/*.wav` are genuine OpenUtau
per-track exports (durations match ours exactly); `_00.wav` (track 0) is 0 frames,
which headless rendering does not reproduce.

## Verification (all run from `native/` unless noted)

| command | outcome |
|---|---|
| `cargo test --workspace` (final rerun, after the last edit) | exactly **1** `test result: FAILED` — `project-session/tests/session_v2.rs::a_note_edit_by_id_reports_a_patch_and_invalidates_render`, **pre-existing**: `git status` reports that file unmodified against `e77cc1a6`, so this task did not touch it; every other target `ok` |
| `synth-cli mixdown` peak/clip re-measured from the written WAVs | plain: 288.4 s mono @44.1 kHz, 16 132 clipped samples (0.127 %), peak reaches full scale; with `libmixerfx.so`: peak −2.16 dBFS, **0** clipped |
| `cargo test -p media` | 14 pass (10 unit + 4 integration, 10.7 s — the integration target decodes the fixture's real 288 s MP3) |
| `cargo test -p runtime --lib` | 60 pass |
| `cargo test -p synth-cli --test mixdown_wave_parts` | 6 pass, and now **warning-free** (two `after_load()` results were being discarded; they are asserted) |
| `cargo build -p synth-cli --features cli` | clean (only pre-existing warnings in `diffsinger`/`moe-train`/`synth-cli`) |
| `cargo build -p engine-wasm --release` | clean (`media` is not a dependency of `engine-wasm`, so the wasm graph is unchanged) |
| `synth-cli render --track 0` after the `EngineFiles`/`open_engine` refactor | `sha256 91db8bd6…`, byte-identical to the pre-refactor output — the CLI refactor changed no audio |
| `synth-cli synth-note --phoneme="- A" --tone 60` | 22017 samples (499.3 ms), peak 0.5963 — the other subcommand still works |
| negative control: disable the wave branch in `render_mixdown` | 5 of 6 mixdown tests fail (the 6th, "no parts at all", correctly still passes), then restored |
| negative control: `cargo test -p engine-wasm --test host_capability_limits` | passes on the real manifests; the assertion is manifest-driven so it cannot pass vacuously |

## Next concrete step

1. Get an OpenUtau reference render of this project (needs `dotnet`, or a WAV
   exported from the desktop app) and re-run `scripts/check_render_pitch.py`
   against it. That decides whether L8 is an engine bug or correct behaviour.
2. If it is a bug, the shared suspect is the phoneme→f0 assignment around
   419760–424000 ticks; `phrase/` and `feed/render_input` are where to look.
3. Then close the remaining audio-track gaps in L3: an OPFS `MediaSource` for
   `engine-wasm`, `MediaSource` caching (a 288 s MP3 costs ~10 s to decode per
   mixdown), and waveform peaks for the editor.