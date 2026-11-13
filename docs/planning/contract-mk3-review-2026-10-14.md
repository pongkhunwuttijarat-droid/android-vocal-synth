# MK3 contract review — 2026-10-14

**Reviewer:** Codex  
**Scope:** requirement coverage and implementability of [MK3 draft](contract-mk3-proposal.md), compared with the user's latest goals, frozen logical v2 semantics, existing USTX/mixdown source, and the pinned SoundCraft source audit.  
**Verdict:** direction is covered, but important product requirements are incomplete and the protocol is not implementation-ready. Do not treat this review as contract approval or implementation authorization.

## Requirement coverage

| User goal | Coverage in the draft | Remaining gap |
|---|---|---|
| Keep Rusted UTAU's synthesis engine and Rust project authority | Covered (§2) | Preserve mixed voice/wave track behavior explicitly; see F5. |
| Improve mixer using SoundCraft as reference | Partial (§6) | No concrete mixer/FX model, commands or ordered graph; see F6. |
| Assess SoundCraft FX crate for reuse | Covered as candidate (§6.1) | Processor catalogue/parameter schema and adapter audit remain necessary. |
| Keep USTX track FX separate from new mixer FX | Covered (§6.1) | Chain placement, bypass, tails and save semantics are still unspecified. |
| Start playback before whole-project rendering completes | Covered as intent (§5) | Pending/readiness, flow control and transport state machine are incomplete; see F3. |
| Virtual instrument track with SoundCraft or RustySynth backend | Partial (§3) | SoundCraft is limited to hosted native plugins; built-in Rust instruments are omitted; see F1. |
| Web plus Android WebView/local-server WebSocket target | Covered as target (§4, §5.5) | Exact adapter/framing/capability contract and initial target acceptance are open. |
| Keep instrument data in `.rustx`; represent it as audio for USTX interoperability | Partial (§7, §9) | No bounce-to-wave/export contract or reconstruction/loss policy; see F2. |
| Portable single-file project, optional voicebank inclusion and exclude | Incomplete (§7) | Voicebank assets, per-asset inclusion/exclusion, and unresolved open behavior are absent; see F2. |
| Benefit from existing chunk rendering/cache | Partial (§5.3) | No synthesis-versus-mix invalidation boundary; see F7. |

SoundCraft UI styling/reference belongs to the UI roadmap. Its contract implications are queryable instrument/FX descriptors and Rust commands; the contract need not prescribe visual styling.

## Findings

### F1 — P1: SoundCraft backend excludes the built-in Rust instruments

**Draft:** §1/§3.2, especially lines 13, 63–68 and 86.

The only SoundCraft configuration requires a plugin format and installed plugin identity. This can express CLAP/VST3/AU instruments, but cannot express SoundCraft's built-in subtractive synth, drum synth or sampler. Those instruments are directly relevant to the user's discussion of existing SoundCraft instruments and Rust plugins, especially for Web/Android where native plugin support is conditional. As written, selecting SoundCraft could mean there is no usable instrument on the primary hosts despite the built-in DSP library being available for assessment.

**Evidence:** at audited SoundCraft commit `c0cb99340891d306e216582db465f3594b89dc1d`, `crates/dsp/src/plugins/mod.rs` registers `subtractive_synth`, `drum_synth`, and `sampler`; `soundcraft_dsp::create` and the `Plugin` trait expose construction and block processing. This source evidence does not prove Rusted UTAU host compatibility.

**Required revision:** keep the user's two engine choices, but distinguish SoundCraft built-in instrument configuration from SoundCraft hosted-plugin configuration. Built-ins need instrument ID, parameter state and sampler asset references; hosted plugins need format, component identity, parameters and opaque state. RustySynth keeps SoundFont/preset configuration. Report per-host availability for each kind, without substituting instruments automatically.

### F2 — P1: `.rustx` portability and USTX audio interoperability are deferred rather than specified

**Draft:** §7/§9, lines 198–216.

The user explicitly wants `.rustx` to retain the extended instrument data and bridge to USTX using audio tracks, with optional embedded voicebanks and a way to exclude them. The draft records backend references but offers no export/bounce contract, voicebank asset category, inclusion policy, or excluded-asset resolution. A project could therefore lose editable instrument state on export or fail to be self-contained even when the user requested a portable bundle.

**Evidence:** `docs/architecture/rustx-format.md` describes v1 as `manifest.json` + canonical `project.ustx` + media and explicitly excludes virtual instruments and voicebank packaging. A new extension cannot fit into v1 merely by adding an `instrument` JSON property.

**Required revision:** specify a versioned Rust-owned extension payload and its stable-ID linkage to USTX; the authoritative location of MIDI/backend/mixer state; exact export-to-USTX policy that renders instrument content into wave parts; frame/tick placement, tails and applied mixer/FX behavior to prevent double processing; loss warnings and editable-state retention. Define asset categories, per-asset include/exclude choices for voicebanks/SoundFonts/samples, integrity/size metadata, and open/relink behavior when excluded assets are missing. Plugin-state blobs and executable plugin binaries need distinct packaging rules. An explicit user-selected rendered-audio fallback may be supported; it must not silently replace the editable instrument model.

### F3 — P1: A bounded `next` has no response for audio that is not ready

**Draft:** §5.1–§5.2, lines 134–149.

`next` must return within a defined bound, but the response variants only describe data or completion/failure. If vocal synthesis is still running when the deadline expires, returning `end` is incorrect, returning zero-frame `data` is undefined, and waiting indefinitely violates the stated contract. WebSocket push versus pull, credits/acknowledgements, request correlation and duplicate requests are also unspecified, so two adapters could disagree or grow queues despite the bounded-buffer requirement.

**Required revision:** define `pending/not_ready` or an explicit asynchronous wait/ready operation, its retry/deadline behavior and cancellation while waiting. Specify stream states, terminal replay, `max_frames` validation, single-reader/concurrent requests, monotonic block/frame rules, binary/control correlation, and pull/credit ownership. Define disconnection cleanup and a new-generation restart policy. Pin the control/binary framing and capability fields before implementing the streaming adapter.

### F4 — P1: Instrument MIDI data and backend switching remain names without schemas

**Draft:** §3.1/§3.3, lines 55 and 95–102.

There is no exact MIDI note/event DTO, tick resolution, event ordering, range validation, parameter command, or snapshot/read schema. SoundFont `bank`, `program`, and `channel` have no ranges or numbering convention. The same part could render differently when events share a timestamp or cross a block edge. Selecting a new backend also lacks rules for retaining MIDI/part IDs, preserving the old backend's state for undo, and handling incompatible parameters or missing resources.

**Required revision:** specify event IDs, timebase, note duration/velocity/channel ranges, bank/program conventions, initial controller state and tie-breaking rules, including events at a block boundary. Define the initial required MIDI subset and defer other event types explicitly. Specify backend-switch atomicity, state retention, invalidation and undo. Provide typed descriptors and bounded reads for instruments/presets/parameters and canonical request/result/error examples. If live audition is part of the initial product scope, define transient note-on/off/all-notes-off commands separately from recorded project edits; otherwise defer it explicitly.

### F5 — P1: Exclusive source kinds risk changing existing mixed USTX tracks

**Draft:** §3.1, lines 32–55.

The proposed single `source_kind` enum implies separate vocal and audio tracks, without explaining what happens to an existing track containing both voice and wave parts. The current Rust mixdown intentionally renders voice and wave contributions for the same track and then processes its track FX. Restricting one track to one source kind could split it, apply its fader/FX twice, or reject a document that previously worked.

**Evidence:** `native/tools/synth-cli/src/pipeline.rs` lines 973–1021 renders voice and wave parts into one `track_bus`; `native/crates/domain/src/track.rs` has no exclusive vocal/audio track discriminator.

**Required revision:** explicitly preserve mixed legacy tracks. Model track processing role separately from part content, or define a compatibility representation that retains identity, placement and shared mixer/FX semantics. State the allowed part types for instrument tracks and define any explicit conversion operation. Add a mixed voice/wave regression vector.

### F6 — P2: Separate FX chains are correct, but the new mixer API is still missing

**Draft:** §6, lines 183–194.

The draft promises insert/send/master effects, ordering, parameters and automation, but provides no mixer entities, command DTOs, units/ranges, parameter descriptor versioning, routing/cycle rules or serialization. There is no fixed signal path for the legacy USTX FX and new inserts, so exporting or playing both chains can apply a fader or effect twice. Plugin delay compensation, bypass behavior and finite tail termination are also open.

**Required revision:** freeze a minimal FX/mixer schema with stable insert IDs, processor IDs/version, parameters, bypass, chain placement and Rust commands, coordinated with T018. Define the graph sequence and tail/latency policy for that increment. Bus/send/automation features may be deferred to named follow-ups, but the contract must distinguish required initial support from future scope instead of promising all of them without DTOs.

### F7 — P2: Unconditional revision invalidation loses the existing render/mix distinction

**Draft:** §5.4, line 164; compare existing logical v2 §6.4.3 and §6.13.

The draft cancels or suppresses the stream whenever the project changes. That includes non-audio edits such as renaming, and leaves no defined way to reuse synthesized chunks after changing fader/pan/FX. It also tightens existing v2 semantics, which permit explicitly labeled stale playback, while claiming those semantics remain unchanged. This is particularly relevant to the user's goal of improving the mixer around an already chunk-based engine.

**Required revision:** define document revision, render-affecting invalidation and source/FX/mix cache identities as distinct concepts. Specify which edits stop the base T033 stream, which require remix only, and which can reuse source PCM. If all edits stop playback initially, state that as an explicit scoped policy without requiring re-synthesis. Keep advanced continuous edits/reconciliation in T035 and reconcile the explicitly labeled stale-playback rule with the parent contract.

### F8 — P2: Persistent project state and runtime plugin state need a concrete bridge

**Draft:** §3.2/§5.1/§6.3, especially lines 90, 132 and 194.

The draft promises an immutable backend snapshot but does not define how live plugin editor parameter edits and opaque state are captured into Rust-owned history/save/render state. A save or offline render could read stale parameters, or two streams could share a mutable instrument instance and corrupt note/FX state. Loading a project with unavailable assets also needs to preserve editable configuration rather than failing the entire load merely because its renderer is unavailable.

**Required revision:** define the engine command/state-capture path for backend controls, runtime instances per stream, construction/destruction outside time-critical audio processing, failure behavior and state-restore rejection. Specify preserved-but-unavailable document state on open, and distinguish strict render failure from any explicit partial/frozen-audio mode. Define who owns opaque state and asset generations while async work is in flight.

### F9 — P2: T033's acceptance slice is conflated with the full MK3 product

**Draft:** §6.2/§10, lines 190 and 220–234; T033/T034/T035 task records.

The new draft includes seek/checkpointing, instrument support and packaging within one conformance/release list, but existing T033 is the first voice/wave streaming increment; T034 adds seek and T035 adds advanced edit scheduling. T033's task record also still names Direct WASM/HTTP and does not yet point directly to T045/the proposed Android WebSocket mapping. Without phase-specific required capabilities and vectors, the streaming increment could either expand unexpectedly or be advertised as satisfying the entire contract.

**Required revision:** define separate capability/conformance groups for base streaming, sequential FX, seek, instruments, advanced routing and portable packaging. Bind each task to its required group and host adapter matrix. Update T033's dependency reference only as a planning change when the accepted contract artifact and source owners are agreed; implementation remains gated.

## Minor editorial correction

- Draft line 25 links FX separation to §7, but FX separation is in §6.

## Recommended revision order

1. Correct product coverage: SoundCraft built-ins, mixed-track compatibility, `.rustx`/USTX bridge, portable assets/exclude policy.
2. Turn stream lifecycle and instrument/MIDI configuration into exact schemas, examples and state tables. Resolve pending/block delivery and declared limits first.
3. Specify the minimal FX/mixer state model and backend runtime/state-capture boundary.
4. Split required capability/conformance groups for T033 versus subsequent instrument/seek/routing/packaging work, then present the concrete version/behavior choices for review.

## Review evidence and limitations

- Checkout: branch `wasm-native-rust`, HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- The MK3 draft and work board/task records are untracked in this checkout. The draft was created during this conversation; unrelated dirty source remains unowned and was read only.
- Reviewed `docs/planning/contract-mk3-proposal.md`, `docs/CONTRACT-v1.md` §6.4/§6.13–§6.15, `docs/architecture/rustx-format.md`, T033/T034/T035/T044/T045, and current domain/pipeline source. Source inspection describes the visible checkout and does not certify uncommitted implementation.
- SoundCraft audit reference: commit `c0cb99340891d306e216582db465f3594b89dc1d`; [built-in registry](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/dsp/src/plugins/mod.rs) and [DSP interface](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/dsp/src/lib.rs).
- No build, runtime/backend compatibility test, or audio benchmark was run. This is a document/requirement review. The contract itself was not changed by this review.

## Revision dispositions — 2026-10-14T08:02:09Z

The findings above describe the original 253-line draft reviewed before revision; their line numbers are historical. The subsequent user-authorized revision addresses them at document level as follows. This disposition does not establish implementation, passing tests or contract acceptance.

| Finding | Revised draft coverage |
|---|---|
| F1 | §3: SoundCraft builtin/hosted variants plus RustySynth; queryable catalogue/parameters and host gates. |
| F2 | §11: proposed `.rustx` v2 layout/linkage, asset include/exclude, explicit relink and USTX compatible/track_bake/mix_bake modes. User selected no automatic fallback audio. |
| F3 | §6–§7: pending replies, closed reply enum, states/concurrency/EOF/cancel/close, bounded pull and binary WS envelope. |
| F4 | §4: initial MIDI DTOs/ranges/order, stable-ID commands, backend-switch retention/undo and optional audition scope. |
| F5 | §2: `ustx_compatible` role retains voice+wave on one track; part content and track processing role are distinct. |
| F6 | §9: minimal insert model/commands, user-selected pre-fader new FX then legacy USTX stage, latency/tails; routing/automation explicitly separate. |
| F7 | §8: user-selected stop/replay for audible edits; cosmetic continuity; source/FX/mix cache distinction; snapshot export and v2 stale workflow preserved. |
| F8 | §10: per-stream mutable runtimes, Rust editor capture/history, state rejection and unavailable-but-editable project semantics. |
| F9 | §13–§14: feature/task slices and C01–C25; T033 documentation now directly links T045 and the native WS target without expanding source implementation scope. |

The former FX section cross-reference was replaced during the restructuring. Proposed wire/version/storage choices remain subject to exact schema validation and final review. No new product preference is inferred from silence; the three product decisions were supplied explicitly by the user.

The subsequent [round-2 control audit](contract-mk3-audit-round2-2026-10-14.md) adds source-backed SoundCraft/RustySynth limitations, concrete control APIs and the user's later decisions for 16-channel SoundFont support and declared block timing. See that report for the current control-surface assessment; this first review remains historical evidence.
