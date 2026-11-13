# T044 — Mixer streaming, FX, and SoundCraft reference roadmap

**Status:** blocked — crate audit, MK3 review, path-owner map, Linux baseline, and isolated browser DSP/WebAudio probe are recorded; D1/D2 product integration and app-level target-memory acceptance remain
**Owner:** unassigned — proposed Rust mixer / Project Session lane
**Last updated (UTC):** 2026-10-15T04:45:08Z

## Goal

Plan a Rust-owned mixer/audio playback improvement path for Rusted UTAU, using SoundCraft's mixer/playback crates as a reference implementation against Rusted UTAU's own Project Session. The user identifies `soundcraft-dsp` built-in FX as the strongest direct-reuse candidate; assess that crate first for the effect-processing layer. Keep Rusted UTAU's project model, USTX semantics, and `.rustx` format authoritative. The user has authorized implementation against the accepted MK3 baseline; this roadmap records that scope, while source ownership and target evidence still gate changes to shared files/dependencies.

## Proposed task breakdown

### D-1 — SoundCraft crate audit and reuse decision (first gate)

- Inspect the relevant upstream crates and actual call paths before changing Rusted UTAU: `soundcraft-mix`, `soundcraft-dsp`, `soundcraft-model`, `soundcraft-playback`, and plugin-host crates only where needed to understand dependencies.
- Record crate boundaries, public APIs, workspace/path dependencies, target-specific dependencies, unsafe/FFI boundaries, minimum Rust/toolchain needs, and MIT/Apache-2.0 licensing/attribution obligations. Confirm these details against the upstream revision used for the review.
- Trace block rendering, allocation behavior, persistent effect/plugin state, seek/reset/tail handling, routing/PDC, realtime/offline reuse, and how playback feeds the audio callback. Identify which behavior is implemented in the crate versus the SoundCraft host app.
- Prioritize `soundcraft-dsp`'s built-in effects as a direct-reuse candidate: map its effect registry/IDs and parameter metadata, `Plugin::prepare/reset/process`, planar channel buffers, state/tail/latency reporting, and target support onto Rusted UTAU's `mixer-fx` boundary and USTX/Rust project parameter semantics.
- Decide whether to depend on the full `soundcraft-dsp` crate or isolate an allowed effect subset only after checking its inherited workspace license, `serde`/`log`/`rustfft` dependencies, MSRV/edition, no-std/WASM/native/Android behavior, and contribution/attribution obligations. Avoid copying an incomplete slice that causes DSP divergence from upstream.
- Compare the crate's `soundcraft-model::Session` assumptions with Rusted UTAU's Rust Project Session and USTX semantics. Do not translate the whole project into a second authoritative model.
- Deliver a reuse matrix for each candidate: (a) depend on the crate directly, (b) reuse/adapt selected DSP algorithms or small abstractions, (c) reproduce the pattern in Rusted UTAU, or (d) reference only. Include compatibility risks for native, Web/WASM, and Android.
- Gate: this audit recommends `soundcraft-dsp` as the primary direct-reuse candidate for the new mixer-insert chain. The pinned `soundcraft-dsp`, `soundcraft-mix`, and `soundcraft-playback` crates now pass native/WASM compile checks, and the DSP crate's native tests pass; Rusted UTAU adapter, browser runtime, and memory evidence are still required before direct use. Do not adopt `soundcraft-mix`, `soundcraft-model` or `soundcraft-playback` as Rusted UTAU's project/session or playback authority. Preserve one Rust-owned project/session model.

### D0 — Ownership and baseline reconciliation

- Identify owners for relevant dirty/untracked mixer, pipeline, Project Session, and Web transport files before implementation.
- Reconcile this work with [T018](T018-openutau-flags-and-track-fx.md), [T032](T032-full-project-streaming-feasibility.md), [T033](T033-session-streaming-product-integration.md), [T034](T034-streaming-seek-fx-continuity.md), and [T035](T035-edit-during-playback-scheduling.md); avoid duplicating their scopes.
- Record reproducible current time-to-first-audio, total render time, peak memory, and PCM reference on a real multi-track project. Existing claims that synthesis is fast do not isolate mixer delay.

### D0.5 — Streaming contract review before T033

- Complete [T045](T045-streaming-contract-mk3-review.md) before approving or implementing T033. T045 covers virtual-instrument backend selection and the logical SoundCraft-hosted VSTi contract. SoundCraft host-library/adapter changes have their own follow-up in [T046](T046-soundcraft-hosted-vsti-adapter.md); do not mix them into base streaming or the FX crate reuse decision.
- T045 has reconciled T032's feasibility proposal with frozen CONTRACT-v1 §6 and accepted the MK3 logical implementation baseline. Do not edit the frozen contract in place; future compatible additions follow MK3 §2.1.
- The contract must explicitly cover revision-pinned streams, stable identity/generation, ordered bounded blocks, terminal/cancel/stale/error states, cancellation and closure, host capability differences, backpressure/underruns, and the FX state/tail continuity needed for streaming.
- T033 is authorized against MK3 and remains blocked on source ownership, T011 coordination, and measured native-server acceptance baselines. The proposed WebSocket route/schema is not live.

### D1 — Bounded block mixer and early playback

- Once T033's base streaming slice and its host-specific baselines are established, extend it with the bounded block mixer described here; no second Session/API approval is implied by this step.
- Keep the same deterministic Rust mixer/DSP graph for offline export and interactive playback. Preserve timeline order, sample accounting, cancellation, backpressure, underrun reporting, and stale-revision handling.
- Define block size, overlap/context ownership, and end-of-stream behavior explicitly; do not let overlapping synth chunks double-count samples in the mixer.
- Acceptance: first audio is playable before the entire project finishes; long projects remain bounded in memory; streamed PCM matches the deterministic offline reference within an explicitly approved tolerance; host-specific capability and timing evidence is recorded.

### D2 — Stateful per-track FX over blocks

- Coordinate with T018's active USTX FX work and T034's streaming seek/continuity scope before changing FX ownership.
- Persist processor state across blocks and define reset/seek/cancel behavior, latency, and component-specific tail policy. Legacy USTX `UMixFx` remains source-bounded in streamed and offline output; do not flush a post-source reverb tail. A new SoundCraft-derived insert may drain only its separately declared bounded tail. Avoid reconstructing compressors/reverbs per block.
- Preserve existing USTX FX order and post-fader behavior after parity review. Keep any new native mixer inserts distinct from legacy USTX `MixFx` until a migration rule is approved.
- Acceptance: block-boundary audio is continuous; legacy USTX output ends at the source boundary; any new insert tail follows its declared bound; seek/reset behavior is deterministic; and offline/interactive output agrees under the same component-specific policy.

### D3 — Mixer graph and product controls

- Add Rust-owned channel/track, bus, aux, send, and master graph only after D1/D2 and an approved model/command design.
- Separate pre-fader and post-fader sends; validate routing cycles; define mute/solo, pan law, meters, plugin latency compensation, and automation semantics.
- Prioritize built-in EQ, compressor, reverb, delay/chorus, and limiter based on product needs. SoundCraft's graph and controls are references; do not copy its session/model or assume its DSP ordering has USTX parity.
- Acceptance: stable IDs, atomic validated commands, grouped undo/redo, deterministic serialization, and model/command tests for routing and parameters.

### D4 — Shared Project Session, host adapters, and `.rustx`

- Route all persistent mixer changes through the shared Rust `project-session` semantics; frontends remain presentation/input clients for native, WASM, and future hosts.
- Keep USTX import/export compatibility and existing USTX track FX semantics. If mixer graph persistence needs new `.rustx` data, propose a versioned extension separately and define older-version behavior before changing the format.
- Do not bundle third-party VSTi, SoundFont banks, or other redistributable content in this mixer increment; format/content packaging is a separate decision.
- Acceptance: Rust-owned save/load round-trip and host parity; frontends issue commands and display engine snapshots rather than serializing a parallel mixer model.

### D5 — Verification and release gate

- Add Rust model/DSP tests for block equivalence, seam/tail behavior, FX state, routing validation, atomicity, undo/redo, serialization, and host parity as each increment is implemented.
- Add host playback checks for time-to-first-audio, bounded buffering, cancellation, underrun behavior, and stale project revisions using representative multi-track projects.
- Keep subjective audible review separate from meter, non-silent PCM, or successful decode evidence.

## Completion criteria

- [x] D-1 records the reviewed SoundCraft commit, relevant crate/API/dependency/license/platform findings, and a reasoned reuse-versus-reference decision before implementation.
- [x] D-1 includes an FX mapping/reuse proposal for `soundcraft-dsp` with concrete effect IDs, parameter mapping, buffer adapter needs, and per-host support evidence.
- [x] D0.5 completes T045's streaming contract review; the user accepted logical contract 3.0 as the implementation baseline and authorized T033. T033's source/measurement gates remain separate.
- [x] D0 records named owners for the relevant existing lanes/dependencies and the measured native streaming/offline baseline; Web/Android target acceptance is tracked separately in D5.
- [ ] D1 is approved and provides bounded Rust-owned block mixing with early playback and offline parity.
- [ ] D2 preserves USTX FX semantics and state/tails across blocks, coordinated with T018/T034.
- [ ] D3 provides an approved Rust mixer graph and controls without importing SoundCraft's project model.
- [ ] D4 keeps Project Session authoritative across hosts and documents any versioned `.rustx` changes.
- [ ] D5 records Rust and host evidence for the stated acceptance criteria.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit at planning: `e6af244abbf04d4a490fca8483295ed6418e6cad`
- Relevant working-tree changes: many files are modified/untracked and have unknown ownership. The read-only audit observed `native/tools/synth-cli/src/pipeline.rs`, `native/crates/runtime/src/mixer.rs`, untracked `native/crates/mixer-fx/src/openutau.rs`, `docs/architecture/rustx-format.md`, plus the existing untracked board/task records. Do not attribute or overwrite these changes until ownership is confirmed.
- Durable artifact: this plan and [work board](../BOARD.md).

## Progress

- 2026-10-14T06:13:33Z Read-only source/reference review recorded: current synth-cli mixdown materializes full track/project buffers; SoundCraft demonstrates block-based graph rendering with persistent processing state, while its session is independent. Current dirty `OpenUtauTrackFx` code is an unverified prototype and is not adopted by this plan.
- 2026-10-14T06:13:33Z Created this task breakdown only. No product source, API, schema, dependency, or project file was changed.
- 2026-10-14T06:16:30Z Started the requested crate-first audit against read-only SoundCraft checkout `c0cb99340891d306e216582db465f3594b89dc1d`: `soundcraft-mix` publicly exposes block rendering, but its manifest depends directly on SoundCraft's model, DSP, and CLAP/VST3/AU host crates. The DSP crate is a narrower boundary; direct reuse still requires compatibility review. `soundcraft-playback` feeds a 512-frame block through `MixEngine::render` and configures CPAL with a WASM target feature. This is preliminary evidence, not the final native/Android/WASM compatibility decision.
- 2026-10-14T06:20:17Z User set the preferred direction: use SoundCraft libraries as a reference implementation while adapting relevant mixer patterns to Rusted UTAU's Project Session. Direct reuse remains conditional on the crate audit.
- 2026-10-14T06:30:36Z User identified SoundCraft's FX crate as the likely direct-reuse boundary. The read-only manifest shows `soundcraft-dsp` is narrower than `soundcraft-mix` and depends on serde, log, and rustfft; it exposes a stateful `Plugin` processing contract, static effect metadata/IDs, and planar `f32` buffers. Integration feasibility and target compatibility remain to be evaluated.
- 2026-10-14T10:31:12Z T045 now has a hosted-VSTi feature/evidence matrix and C39–C46 vectors, including channel/offset events, program mapping, state/editor capture, IO/latency/tail preparation and realtime/offline agreement. Adapter/library implementation is tracked separately in T046; this does not change the SoundCraft FX reuse gate or authorize source edits.
- 2026-10-14T10:52:35Z Completed the read-only D-1 crate audit against pinned SoundCraft `c0cb99340891d306e216582db465f3594b89dc1d` (clean checkout). Recommendation: use `soundcraft-dsp` only as the processor implementation for new mixer inserts after target build/adapter evidence; keep existing USTX `UMixFx` and the Rusted UTAU Project Session intact. `soundcraft-mix` is useful reference for persistent block processing, routing/PDC and offline range semantics, but imports SoundCraft's model and all plugin host crates; do not adopt it as our mixer authority. `soundcraft-model` and `soundcraft-playback` likewise stay reference-only.
- 2026-10-15T01:07:45Z Reconciled D0 owners against current task records and the user handoff: T011 RenderPool is Codex-owned but excluded from T033; T015 mixdown/media paths are Buffy-owned; T014 Session contract paths are Codebuff-owned; T018 USTX FX paths/tests are Codex-owned; T033 native streaming and user-assigned Web playback paths are Codex-owned. Recorded Linux baseline: real 288.39 s project, 10.507 s first PCM, 17.757 s begin-to-end, +168,992 KiB sampled server RSS, exact PCM16 on the 131,072-frame prefix; offline render 15.45 s / +1,408,626,688 B RSS. This closes D0's owner-map and native-baseline criteria, not Web playback, Android, or target-device acceptance. Android work is deferred by the user.
- 2026-10-15T01:09:17Z T018's owned `OpenUtauTrackFx` now also matches OpenUtau Freeverb delay scaling/rounding across sample rates, with 8/8 crate tests. This is legacy USTX FX parity work; it does not implement or change the new SoundCraft-backed insert graph.
- 2026-10-15T01:56:38Z Reconciled D2's tail wording with the user's T034 decision: legacy USTX FX must preserve OpenUtau's source-bounded output, while SoundCraft-derived inserts may declare their own bounded tail. T018 now has full-buffer C# vs Rust comparisons for every EQ/compressor/reverb preset at 32/48/96 kHz (42 cases, 13.44M samples; max error `7.45e-8`). This improves legacy FX evidence but does not close D1/D2 streaming integration or host playback gates; no mixer graph or stream implementation was changed.
- 2026-10-15T01:59:47Z Revalidated target support in a fresh detached clean worktree at pinned SoundCraft commit `c0cb99340891d306e216582db465f3594b89dc1d`: `soundcraft-dsp` 0.6.1 cross-compiles to `wasm32-unknown-unknown`, and its native library tests pass 36/36. The worktree remained clean. This confirms the DSP crate-level WASM build, not browser runtime, WebAudio adaptation, memory bounds, or `soundcraft-mix`/`soundcraft-playback` integration.
- 2026-10-15T02:01:11Z Revalidated the complete SoundCraft mix/playback dependency path from that same clean pinned worktree: `soundcraft-mix` and `soundcraft-playback` both pass `cargo check --target wasm32-unknown-unknown`. This is compile-only evidence; it does not demonstrate Rusted UTAU adapter compatibility or real browser/device audio. No upstream files were changed.
- 2026-10-15T04:09:07Z Ran an isolated browser probe against pinned `soundcraft-dsp` 0.6.1 via `wasm32-unknown-unknown` + `wasm-bindgen`: `eq_7band`, `compressor`, and `room_reverb` each processed 1 second at 44.1 and 96 kHz with finite, non-silent output; 512- and 4096-frame block outputs matched exactly in this probe. A user-gesture WebAudio run resumed `AudioContext` to `running` and observed nonzero analyser output (peak 0.0814). Probe WASM memory was 1,114,112 B before processors and 3,211,264 B after all probes; prepared-plugin readings ranged 1,179,648–2,424,832 B. This is isolated crate/browser evidence only, not the Rusted UTAU adapter, worker/server stream, bounded app memory, or target-device acceptance. No repository product source or dependency changed.

### D-1 reuse matrix (source-reviewed; not a target build claim)

| Candidate | Source evidence at pinned revision | Decision for Rusted UTAU |
|---|---|---|
| `soundcraft-dsp` 0.6.1 | MIT OR Apache-2.0; edition 2024 / Rust 1.90; dependencies `serde`, `log`, `rustfft`; inherited `unsafe_code = forbid`. Public `plugins()`, `plugin_info()`, `create()`, `PluginInfo`/`ParamInfo`, and `Plugin::{prepare, reset, set_param, param, process, latency, tail_samples}`. Planar `Vec<Vec<f32>>`; contract says prepare sizes buffers and steady-state `process` does not allocate/panic. | Best direct-reuse boundary for new inserts. Pin exact revision/provenance; adapter pre-validates finite/range values because upstream clamps invalid values; translate stable descriptor IDs/units; allocate and prepare per-stream instances off audio thread. Do not replace USTX `MixFx` DSP or imply OpenUtau parity. Standalone native and WASM compile checks now pass; Android and Rusted UTAU integration remain unverified. |
| DSP FX mapping | `eq_7band` (23 controls incl. HPF/LPF, shelves, 3 parametric bands, input/output gain); `compressor` (threshold, ratio, attack/release, knee, makeup, sidechain HPF, limiter, lookahead, wet mix); `room_reverb` / `plate_reverb` (mix, pre-delay, decay, size, damping, diffusion, width, low cut). Additional IDs include `eq_1band`, `expander_gate`, `de_esser`, `maximizer`, `mod_delay`, `chorus`, `flanger`, `phaser`, `saturator`, `lofi`, `rectifier`, `pitch_shifter`, and utility processors. | Offer the mapped IDs as new mixer inserts, version descriptors independently, and use SoundCraft's canonical parameter metadata/defaults. Do not translate `UMixFx` three-band EQ/Freeverb/compressor presets into these processors; preserve that legacy chain unchanged. |
| `soundcraft-mix` 0.6.1 | Depends on SoundCraft time/audio-io/MIDI/DSP/CLAP/VST3/AU host/model crates plus `log`; non-WASM Rayon. `MixEngine::render(session, pos, frames, out)` processes statefully in blocks and tracks insert latency/PDC; `RangeRender` yields blocks but owns a SoundCraft `Session` and graph. | Reference block lifecycle, order, tails and latency compensation only. Not a base dependency: adopting its session would duplicate Rusted UTAU's authoritative model and adds plugin-host/platform coupling. |
| `soundcraft-model` 0.6.1 | Separate SoundCraft session/track/mixer/automation schema. MIT OR Apache-2.0. | Reference graph concepts only; never serialize it as Rusted UTAU project state. |
| `soundcraft-playback` 0.6.1 | CPAL device callback; depends on `soundcraft-mix`/model/DSP. Has target-specific CPAL WASM feature; documentation describes CoreAudio/WASAPI/ALSA/Pulse/PipeWire/JACK. | Reference callback/thread/lifecycle patterns only. No evidence from source inspection that its device bridge fits Android WebView + local server WS; do not adopt into this increment. |

### WASM capability clarification (2026-10-14)

At the pinned commit, `soundcraft-dsp`'s built-in plugin registry and DSP implementations are available to a WASM build. This includes built-in effects (EQ, dynamics, reverb, delay/modulation, harmonic/pitch and utility processors) plus its built-in subtractive synth, drum synth and sampler. `soundcraft-mix` compiles for WASM; its strip runner intentionally processes strips serially because its Rayon dependency is native-only. `soundcraft-audio-io` is bytes-in/bytes-out and also compiles in the mixer dependency graph. The VST3 and CLAP host crates compile as WASM stubs: scan results are empty and plugin creation returns unsupported. Their third-party plugin hosting does not transfer to the browser build.

`soundcraft-playback` selects CPAL's `wasm-bindgen` feature and its crate compiles for `wasm32-unknown-unknown`, which is evidence for the browser/WebAudio adapter path at compile time. No browser runtime/audio-output test was performed here. This does not make VST3/CLAP available in WASM and does not prove integration with Rusted UTAU's Web wrapper, worker, or local-server WebSocket architecture. For that architecture, use WASM DSP/mix as an option for built-in processing; keep third-party VST3 on a supported native server host and report direct Web/WASM hosting as unavailable.

**Adapter implications:** Rusted UTAU's current runtime mixer is mono/interleaved-oriented while `soundcraft-dsp` consumes planar buffers. The new mixer path needs stereo timeline buses and a bounded planar scratch adapter prepared once per render generation; converting buffers per sample or allocating per block would defeat the realtime contract. The legacy USTX chain stays on its existing implementation/order. `Plugin::note_on`/`note_off` lack a channel argument and `midi` takes raw MIDI bytes; this is not evidence of 16-channel instrument support and remains T046's host/backend concern.

**Target gate:** repository workspace is edition 2021 and contains no `soundcraft-dsp` dependency today; the inspected local compiler is Rust 1.98.1. The upstream crate requires Rust 1.90. The audit now has standalone native and WASM compile evidence, but CI/Android toolchains, runtime/audio behavior, and Rusted UTAU adapter compatibility still need separate checks before adding it. A direct dependency is the preferred first experiment only after the selected source files have a named owner and host builds are authorized.

## Verification

- `git status --short -- docs/work/BOARD.md docs/work/tasks` — inspected the working-tree state; board and task records are already untracked in this checkout, ownership unknown.
- Read-only inspection of `native/tools/synth-cli/src/pipeline.rs`, `native/crates/runtime/src/mixer.rs`, `native/crates/mixer-fx/src/openutau.rs`, `docs/architecture/rustx-format.md`, and SoundCraft's `crates/mix` / architecture docs — source review only; no build or tests run because this is a planning task.
- Read-only inspection of SoundCraft workspace/crate manifests, `soundcraft-mix::MixEngine::render`, `sync`, `render_range`, and the `soundcraft-dsp::Plugin` contract at `c0cb99340891d306e216582db465f3594b89dc1d` — preliminary crate audit only; no dependency was added and no crate was built.
- Read-only inspection of Rusted UTAU's `native/crates/mixer-fx` manifest/interfaces and SoundCraft DSP crate/plugin registry — identified an interleaved-vs-planar buffer adapter and parameter/model mapping as review points; no files under product source were changed.
- Read-only `git -C /tmp/soundcraft-readonly-audit rev-parse HEAD` / `status --short` — pinned revision `c0cb99340891d306e216582db465f3594b89dc1d`, clean source checkout. Inspected workspace/crate manifests, license/attribution, DSP interface/registry/parameter declarations, `MixEngine::render`/`RangeRender`, and CPAL target dependencies. Local Rust compiler reports `rustc 1.98.1`; this does not establish Android/CI MSRV or target support.
- `cargo check --manifest-path /tmp/soundcraft-readonly-audit/Cargo.toml -p soundcraft-dsp` — PASS (Linux `x86_64-unknown-linux-gnu`, soundcraft-dsp 0.6.1, pinned commit above).
- `cargo check --manifest-path /tmp/soundcraft-readonly-audit/Cargo.toml -p soundcraft-dsp --target wasm32-unknown-unknown` — PASS (same pinned crate/revision).
- `cargo check --manifest-path /tmp/soundcraft-readonly-audit/Cargo.toml -p soundcraft-mix --target wasm32-unknown-unknown` — PASS; dependency graph includes SoundCraft DSP, audio I/O and the VST3/CLAP/AU crate surfaces. Their WASM host surfaces do not instantiate third-party plugins.
- `cargo check --manifest-path /tmp/soundcraft-readonly-audit/Cargo.toml -p soundcraft-playback --target wasm32-unknown-unknown` — PASS with CPAL `wasm-bindgen`/WebAudio dependencies. Compile evidence only; no browser runtime/audio-output test.
- Fresh detached worktree `/tmp/soundcraft-wasm-audit`, `HEAD c0cb99340891d306e216582db465f3594b89dc1d`: `cargo check --manifest-path /tmp/soundcraft-wasm-audit/Cargo.toml -p soundcraft-dsp --target wasm32-unknown-unknown` — PASS; `cargo test --manifest-path /tmp/soundcraft-wasm-audit/Cargo.toml -p soundcraft-dsp --lib` — 36 passed, 0 failed; worktree clean.
- `cargo check --manifest-path /tmp/soundcraft-wasm-audit/Cargo.toml -p soundcraft-mix --target wasm32-unknown-unknown` — PASS.
- `cargo check --manifest-path /tmp/soundcraft-wasm-audit/Cargo.toml -p soundcraft-playback --target wasm32-unknown-unknown` — PASS; compile only, not browser runtime/audio-output evidence.
- `git -C /tmp/soundcraft-wasm-audit status --short --branch` — clean detached checkout at the pinned revision after all checks.
- Isolated `/tmp/t044-soundcraft-web-probe` browser harness, built from pinned `soundcraft-dsp` 0.6.1 for `wasm32-unknown-unknown` and bound with `wasm-bindgen`: six DSP/rate cases at 44.1/96 kHz all PASS (finite/non-silent, 512 vs 4096 frames max difference 0); WebAudio user-gesture playback PASS (`AudioContext=running`, analyser peak 0.0814069); browser console 0 errors / 0 warnings. Peak observed linear memory was 3,211,264 B across this small probe. This does not include the application, session, sample assets, stream buffers, or target-device runtime.
- `rustup target list --installed` — only `x86_64-unknown-linux-gnu` and `wasm32-unknown-unknown` are installed; Android target compatibility remains unverified. The upstream checkout remained clean after both checks.

## Blockers and dependencies

- T033's logical 3.0 Session/API baseline is accepted and implementation authorized. Its native-server/WebSocket lane does not depend on T024; T024 gates only Direct-WASM. Native baseline and T033-specific source owners are recorded. Stream capability remains unadvertised and browser playback has not been accepted because the product wrapper bridge is unavailable; Android wrapper work is deferred by the user.
- Coordinate D2 with active T018 and blocked T034; do not duplicate USTX FX changes or claim parity without evidence.
- Existing relevant lanes now have named task owners as listed above, but no extra ownership of their shared files is implied. T044's future mixer-insert implementation and shared Cargo/Session/model changes remain unassigned and need a bounded source handoff before editing.
- `soundcraft-dsp`, `soundcraft-mix`, and `soundcraft-playback` 0.6.1 pass standalone native or WASM crate checks as recorded above. The isolated built-in DSP probe also produces WebAudio output in a browser; VST3/CLAP remain WASM stubs. This does not validate Rusted UTAU's adapter, project stream integration, full application memory, or target-device behavior. Android work is deferred by the user and is outside this task's current target.
- Mixer model/command and `.rustx` changes require their own review; this plan does not approve schema or public API changes.

## Next step

T045 and D0 are complete. The pinned SoundCraft crates compile for the checked native/WASM targets, and an isolated browser probe confirms built-in DSP plus WebAudio output. Next, keep T033's capability gated until the Rusted UTAU Web runtime consumes its stream, then obtain integrated playback, bounded-memory, and underrun evidence. D1's bounded-mixer changes and a new-insert adapter still need their own source-owner handoff; no T044 implementation should reuse T015/T011/T018 files without coordination. Android remains deferred.


## Workspace fixture inventory (2026-10-14T14:49:45Z)

- The user pointed out that required fixtures are already in this workspace, especially `test-ustx-example/`. Confirmed real projects include `test-ustx-example/golden-the-end-of-hutsune-miku.ustx` (668,952 bytes), `test-ustx-example/king.ustx` (397,885 bytes), `test-ustx-example/envybaby.ustx` (531,624 bytes), and `test-ustx-example/test1.ustx` (18,874 bytes). The directory also contains `test-ustx-example/voicebank/extracted/` and existing golden exports under `test-ustx-example/Export/`.
- Existing `test-ustx-example/Export/golden-the-end-of-hutsune-miku.wav` is stereo, 44.1 kHz, 16-bit PCM, 12,716,928 frames / 288.366 seconds (50,867,758 bytes). Per-track files `golden-the-end-of-hutsune-miku_Track1_01.wav`, `_02.wav`, `_03.wav` are also present; `_00.wav` exists but is only 46 bytes. These are available PCM reference assets, not proof that a live streaming path meets time-to-first-audio or memory targets.
- The original USTX's wave-part `relative_path` points outside the project to a user-home download path, and the matching audio is not inside `test-ustx-example/`. The user-supplied workspace bundle [`miku-no-shoushitsu.rustx`](../../../miku-no-shoushitsu.rustx) does contain the corresponding MP3 as `media/offvocal-344ce2fd/...mp3` (4,614,731 bytes); its manifest reports `unresolvedMedia: 0`. This supplies a reproducible media source without editing either fixture. The bundle's own `project.ustx` has singer `.` on vocal tracks, so the tested combination used the original test USTX plus Rust Session relinking to the bundle media asset and the installed extracted Teto bank.
- T033 increased its maximum PCM block to 4,096 frames and aggregates queued renderer blocks within a bounded 64 KiB WebSocket message; the real 288.39-second project then reached `end` with 12,718,080 contiguous frames over 3,105 requests. First PCM arrived at 10.507 s and begin-to-end took 17.757 s. Sampled server RSS was +168,992 KiB over the imported-session baseline. A repeated 131,072-frame stereo prefix still matched offline PCM16 exactly (262,144 samples; max difference 0 LSB). The separate complete offline render took 15.45 s and peaked at +1,408,626,688 B RSS. These are server-side Linux probes; no browser AudioContext/Android output or target-device memory budget was exercised. A 4,096 unique-request connection ceiling still remains for consumers that choose smaller blocks and for streams longer than roughly 380 seconds. D0 owner-map/native-baseline criteria are recorded; host playback and full-device memory remain open.
- Do not modify the original USTX or `.rustx` bundle. Next: connect T033's dedicated audio consumer once the approved Web wrapper bridge exists, obtain host playback/memory evidence, and request a bounded owner for D1/D2 shared mixer changes before implementing SoundCraft DSP inserts.

## Handoff notes

SoundCraft is used as a design/reference implementation for block mixing and mixer graph concepts only. Rusted UTAU must keep its own Rust project/session model and USTX behavior. Current native mixdown has a whole-buffer path; the first product outcome should be early, bounded playback, then continuous stateful FX, then richer routing. Native SoundCraft plugin hosting and VSTi are outside this roadmap. `.rustx` mixer persistence is a future versioned decision, not an implicit extension to v1.

## Fresh dependency verification (2026-10-15T04:27:58Z)

- `cargo test --manifest-path native/Cargo.toml -p mixer-fx` — passed 9 unit + 7 integration tests. This covers the T018 crate-level FX vectors and block/snapshot behavior, not a whole-project voice+wave FX parity claim.
- `cargo test --manifest-path native/Cargo.toml -p synth-cli --test streaming_mixdown` — passed 6/6. It includes exact offline/stream PCM parity with contiguous blocks for one legacy-FX wave-track fixture, and a separate voice+wave streaming test proving early output and offline parity without claiming FX coverage for both track types.
- The build emitted existing unused/dead-code warnings in DiffSinger, moe-train, and synth-cli; no failures. These results strengthen the native prerequisites only. The T033 WebSocket credential/capability gate and T044 D1/D2 ownership/integration plus browser app memory acceptance remain open.

## Follow-up verification (2026-10-15T04:36:42Z)

- `cargo test --manifest-path native/Cargo.toml -p mixer-fx` — passed 9 unit + 8 integration tests after adding C# reference vectors at declared minimum/maximum `UMixFx` edit ranges. This validates the crate-level endpoints only; it does not rerun the separate `streaming_mixdown` suite or establish integrated voice+wave FX parity.

## Follow-up verification (2026-10-15T04:39:17Z)

- `cargo test --manifest-path native/Cargo.toml -p mixer-fx` — passed 9 unit + 9 integration tests after extending numeric min/max C# reference checkpoints to 32/48/96 kHz. At 32 kHz, the maximum-control C# chain becomes non-finite after the first selected sample and Rust reproduces it. This is compatibility evidence, not an endorsement of the resulting signal; no parameter-range policy was changed.

## Fresh stream/offline verification (2026-10-15T04:45:08Z)

- `cargo test --manifest-path native/Cargo.toml -p synth-cli --test streaming_mixdown` — passed 6/6. This freshly revalidates exact offline/stream PCM equality and contiguous 257-frame blocks for one legacy USTX FX wave-track fixture. The separate voice+wave streaming case proves early output/offline parity without FX coverage on both track types. Build emitted existing warnings in moe-train, DiffSinger and synth-cli; no failures. T033 credential/host playback, multi-track FX parity, SoundCraft integration and memory-budget gates remain.
