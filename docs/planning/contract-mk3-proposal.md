# CONTRACT-MK3 — Rusted UTAU Streaming, Instruments and Mixer (Draft)

**Status:** approved as the logical implementation baseline; implementation and runtime capabilities are not yet claimed. Base text is stable under §2.1; future compatible additions use named extensions.  
**Date:** 2026-10-14  
**Version proposal:** logical contract 3.0 as the canonical base for this feature set; draft descriptors use `3.0-draft`. Existing v1/v2 semantics remain available through their negotiated adapters. No existing frozen document is modified.  
**Review:** [findings and evidence](contract-mk3-review-2026-10-14.md).

## 1. Scope and recorded product decisions

Rusted UTAU keeps its vocal synthesis engine and Rust Project Session. MK3 specifies progressive project audio, virtual instruments, separate mixer effects, and portable project data. SoundCraft supplies reference patterns and candidate DSP/host libraries; it does not become the authoritative project model.

Decisions already supplied by the user:

- Virtual-instrument tracks choose SoundCraft or RustySynth. SoundCraft includes built-in instruments and separately available hosted plugins. RustySynth is the SoundFont/SF2 synthesizer backend.
- USTX track `MixFx` and new mixer FX remain distinct.
- Android targets a WebView UI and local Rust server with WebSocket. Direct Web/WASM remains a separate host adapter.
- For the first streaming increment, an edit that affects audio stops old playback; the user presses Play again. Cosmetic edits do not stop sound.
- New track mixer FX run before the existing fader/pan, followed by the legacy USTX FX stage.
- `.rustx` may embed permitted voicebanks/SoundFonts/samples, with per-asset exclude choices. Excluded or missing dependencies retain editable data and require relinking before rendering the affected audible content. No rendered fallback/preview audio is automatically stored for missing instruments or banks.
- `.rustx` retains editable instrument data; USTX interoperability represents instrument material as rendered audio parts.
- SoundFont tracks support independent preset/controller bindings across all 16 MIDI channels in their first release. A track initially references one SF2 asset; per-channel SF2 files are a separate future capability.
- RustySynth block-based event timing is acceptable for the first release when its resolution and effective scheduled frames are reported. Sample-accurate timing must not be claimed without a verified adapter/backend.
- Streaming seek uses versioned, session-local FX-state checkpoints. An open-ended seek beginning strictly beyond source end terminates with `end` and no PCM. The checkpoint cache is capped at 32 MiB per session, evicts the oldest checkpoint deterministically when full, and is cleared when the session closes.
- This contract is the implementation base. Add later capabilities and implementation requirements as named, versioned additive extensions; do not replace or rewrite the accepted base for small changes. A behavior-breaking change requires a new major contract and an explicit migration path.

Technical choices below are the accepted logical contract baseline, not implemented APIs. Exact Rust DTO conformance, host measurements and backend evidence remain engineering gates; an unverified backend/transport must not advertise support.

## 2. Authority, compatibility and identity

Rust owns canonical document contents, MIDI, backend/configuration/state, mixer/FX, validation, stable IDs, history, asset identities, serialization, render planning and cache invalidation. Frontends dispatch intents and display Rust views. Selection, layout, viewport and transient audio-device/playback buffers remain presentation/host state.

The frozen logical v2 rules apply to existing operations: `expected_revision`, atomic validation, grouped history, error envelopes, committed-state reads, lifecycle and stable identity. Render/save/export consume a committed Rust snapshot. Draft MK3 schemas are negotiated explicitly; new fields never change an old endpoint's semantics implicitly.

### 2.1 Base-contract evolution policy

The user has selected this MK3 document as the base for the streaming, instrument and mixer work. Once the base contract is accepted, its normative clauses are treated as a stable baseline:

- Add a later feature in a named extension or capability group with its own schema/capability version, scope, compatibility behavior, examples and conformance vectors. Compatible additions extend the base; they do not replace it.
- Do not rewrite or renumber the accepted base to absorb routine implementation discoveries or small feature additions. Put implementation notes and host-specific profiles in the relevant extension/task record.
- Correct a clear editorial defect with a dated erratum that preserves the original decision trail. If evidence shows a normative rule is wrong, document the evidence and affected vectors, then publish an explicit amendment/migration; do not silently change behavior under the same version.
- A breaking change requires a new major contract version, explicit migration/loss behavior and compatibility treatment for existing projects and hosts.
- Acceptance of this base, acceptance of each later extension, and authorization to implement them are separate gates. This policy does not freeze the current draft or authorize product/API changes by itself.

Track processing role and part content are distinct:

- `ustx_compatible` tracks retain existing voice parts, wave parts, or both on the same track. Import does not split, rename or duplicate the track or its mixer/FX.
- `instrument` tracks have one selected instrument configuration and permit instrument parts plus wave parts. They do not acquire vocal parts implicitly.
- Existing imported tracks default to `ustx_compatible`. Moving/converting incompatible parts requires an explicit validated command; selecting an instrument on a track containing voice parts returns `validation_error` and preserves the document.
- Track/part/note/event/insert IDs are opaque and minted by Rust. Reorder preserves IDs; duplicate/import creates new IDs under the parent lifecycle rules.
- Persistent entity IDs reuse the frozen v2 `EntityId` wire shape: lowercase hyphenated UUID strings, validated by Rust. Short labels such as `track-01`, `note-01` and `event-01` in examples are explanatory placeholders, not valid request values. Asset/component refs are separate typed identities and are not entity IDs.
- Snapshot schema 3 adds `track_role` and extension views for instruments, MIDI, mixer and assets. Existing USTX fields remain authoritative for existing voice/wave data; extension views do not duplicate that musical data.

## 3. Instrument configuration and descriptors

The following are complete backend variants for this draft. `parameters` contains values in the descriptor's physical units, not normalized slider positions. Descriptor/schema versions and defaults participate in render identity.

SoundCraft built-in:

```json
{
  "backend": "soundcraft",
  "instrument": {
    "kind": "builtin",
    "instrument_id": "subtractive_synth",
    "descriptor_version": 1,
    "parameters": {},
    "sample_ref": null
  }
}
```

Initial candidate built-in IDs are `subtractive_synth`, `drum_synth` and `sampler`. The sampler requires `sample_ref` to an engine asset. Other built-ins reject an unexpected sample reference. Empty parameters mean descriptor-version defaults; Rust materializes canonical values in its returned view.

SoundCraft hosted instrument:

```json
{
  "backend": "soundcraft",
  "instrument": {
    "kind": "hosted",
    "format": "vst3",
    "component_id": "vendor.unique.component",
    "version_requirement": null,
    "descriptor_version": 1,
    "parameters": {},
    "state_ref": "asset:plugin-state-01"
  }
}
```

Formats are `clap`, `vst3` or `au`, each independently capability-gated. A component ID identifies the format's component, not an OS pathname or display name. Descriptor IDs for external parameters are namespaced by component identity. A compatible plugin installation and platform binary are required; compiling Rust does not imply native plugin loading on Android/WASM.

RustySynth:

```json
{
  "backend": "rustysynth",
  "instrument": {
    "kind": "soundfont",
    "soundfont_ref": "asset:sf2-01",
    "channels": [
      {"channel":0,"enabled":true,"preset":{"bank":0,"program":0},"initial_cc":{},"initial_pitch_bend":8192},
      {"channel":9,"enabled":true,"preset":{"bank":128,"program":0},"initial_cc":{},"initial_pitch_bend":8192}
    ],
    "render_options": {"block_size_frames":64,"maximum_polyphony":64,"enable_reverb_and_chorus":true},
    "parameters": {}
  }
}
```

SF2 is the initial format. Channel is zero-based `0..15`; program is zero-based `0..127`; bank is an unsigned 16-bit catalogue value, validated against a preset actually present in the loaded bank and reachable by the adapter on that logical channel. UI labels may show channels/programs starting at 1; wire values remain zero-based. `channels` contains at most 16 distinct channel bindings; duplicate channels reject atomically. An omitted binding is unassigned, not an implicit General MIDI preset. The authoritative view always reports all 16 slots as assigned/unassigned, enabled/disabled and ready/unresolved. A MIDI note on an unassigned channel fails render preflight as `channel_unassigned`; an explicitly disabled assigned channel is intentional silence and is listed in the plan diagnostics.

Each channel's initial preset, CC map and bend state is independent. An explicitly selected preset is installed before that channel's first timeline event. Recorded CC/program events may change its effective sound later in the stream without rewriting the saved initial binding. One preset/channel does not overwrite another. `initial_cc` keys are decimal controller numbers without leading zeros, with values `0..127`; only the backend's declared supported CCs are accepted. Bank-select controllers are represented by the preset binding at initialization; duplicate conflicting bank/preset controls reject.

`render_options` is persisted source state because block size/polyphony/effects can affect sound. For audited RustySynth 1.3.6 the library accepts block sizes 8–1024, polyphony 8–256 and sample rates 16,000–192,000 Hz; host descriptors may narrow those ranges. Defaults shown above match that release, and descriptor versions pin defaults. `sample_rate` belongs to the render request, not a hidden per-track device choice. `parameters` exposes only descriptor-supported runtime controls such as backend master gain; internal source reverb/chorus and MIDI sends remain distinct from mixer FX. New runtime construction options are applied by rebuilding that generation, never by pretending they are live setters.

Descriptor/catalogue bounded reads return stable component/preset IDs, name, backend/kind/format, descriptor version, required asset kind, output layout, and per-parameter ID/type/unit/range/default/step/choices. Rust validates finite numbers, ranges and enum choices. Unsupported parameters reject edits; unavailable descriptors on load preserve data as unresolved rather than dropping fields.

Catalogue availability and document validity are separate. Projects with an unavailable plugin/bank open and remain editable. Rendering validates dependencies of the audible graph and returns diagnostics for unresolved contributors. Muted/unrouted content does not need to be loaded just to render other content. A missing backend is never silently substituted.

## 4. Instrument parts, MIDI and commands

### 4.1 Musical data

Ticks use the existing Rust project's ticks-per-quarter resolution (currently 480). Instrument parts store `part_id`, `track_id`, `position_tick`, `duration_tick`, `notes` and `events`. Part position is project-relative; note/event positions are part-local integers. Positions are nonnegative; duration is positive; note ends/events must lie within the part bounds. Checked arithmetic rejects overflow. Tempo/meter conversion to audio frames belongs to Rust.

Canonical note view:

```json
{"note_id":"note-01","position_tick":0,"duration_tick":480,"key":60,"velocity":100,"release_velocity":64,"channel":0,"order":0}
```

`key` is `0..127`, velocity `1..127`, release velocity `0..127`, and channel `0..15`. A note's end derives from its duration; a second stored note-off is not required. `order` is engine-owned stable ordering for otherwise simultaneous events.

Initial controller event variants:

```json
{"event_id":"event-01","position_tick":0,"channel":0,"order":0,"type":"cc","controller":64,"value":127}
```

```json
{"event_id":"event-02","position_tick":0,"channel":0,"order":1,"type":"program_change","program":0}
```

```json
{"event_id":"event-03","position_tick":240,"channel":0,"order":2,"type":"pitch_bend","value":8192}
```

CC/controller values are `0..127`; pitch bend is `0..16383` with center 8192. Initial event kinds are notes, CC, program change and pitch bend, gated by the selected instrument's exact supported-message descriptor. RustySynth multitimbral preset/controller state across 16 channels is required in its initial feature group; it is not implied for every SoundCraft builtin/plugin. Aftertouch, SysEx, MIDI 2.0, per-channel SoundFont assets and MPE are explicit future capabilities. Import reports unsupported events and does not silently discard them. The existence of a generic backend `midi` method is not evidence that it handles all CCs/messages.

Scheduling uses absolute project time. Rust first converts ticks to requested frames, then maps them through the selected backend's declared event timing. At one effective scheduled frame, generated note-offs precede controller/program/bend events in stored order, then note-ons in stored order; stable IDs break any remaining tie. Blocks are half-open `[start, end)`: an event at `end` belongs to the next block. Preroll/controller chasing is Rust-owned and not guessed by the UI. An event at part end may release an existing voice but cannot start a new note outside the part. Backend-specific note-off semantics are declared: a one-shot drum may ignore note duration, while a sustained instrument must release it. Unsupported release-velocity behavior is explicit in metadata rather than presented as an audible control.

`event_timing` reports `mode: sample_accurate|block_quantized`, `quantum_frames`, and `rounding: none|ceil`. For unmodified RustySynth 1.3.6, use block-quantized scheduling with the persisted internal block size. The grid is anchored to project frame zero: effective frame is `ceil(requested_frame / quantum_frames) * quantum_frames`; this bound applies to note-offs/CC/program events too. The adapter sends events before synthesizing their effective internal block and keeps a FIFO across arbitrary transport-block lengths. Changing PCM pull size cannot change event timing. Render plan/diagnostics report requested/effective frames and the resolution; offline and streaming use the same scheduling. A future sample-accurate adapter advertises a different verified timing profile and participates in render/cache identity.

For a duration-based note, its effective end is at least one backend quantum after its effective start: `effective_end = max(ceil(requested_end / Q) * Q, effective_start + Q)`. This explicit exception prevents a short note's quantized note-off from preceding its note-on at the same frame and leaving it held indefinitely. Any resulting release beyond the part boundary belongs to the source release tail and is reported in plan diagnostics. Raw CC/program events retain the ordinary ceil rule. Overlapping notes on the same key/channel follow the backend's declared MIDI key-release behavior; independent per-note voice addressing is not implied by document note IDs.

### 4.2 Persistent command surface

All persistent commands use the frozen parent envelope `{expected_revision, command, include_snapshot?, group_id?}` exactly once. `expected_revision` is an unsigned integer; `include_snapshot` defaults to false; `group_id` is an optional Rust-issued entity ID for an already-open grouped edit. The `command` is a closed `type`-tagged object with snake_case variant/field names; unknown fields and unsupported command types reject according to the frozen v2 rules. Draft additions:

| Command | Required command fields | Result/invalidation |
|---|---|---|
| `track.create` | `track_role`, `name`; instrument config for an instrument track | Mint track ID; return authoritative metadata. |
| `instrument.track.set_backend` | `track_id`, complete configuration, optional `allow_unresolved` (default false) | Retain track/part/MIDI IDs; invalidate its instrument source and downstream mix. |
| `instrument.track.set_parameters` | `track_id`, descriptor version, parameter map | Validate atomically; invalidate affected instrument source. |
| `instrument.part.create` | `track_id`, position/duration, note/event input | Mint part and new event/note IDs; return mappings for optional client refs. |
| `instrument.part.set_events` | `part_id`, bounded notes/events | Existing IDs must belong to that part; new items use client refs and Rust-minted IDs. One atomic replacement/history step. |
| `instrument.part.move_resize` | `part_id`, position/duration | Preserve IDs; reject changes that leave content outside bounds. |
| `instrument.part.remove` | `part_id` | Remove part; undo restores its identities and content. |

Commands return the existing mutation envelope with revision, authoritative `changes`, invalidation and optional snapshot. New change kinds are `instrument.changed`, `instrument_part.added/changed/removed` and `mixer.changed`; their payloads identify affected stable IDs and canonical views. Bounded reads expose parts/events/descriptors with the revision used for the read and a cursor; they do not construct a frontend-owned document.

For example, the command envelope is:

```json
{"expected_revision":12,"command":{"type":"instrument.track.set_backend","track_id":"track-01","allow_unresolved":false,"configuration":{"backend":"rustysynth","instrument":{"kind":"soundfont","soundfont_ref":"asset:sf2-01","channels":[{"channel":0,"enabled":true,"preset":{"bank":0,"program":0},"initial_cc":{},"initial_pitch_bend":8192}],"render_options":{"block_size_frames":64,"maximum_polyphony":64,"enable_reverb_and_chorus":true},"parameters":{}}}},"include_snapshot":false}
```

Backend changes preserve MIDI and retain the previous complete config/state in undo. Parameters/presets are not copied across incompatible backends. Moving a 16-channel SoundFont part to an instrument that cannot handle those channels/messages requires an explicit compatible channel mapping or returns `unsupported_midi_channel` with affected IDs; the engine never silently folds or drops channels. A rejected change leaves document/history/runtime playback configuration unchanged. A configuration that names unavailable resources may be stored explicitly as unresolved; it cannot be rendered until resolved. No-op commands increment neither revision nor history.

Live audition is a separate optional `instrument.audition.v1` capability: transient note-on/off/all-notes-off targets a Rust-owned audition instance and never increments project revision or records notes. Recording those gestures requires a separate persistent command. Live MIDI input/recording is not a T033 prerequisite.

### 4.3 Backend-control reads and authoritative track view

MK3 includes a logical control API for both SoundCraft and SoundFont tracks. These method names are draft transport-independent methods; they are not raw third-party library calls or claims that routes exist.

| Read method | Request fields | Authoritative result |
|---|---|---|
| `instrument.catalog.list` | Optional backend/kind/format filter, `limit`, `cursor` | `catalog_generation`, available component IDs/descriptors, preset/editor/control capabilities, and `next_cursor`. |
| `instrument.track.get` | `track_id` | Current revision, source enabled state, full committed configuration/parameters, parameter descriptor version, resource identities and runtime readiness diagnostics. |
| `instrument.parameters.list` | `track_id`, `limit`, `cursor` | Descriptor IDs/types/units/ranges/defaults/current values; writable flags; initialization-only versus runtime-settable controls. |
| `instrument.presets.list` | `track_id`, optional logical MIDI channel, `limit`, `cursor` | Presets compatible with that backend/component/asset/channel, generation, stable preset refs, and playability reasons. |
| `instrument.soundfont.channels.get` | `track_id` | All 16 initial bindings and currently committed controller defaults, plus per-channel unresolved/mapping status. Runtime effective program during playback is a separately labelled derived view. |
| `instrument.state.inspect` | `track_id` | Committed opaque-state reference/version/size/hash, pending editor-capture status; UI never authors or parses the blob. |

Session-bound reads carry `revision`; host catalogue reads carry `catalog_generation`. Pagination is bound to the relevant revision/generation and asset hash; stale cursors return `cursor_stale`, not a page from a different bank/plugin. Limits are declared/enforced. Library discovery, bank parsing and native scanning run off the audio thread and do not increment project revision. Track runtime states are `unresolved|loading|ready|failed`; they are engine-service diagnostics and not a second persisted project model.

Preset refs are typed: a SoundFont preset identifies asset ID/content hash/bank/program; a SoundCraft builtin preset is a versioned parameter/sample snapshot; a hosted preset additionally requires exact format/component identity and opaque state where necessary. A native plugin's program list is optional `backend_programs` support, not guaranteed by the SoundCraft `Plugin` trait. If enumeration is unavailable, return that capability explicitly; engine-managed snapshots and direct parameter controls can still be supported.

Example SoundFont preset item:

```json
{"preset_ref":{"kind":"soundfont","asset_id":"sf2-01","content_sha256":"0000000000000000000000000000000000000000000000000000000000000000","bank":128,"program":0},"name":"Standard Kit","channel":9,"playable":true,"reason":null}
```

The illustrative digest is replaced by the actual asset identity. Preset IDs/names are not array indices. Native plugin display names never replace component identity.

### 4.4 Persistent SoundCraft and SoundFont controls

These commands supplement §4.2 and use the same atomic revision/history/stop-on-audio-edit rules. `catalog_generation` or asset/descriptor identity is required when applying a browsed selection; resource races return `catalog_conflict`/`asset_unavailable` and do not partially switch the track.

| Command | Fields and applicability | Commit behavior |
|---|---|---|
| `instrument.track.set_enabled` | `track_id`, `enabled` | Enable/disable instrument contribution only; wave parts on the track and track mute remain distinct. Preserve MIDI/configuration. |
| `instrument.track.apply_preset` | `track_id`, typed SoundCraft `preset_ref`, `catalog_generation` | Atomically replace that component's parameter/sample/state snapshot; validate compatibility and retain full prior state for undo. SoundFont channel presets use the specific command below. |
| `instrument.track.reset_parameters` | `track_id`, descriptor version | Reset exposed control values to pinned descriptor defaults; retain component, MIDI, bank/preset/sample bindings. Opaque native factory reset requires a supported preset/state operation instead. |
| `instrument.soundcraft.set_sample` | `track_id`, sample asset ref, expected asset generation | Sampler only; preserve applicable controls, validate sample format/layout, replace source atomically and invalidate it. Reject on non-sampler instruments. |
| `instrument.track.load_state` | `track_id`, state asset ref, component identity, state schema | Hosted-state capability only; stage and restore in an isolated control instance, validate and capture canonical params, then commit. State refusal never silently falls back to defaults. |
| `instrument.soundfont.set_soundfont` | `track_id`, SF2 asset ref/hash, complete assigned-channel binding plan, optional `allow_unresolved` | Validate each assigned preset against the new asset/mapping; preserve MIDI IDs; one commit for the bank and channel changes. Never choose an arbitrary first preset. |
| `instrument.soundfont.set_channel_preset` | `track_id`, channel, typed SF2 preset ref, catalog generation | Update one channel's initial bank/program; keep its controllers and all other bindings. |
| `instrument.soundfont.set_channels` | `track_id`, bounded channel binding changes | Validate distinct channels and every binding before one atomic commit; covers enable/disable, presets, initial CC and bend defaults. |
| `instrument.soundfont.set_render_options` | `track_id`, descriptor version, changed options | Validate block size, shared maximum polyphony and internal reverb/chorus setting; rebuild next generation and invalidate source cache. No unsupported live setter is assumed. |

`instrument.track.set_parameters` remains the generic descriptor-based SoundCraft control path and the path for genuinely supported RustySynth runtime controls. It does not reinterpret bank/program or construction settings as arbitrary parameters. Project source enabled state defaults to true; it is separate from strip bypass and from UI panel visibility.

Example one-channel preset change:

```json
{"expected_revision":12,"command":{"type":"instrument.soundfont.set_channel_preset","track_id":"track-01","channel":9,"catalog_generation":"catalog-07","preset_ref":{"kind":"soundfont","asset_id":"sf2-01","content_sha256":"0000000000000000000000000000000000000000000000000000000000000000","bank":128,"program":0}},"include_snapshot":false}
```

Selecting a preset changes the saved initial setting, not recorded MIDI events. The result includes affected IDs and a diagnostic if later recorded program/bank events override it. Applying a SoundCraft snapshot does not copy parameters into a different component or erase instrument parts. Runtime controls appear in the authoritative result only after Rust validates/commits them; frontend widgets refresh from that result.

### 4.5 Backend capability and library mapping guardrails

Descriptors list supported MIDI message kinds and actual CC numbers, bank-selection encoding/mapping, channel count/mapping, note-off/release-velocity semantics, event timing, preset enumeration/state restore/native editor support, and resource/polyphony limits. Unsupported controls return `unsupported_operation`/`unsupported_midi_event`; a generic no-op library method is not a valid successful control implementation.

Pinned SoundCraft DSP audit: builtin synth supports explicit note offsets and a subset of sustain/reset/all-off/pitch-bend messages; drum synth uses one-shot note-off behavior; sampler needs its explicit sample binding. Per-component declarations reflect those differences. Hosted plugin MIDI/parameter/editor support is established per plugin instance and platform, not inferred from its VST3/CLAP/AU label.

Pinned SoundCraft VST3/CLAP host audit: the generic note-on/off interface has no MIDI channel argument; VST3 note events and the native CLAP-note path encode channel 0. VST3 `midi()` routes supported CC/bend/pressure through plugin mappings but does not handle MIDI Program Change. Unmodified adapters must declare those limits. Raw MIDI-capable paths may differ, but cannot advertise broader note/program support until their complete path is verified. A 16-channel RustySynth configuration is not proof of 16-channel SoundCraft support. An explicit stored `midi_channel_map` may remap channels only when the selected adapter supports the resulting semantics; no automatic folding or program substitution occurs.

The user expects SoundCraft libraries/adapters to be extended where needed. The paragraph above records the audited upstream baseline, not a permanent channel-0/product restriction. MK3 retains channel-aware MIDI/control semantics; the integration work below closes host-side gaps. Independent restrictions of a real plugin or platform still require honest capability reporting.

Pinned RustySynth 1.3.6 audit: `SoundFont::get_presets()` exposes bank/program/name; channel control uses `process_midi_message`, and construction settings are separate from runtime `set_master_volume`. The adapter must validate all notes, CCs, program/bank transitions and output buffer lengths before entering the library. CC 32 (bank LSB) is not implemented by that release and must not be advertised merely because it is standard MIDI. Channel 9 has special percussion-bank behavior; preset queries report whether the selected bank is mappable on the requested logical channel. An adapter may add verified mapping/instance support, but unsupported mappings reject explicitly.

That release's `note_on` falls back to a GM/default preset if a requested bank/program is absent. MK3 forbids silently exposing that fallback: render preflight simulates initial and recorded per-channel bank/program state, validates the effective preset at every note-on, and either proves an exact mapping or fails before publishing that render's PCM. Direct/audition messages undergo the same validation. Asset generation is pinned between the check and render.

Controller/program state persists across adjacent/overlapping parts on the same track; the engine does not reapply initial settings at every part boundary. A nonzero stream start reconstructs prior state through approved preroll/checkpoints. Natural project end releases held notes and sustain according to the declared release policy, then drains a bounded tail. Panic/cancel clears held notes and queued backend PCM; no stuck note survives an audition/session close.

### 4.6 Audition and native editor lifecycle

When `instrument.audition.v1` is advertised, runtime methods are:

| Method | Scope/result |
|---|---|
| `instrument.audition.open` | `track_id`, expected revision -> pinned `audition_id`, config generation, supported channels/messages and timing. |
| `instrument.audition.note_on` | Audition ID, channel/key/velocity -> engine-issued note token and requested/effective scheduling frame. |
| `instrument.audition.note_off` | Audition ID, note token -> release with declared backend semantics. |
| `instrument.audition.midi` | Audition ID and supported typed CC/program/bend message -> validated transient control. |
| `instrument.audition.all_notes_off` | Release held notes where supported; return tail behavior. |
| `instrument.audition.panic` | Clear held notes, sustain and pending PCM immediately for that audition only. |
| `instrument.audition.close` | Release/clear and retire runtime state; repeated close is idempotent within bounded handle retention. |

Audition mutates no document/history or saved preset. A project/backend change invalidates old audition handles and the UI opens a new one. Disconnect/session close panics and retires owned audition/editor handles. It does not cancel an unrelated export. Note tokens identify logical requests; if the backend uses MIDI key-level releases, metadata and overlap diagnostics disclose that limit.

For a host/plugin advertising `instrument.editor.native.v1`, `instrument.editor.open(track_id, expected_revision)` returns an editor handle and an engine control instance; `instrument.editor.close(editor_id)` closes it. These calls run on the required host UI/control thread. Native windows are host surfaces, not assumed to embed in a WebView. Generic descriptor controls remain available where implemented. Editor parameter gestures and opaque state captures pass through §10's Rust transaction/history rules; opening/closing a window itself increments no project revision. Unsupported native editors return a typed error rather than a fake open result.

### 4.7 Read/view consistency and unavailable resources

`instrument.track.get` keeps committed values and runtime diagnostics separate:

```json
{"session_id":"session-01","revision":13,"track_id":"track-01","source_enabled":true,"configuration":{"backend":"soundcraft","instrument":{"kind":"builtin","instrument_id":"sampler","descriptor_version":1,"parameters":{},"sample_ref":"asset:sample-01"}},"runtime":{"status":"unresolved","config_revision":13,"missing_assets":["sample-01"],"editor_available":false}}
```

The example's empty parameter map is an illustrative descriptor default; real canonical reads materialize descriptor values. Read/render/save use the same committed bindings. Queries expose availability but never replace it with a guessed preset or mutable frontend config. Unresolved open/edit/save remains possible; playback waits for explicit resolution under the recorded user decision.

### 4.8 Hosted VSTi preparation and control completeness

Before a hosted instrument becomes `ready`, Rust resolves exact format/component identity, confirms an instrument role and event input, negotiates a supported audio output bus/layout, restores accepted state, applies canonical parameter overrides, sets transport/sample rate/max DSP block and prepares a generation-private instance. Event/control queue bounds and accepted MIDI channels/messages are reported in its descriptor. Effects-only components or unsupported bus/layout/event combinations reject with structured diagnostics. Additional MIDI/audio ports and multi-output routing require explicit capabilities/mappings; they are not silently merged or discarded by the stereo base contract.

Controls for a SoundCraft-hosted track therefore include discovery/status, component selection, parameter reads/edits/reset, engine-managed preset apply, opaque state load/capture, source enabled state, MIDI render/audition, and native editor open/close where supported. Optional native program selection is `instrument.track.select_program(track_id, program_ref, catalog_generation)` in the normal command envelope only when `backend_programs` is advertised. It captures resulting state/parameters in one Rust history transaction. Plugin-provided MIDI program events remain independently gated and are not simulated by silently choosing a different state.

Host instance/editor creation and retirement obey platform thread rules. The full state-capture/restore order is state first, canonical overrides second; failure leaves the committed prior configuration intact. Native editor windows are optional host surfaces, while descriptor controls are the normal UI contract. Real plugin ABI/IO/MIDI/editor/state behavior must pass conformance per tested plugin/platform; this document's complete control specification does not certify a plugin host implementation or every VSTi.

Planned SoundCraft library/adapter changes, rather than reasons to narrow the project contract:

| Integration gap | Target behavior and evidence |
|---|---|
| Note events lose channel and/or sub-block position | Extend event records and format adapters to preserve logical channel `0..15`, note identity where the format supports it, and the event's frame offset in the prepared block. Prove nonzero-channel note-on/off and block-edge delivery in both streaming and offline render; never fold a channel or clamp an offset silently. Advertise each plugin's actual channel/note-ID limits. |
| Program Change is absent from the audited VST3 path | Add typed scheduled bank/program events and verified format/plugin mapping where supported. Keep timeline Program Change distinct from applying a saved preset/state. Include channel and frame offset; reject an unmappable bank/program before publishing PCM. |
| MIDI control path is partial or defaults to no-op | Inventory CC, pitch bend, channel pressure and any other claimed messages per built-in and hosted format. Implement explicit mappings with return status; preserve event order and declared timing. Never report success for an unhandled event. SysEx/aftertouch/MPE remain unavailable unless a separate capability is implemented. |
| Generic trait lacks a complete instrument-control service | Add adapter services for discovery, descriptor/versioned parameter reads and writes, preset listing/application, state inspect/load/capture, source readiness and optional editor lifecycle from §4.3–§4.8. Use real plugin metadata; do not invent program lists, parameter units, writable flags or editor support. |
| Processing contract does not prove an audio-ready VSTi instance | Verify instrument role/event input, output buses/layout, sample rate, maximum block, channel mapping, latency and tail reporting during prepare. Reject unsupported configurations before `ready`; test reset/reprepare and state restoration. No unreported port merge or output discard. |
| Realtime and offline paths may dispatch events differently | Route both through the same Rust-owned event ordering, conversion and capability validation. Pin configuration/state per render generation; compare scheduled event traces and PCM under the declared deterministic/tolerance policy. UI edits commit through Rust and never mutate an active render instance. |
| Upstream API/library may need patches | Pin the chosen upstream revision plus owned patch/fork provenance, maintain notices, and keep compatibility tests for channel/offset/message/IO/latency/state/editor behavior. The Rusted UTAU Project Session remains authoritative. |

SoundCraft-hosted VSTi completeness matrix (contract scope versus implementation evidence):

| User-visible capability | Contract location | Required SoundCraft/adapter work | Acceptance evidence before advertisement |
|---|---|---|---|
| Find and select a VSTi by stable component identity | §§3, 4.3, 5 | Scan/instantiate off the audio thread; expose format, component identity/version, instrument role and availability; bind catalogue cursors to a generation. | Rescan/change-generation behavior, stale cursor rejection, missing-plugin open and exact component selection. |
| Play notes and sustain/release on logical MIDI channels | §§4.1, 4.5, 4.8 | Carry channel and frame offset through SoundCraft event structs and CLAP/VST3/AU adapters; retain plugin-specific note-ID/channel limits. | Nonzero-channel note-on/off, same-frame ordering, half-open block boundary, overlap/release and unsupported-channel cases. |
| Deliver recorded controller/program/bend events | §§4.1, 4.5, 4.8 | Explicit per-format conversion for supported CC, program/bank, bend and pressure; no default no-op or silent fallback. | Exact event trace at requested/effective frames; missing program/CC mapping fails before PCM; per-channel selection where the plugin supports it. |
| Browse and apply plugin presets; edit parameters | §§3, 4.3–4.4 | Stable preset refs and parameter descriptors (ID, type, unit, range, default, step, choices, writable/automatable); distinguish plugin programs from host-managed snapshots. | Descriptor round-trip, stale generation, invalid value atomic rejection, preset application/undo and host notification behavior. |
| Save, restore and capture plugin state/editor changes | §§4.4, 4.6, 4.8, 10 | Capture opaque state plus canonical parameters on the required control thread; stage restores in a private instance; commit once through Rust. | Save/reopen, failed restore, editor change capture, conflict during capture, undo/redo and immutable render snapshot. |
| Prepare stable audio processing | §§4.8, 5, 6, 8 | Negotiate event/audio ports, sample rate, max block, output layout, latency and tail; use generation-private instances and bounded queues. | Prepare/reprepare/reset, latency/tail continuity, bounded failure and matching realtime/offline scheduling. |
| Expose native editor when supported | §§4.6, 5 | Report editor availability per component/host; marshal open/close to required native UI thread. Keep descriptor controls available as the portable surface. | Correct native-thread lifecycle on each supported host; explicit unavailable result on WebView/WASM or headless hosts. |

This matrix defines coverage, not a promise that every VSTi or format supports every row. A capability is advertised only after the corresponding adapter and tested component/host combination pass its evidence. Initial hosted-VSTi support should not claim multitimbral routing merely because the logical event model supports 16 channels; the concrete channel map and plugin behavior must be verified.

The backend extension acceptance belongs to the hosted-instrument capability increment, not a prerequisite to base T033 voice/wave streaming. It requires actual integration work and validation before advertisement; writing this target does not imply those patches already exist.

## 5. Capabilities and target adapters

MK3 capability DTOs retain the parent shared-versus-host separation. Draft version values are `contract_version: "3.0-draft"`, `command_schema_version: 3`, `snapshot_schema_version: 3`, and existing `id_schema_version: 1`.

Independently advertised feature groups:

- `stream.pcm.v1`, `stream.sequential_fx.v1`, `stream.seek.v1`, `stream.live_edits.v1`;
- `instrument.soundcraft.builtin.v1`, `instrument.soundcraft.hosted.v1`, `instrument.rustysynth.sf2.v1`, `instrument.audition.v1`, `instrument.editor.native.v1`;
- `mixer.inserts.v1`, `mixer.routing.v1`, `mixer.automation.v1`;
- `bundle.rustx.v2`, `export.ustx.instrument_audio.v1`.

`host.instrument_backends[]` reports actual available kinds/formats/component IDs and unavailability reasons. Supported MIDI kinds/CCs, channel mappings, event timing profiles, preset/editor/state capabilities, channel/layout limits, state limits and descriptor versions are queryable. RustySynth's initial group requires 16 independent logical channel binding/controller slots, with playable preset mappings declared by the adapter. `render.stream` reports supported modes/PCM formats, per-session stream count, block/header/control limits, outstanding-request limit, queue bounds, maximum wait/idle duration and tail bounds. Every declared numeric limit is enforced; absence of an optional feature means unavailable, not unbounded.

Web/WASM is a direct/worker adapter. SoundCraft built-ins and RustySynth are candidates requiring real build, asset and audio evidence. Hosted native plugins require a separately verified host; browser support is not implied. Android uses the user-selected WebView/local-Rust-server WebSocket architecture. Desktop plugin formats remain platform-specific. Existing HTTP APIs remain compatible; MK3 transport support is advertised separately.

## 6. Streaming API, states and ordering

### 6.1 Logical operations and snapshot

```text
session_render_stream_begin(expected_revision, options) -> StreamInfo
session_render_stream_next(stream_id, max_frames, wait_ms) -> StreamReply
session_render_stream_cancel(stream_id) -> TerminalReply
session_render_stream_close(stream_id) -> CloseReply
```

`options` requires `mode: playback|export`, sample rate, channel layout and frame range `{start_frame, end_frame?}`. Positions/counts are nonnegative JSON-safe integers for new stream fields; Rust uses checked integer arithmetic. Initial audio layout is stereo; additional layouts require a declared capability. Export mode pins its snapshot and continues despite subsequent project edits. Playback mode follows §8's edit policy.

`begin` validates the committed revision, pins project/backend state and immutable asset generations, and returns opaque `stream_id`, Rust-minted `generation`, `session_id`, `revision`, negotiated sample rate/layout/format, range and limits. Input assets remain referenced until the stream is closed; replacements do not mutate an existing snapshot. One stream uses its own instrument/FX runtime instances. An externally modified asset that cannot satisfy its pinned content identity fails as `asset_unavailable`; it cannot contribute a mixture of old and new bytes.

StreamInfo additionally returns `timing_profiles` keyed by instrument track ID, with requested/effective-frame rules and event quantum from §4.1. Empty voice/wave-only streams report an empty list. Profiles and persisted synthesis settings participate in source/cache identity. PCM transport block length and backend event quantum are separate values.

The range is half-open and must satisfy `end_frame >= start_frame` when an end is supplied; equal bounds represent an empty request. Negotiated output settings are immutable during the stream. A begin result has this shape:

```json
{"status":"preparing","stream_id":"stream-01","generation":"gen-01","session_id":"session-01","revision":12,"sample_rate":44100,"channels":2,"layout":"stereo","pcm_format":"f32le_planar","range":{"start_frame":0,"end_frame":null},"timing_profiles":[],"limits":{"max_block_frames":512,"max_wait_ms":20}}
```

Numbers in examples illustrate a capable host; they are not shared minimums or performance targets.

#### T034 proposal: seek through versioned FX checkpoints

`stream.seek.v1` enables nonzero `range.start_frame` on `session_render_stream_begin`; it adds no seek command and no checkpoint identifier or DSP snapshot to the wire DTOs. Without that feature, a nonzero start is rejected as `unsupported_range` before a stream is created. With it, the new stream pins the same immutable render identity as any other begin, starts its own generation and sequence at zero, and labels its first emitted block with the requested `frame_start`. The playback client cancels the previous stream generation and flushes its queued PCM before accepting the new generation; blocks from the prior generation remain invalid.

Rust may restore the nearest compatible internal checkpoint at or before the requested frame, process/discard the intervening source frames, and emit at the exact requested frame. If there is no compatible checkpoint, it deterministically replays from frame zero and discards to the target. Checkpoints are session-local, in-memory typed FX state captured only after completed block boundaries. Bind reuse to session/revision, plan and renderer/build identity, immutable source-asset byte identities, sample rate/layout, ordered FX configuration, processor-state version, and per-track processed frontier. Any mismatch rejects the checkpoint atomically; it must not partially restore active processors. The session-local cache has a 32 MiB byte cap, evicts the oldest checkpoint deterministically when full, and is cleared on session close. Charge actual cloned DSP payload bytes against the cap. A cold seek may still need to regenerate voice/wave source audio; FX checkpoints do not promise source-render skipping or bounded seek latency.

An open-ended seek whose `start_frame` is strictly beyond the computed source end terminates with `end` and emits no PCM. Equal explicit bounds are also an empty request under the range rule above. Legacy USTX `MixFx` remains source-bounded in both cases.

Legacy USTX `MixFx` remains source-bounded in both sequential and sought output; seek does not drain its post-source tail. Only a new processor with its own declared finite tail policy may extend output under §9.2. This T034 proposal does not change frozen CONTRACT-v1, add project persistence for DSP state, or claim that `stream.seek.v1` is implemented or advertised.

| State | `next` result | Transitions |
|---|---|---|
| `preparing` / `rendering` | `pending` or ordered `data` | More data, draining, stale, cancelled or failed. |
| `draining` | `pending` or ordered `data` with instrument release tails and explicitly permitted processor tails | End, stale, cancelled or failed. Legacy USTX `MixFx` does not extend output beyond its source end. |
| `ended` | Stable `end` | Close. |
| `cancelled` / `stale` / `failed` | Stable matching terminal reply | Close. |
| `closed` | `stream_closed` | Tombstone expiry. |

`next` validates `1 <= max_frames <= max_block_frames` and `0 <= wait_ms <= max_wait_ms`. One outstanding `next` per stream is allowed; a concurrent call returns `stream_busy`. `cancel` is allowed while `next` waits. If no block is ready by the deadline, reply `pending`; it consumes neither sequence nor frames. Pending may carry progress and a bounded retry hint, but never implies completion. Consumers must not busy-spin.

Data blocks have strictly positive frame count. An empty project/range returns `end` with no data blocks. A short data block is not EOF. `end` includes `next_sequence` and `final_frame_exclusive`; requested hard range ends and truncation under an explicitly permitted component tail policy are reported. Repeated terminal reads return the same terminal information without new PCM.

`cancel` moves an active stream to `cancelled`; cancelling an already terminal stream returns its existing terminal state. It affects no other stream. `close` cancels active work and releases it; repeated close is successful while a bounded closed-handle tombstone exists, and an unknown/expired handle returns `stream_not_found`. Session close/disconnect cancels and closes owned streams. Version 1 has no cross-connection stream resume.

### 6.2 Data and terminal DTOs

Example data header (PCM bytes travel separately as specified in §7):

```json
{"request_id":"req-02","status":"data","stream_id":"stream-01","generation":"gen-01","session_id":"session-01","revision":12,"sequence":0,"frame_start":0,"frame_count":512,"sample_rate":44100,"channels":2,"layout":"stereo","pcm_format":"f32le_planar"}
```

```json
{"request_id":"req-03","status":"pending","stream_id":"stream-01","generation":"gen-01","session_id":"session-01","revision":12,"next_sequence":1,"next_frame":512,"retry_after_ms":10}
```

```json
{"request_id":"req-04","status":"end","stream_id":"stream-01","generation":"gen-01","session_id":"session-01","revision":12,"next_sequence":1,"final_frame_exclusive":512,"warnings":[]}
```

All stream replies identify session, stream, generation and pinned revision. The closed status enum is `pending|data|end|cancelled|stale|failed`; unknown handles are operation errors rather than fabricated stream replies. Terminal `failed` adds the inherited structured `error` object and diagnostic asset/backend IDs. Terminal replies include `next_sequence` and `final_frame_exclusive`, identifying the emitted prefix even on failure; `pending` never contains PCM. `sequence` starts at zero; `frame_start` starts at the requested range start. Errors that reject a `next` request do not consume either counter.

One Rust project plan includes every contributing voice, wave and instrument part. Emission is ordered by absolute output frame, regardless of worker completion order. Context/crossfade overlap is resolved internally and cannot duplicate emitted samples. Output time is latency-compensated timeline time; preroll/latency warmup is not mislabeled as project frame zero.

### 6.3 Bounds and playback clock

Queues/lookahead have negotiated frame/byte limits. A producer does not enqueue beyond the consumer's outstanding pull and bounded internal window. Filling the queue pauses production or schedules bounded work; dropping ordered data silently is prohibited. Maximum block size is a protocol/host bound, not an assumed DSP quantum. Decoded wave caches, voicebank/SF2/sample residency and transfer buffers are accounted for separately under enforced host asset/cache quotas. Full-bank loading may be a declared backend requirement, but no entire-song final mix allocation may be hidden behind a claim of bounded streaming queues.

Initial underrun behavior is silence at the audio device while the musical consumed-frame cursor remains paused; this silence is measured as underrun output, not counted as project PCM. Resume continues the next project frame after sufficient negotiated prefill. Offline PCM is never padded with underrun silence. The UI shows buffering and derives the playhead from acknowledged project frames consumed by the audio sink, not wall-clock render progress. Producer completion and audible end are different events.

## 7. Android WebSocket and Web/WASM mapping

Draft native route: `/api/v3/ws`, subprotocol `rusted-utau.v3`. This is a proposed route, not a live claim. The wrapper supplies the launch-scoped local-server credential; connection setup checks credential and configured origin. No document mutation occurs during negotiation.

The first JSON message is `hello`, with `session_id: null` and `params` containing the launch credential, offered contract versions and required feature groups. Authentication and negotiation complete before session/stream operations are accepted. Its response selects one version, provides capabilities, or rejects incompatible required features. Session creation/open then uses the parent logical lifecycle. Normal control requests use `{request_id, method, session_id, params}`. Mutation `params` contains the existing `expected_revision` envelope; direct/WASM adapters map to the same logical fields/errors. JSON responses echo `request_id` and authoritative revision. Request IDs are unique per connection. Reusing an ID is `duplicate_request`; duplicate mutation application is prohibited. After a lost connection the client rereads committed state before retrying a mutation with a new ID. Credential values are host-local secrets and not project state.

Streaming uses pull in version 1. One `next` produces exactly one pending/data/terminal response. No unsolicited PCM push or separate credit protocol is introduced. For data, a single binary WebSocket message contains:

```text
bytes 0..4:  ASCII RUP3
bytes 4..8:  little-endian u32 JSON-header byte length H
bytes 8..8+H: UTF-8 JSON data header, including request_id
remaining:   PCM payload, exactly channels * frame_count * 4 bytes
```

PCM is planar little-endian Float32: all frames of channel 0, then channel 1, and so on. Non-finite output samples cause a typed backend/render failure. Header/control/payload bounds are checked before allocation; lengths/layout/negotiated format must agree. Pending/terminal responses are JSON. Connection ordering correlates each binary data frame with its header; no separate metadata message can be mismatched.

WASM worker data responses carry the same header and one transferred `ArrayBuffer` with the same planar sample order. Transfer relinquishes the sender's writable ownership. Buffer-pool reuse cannot occur until ownership has been returned or a distinct buffer is available. Direct typed calls may expose equivalent planar views with an explicitly bounded lifetime.

Cancel cannot retract bytes already delivered to a device/socket. On cancel/stale, the sink immediately rejects further blocks of that generation and flushes queued project PCM. After terminal acknowledgement the producer publishes no new data. Version 1 disconnect/reconnect starts a new stream generation; binary data from an old connection is rejected.

## 8. Revisions, invalidation, cache and edit behavior

Document revision and audio/cache identity are distinct. Rust returns invalidation for source generation, FX processing or final mix; frontends do not derive those distinctions from command names.

- Name/color/comment changes that do not affect Rust's audio plan leave playback running. The stream retains its original pinned revision and Rust reports compatibility with the newer document revision; it is not falsely retagged.
- For playback, any committed audio-affecting edit stops the old generation and flushes its queued audio. The UI requires an explicit new Play. This includes notes, instruments, FX and fader/pan changes in the first increment.
- Export streams continue using their pinned snapshot and are returned with that revision. Existing v2's explicitly labelled stale completed audio remains available through its old workflow; stopped MK3 playback does not silently resume it.
- Vocal source identity covers notes/timing/singer/voicebank/source-render parameters and source asset generations, but excludes downstream fader/pan/mixer FX. Instrument controls that change its waveform invalidate that instrument source. FX changes invalidate the corresponding processing stage; routing/fader/pan invalidate downstream mix.
- Restarting after a mix-only edit reuses valid source chunks where possible; it does not require resynthesis just because document revision increased. Cache correctness is Rust-owned and output-affecting engine/descriptor versions participate in keys.
- Seamless live edits are the future `stream.live_edits.v1`/T035 capability. Seek/state checkpoints belong to `stream.seek.v1`/T034. Their absence does not prevent base sequential streaming.

## 9. Mixer entities, FX and signal graph

### 9.1 Minimal insert model and commands

The initial graph has track strips and a main output. Rust mints persistent strip IDs; each track has one strip linked by its track ID and each project has one main strip. Each strip owns an ordered list of inserts with `insert_id`, processor descriptor identity/version, complete canonical parameter map and `bypass`. Main-output inserts use the same model. SoundCraft DSP processors are candidates for the backend; USTX `MixFx` stays in its own existing domain structure. A null/omitted `before_insert_id` appends; a specified ID must belong to the destination strip.

```json
{"insert_id":"insert-01","processor":{"backend":"soundcraft_dsp","processor_id":"eq_7band","descriptor_version":1},"parameters":{},"bypass":false}
```

Processor metadata defines units, finite min/max/default, step/enum values, layouts, latency and tail policy. Rust materializes defaults. Unknown processor/version on load preserves the insert as unresolved; active rendering rejects it until resolved or explicitly bypassed.

Draft command names use the parent revision/transaction envelope:

| Command | Fields |
|---|---|
| `mixer.insert.add` | `strip_id`, processor descriptor, parameters, optional `before_insert_id`; Rust mints insert ID. |
| `mixer.insert.remove` | `insert_id`. |
| `mixer.insert.move` | `insert_id`, destination strip ID, optional `before_insert_id`. |
| `mixer.insert.set_parameters` | `insert_id`, descriptor version, parameter map. |
| `mixer.insert.set_bypass` | `insert_id`, `bypass`. |

All commands validate layout and identity, are atomic, and participate in grouped undo/redo. No-op edits change no revision. Bypass is a persisted hard bypass: processing is skipped and its tail is cut at the new generation; returning from bypass starts with the defined prepare/reset path. Click-free live bypass is not promised by the first stopped-edit workflow.

### 9.2 Ordered graph and continuity

Selected track path:

```text
voice/instrument/wave contribution assembly
  -> new track mixer inserts
  -> existing track fader/pan applied exactly once
  -> legacy USTX MixFx, preserving its existing order
  -> align contributing paths for their processing latency
  -> main summation
  -> new main-output inserts
  -> remove total render warmup latency from output timeline
  -> output
```

Voice+wave legacy tracks retain the existing shared bus and legacy FX semantics, including source-specific mono/stereo conversion and pan law. The diagram defines processing order, not permission to replace those numerical rules. Empty new-insert lists must reproduce the legacy result within the approved PCM comparison. SoundFont/instrument internal effects belong to source generation, and are not automatically replaced with mixer FX.

Every stream has independent sequential FX state. Construction, sample loading, expensive prepare, native plugin setup/state capture and destruction happen outside time-critical audio processing. The stream processes in order and drains note releases plus only the FX tails explicitly declared by active processor components, up to negotiated maximum tail frames. Truncation reports an explicit warning with the truncated frame count/policy; the offline reference uses the same policy. Latency compensation aligns contributing paths and removes internal warmup delay from timeline frame labels. Continuous/bounded-tail and seek reconstruction vectors are different conformance groups.

Tail behavior is component-specific. The legacy USTX `UMixFx` track chain keeps OpenUtau's source-bounded output length: it processes the source frames, carries state across blocks, and emits no extra post-source tail frames in either streaming or offline output. A future SoundCraft-derived mixer insert may declare its own finite tail and use the negotiated drain bound; that policy does not change legacy USTX track-FX duration.

### 9.3 Legacy USTX `MixFx` contract (merged from the T018 extension)

The existing `UTrack.mix_fx` object remains the USTX-compatible track-effects chain. It is separate from the new `mixer.insert.*` graph above and is serialized in the USTX track. The canonical read shape is `mix_fx: null` when absent, or the complete `UMixFx` value when present. Preset labels are retained for display; concrete numeric values drive DSP. Preset libraries/names remain host preferences and are not project data.

```json
{"mix_fx":{"enabled":true,"eq_preset":"vocal_air","comp_preset":"gentle","reverb_preset":"small_room","eq_low_db":0.0,"eq_mid_freq":3000.0,"eq_mid_db":1.5,"eq_high_db":3.0,"comp_threshold_db":-18.0,"comp_ratio":2.0,"comp_makeup_db":2.5,"reverb_size":0.30,"reverb_damp":0.7,"reverb_wet":1.0,"reverb_pre_delay_ms":0.0}}
```

`track.set_mix_fx` replaces the complete value atomically using the parent revision/command envelope and stable `track_id`. An explicit `mix_fx: null` clears it; `enabled: false` preserves its settings while bypassing processing. Omission of the field is invalid. EQ preset keys are `off`, `vocal_air`, `warm`, `demud`, `telephone`; compressor keys are `off`, `gentle`, `pop`, `limit`; reverb keys are `off`, `small_room`, `vocal_plate`, `hall`, `ambient`. Numeric fields must be finite and within: EQ gain ±24 dB, mid frequency 20–20000 Hz; compressor threshold −60–0 dB, ratio 1–20, makeup ±24 dB; reverb size/damp 0–1, wet 0–2, pre-delay 0–200 ms. Unknown preset keys and invalid ranges reject without mutation. No-op values do not increment revision/history; applied edits make one undo item and invalidate that track's downstream mix while preserving reusable source renders.

The legacy chain order is EQ → compressor → Freeverb reverb. It runs after the existing track fader/pan and after the new pre-fader inserts, exactly once per track, before project summation. Null or disabled values bypass it. FX state persists across stream blocks; a new render/seek generation follows the declared reset/reconstruction policy. Dry-stem/export intent is explicit (`apply_mix_fx`), never inferred from hidden frontend state. Stereo parity is required: duplicating a mono signal into two channels and folding it back is not accepted as proof. Save/reopen and USTX import/export preserve the full `UMixFx` value.

This legacy chain is source-bounded: it does not lengthen stream or offline output to emit Freeverb after the track's final source frame. Its internal state still persists across contiguous source blocks and is checkpointed/restored for seek. New SoundCraft-derived inserts may use a separate declared bounded-tail policy under §9.2.

This subsection consolidates the per-track Mix FX contract from [T018](../work/tasks/T018-openutau-flags-and-track-fx.md) and the former [standalone T018 extension draft](contract-v2-extension-t018-fx.md). T018 remains responsible for renderer-flag parity and verification of the already implemented session/render paths. Neither this merge nor the draft ratifies the extension or changes frozen `CONTRACT-v1` §6.

Buses/aux/pre-post sends, routing-cycle validation and automation are `mixer.routing.v1`/`mixer.automation.v1` follow-ups. They do not have implicit support through the insert model. Loading a graph that needs an unsupported group preserves its data and marks rendering unavailable. The first insert increment cannot silently flatten an unsupported graph.

## 10. Runtime backend controls and saved state

Per-stream runtime instruments/FX are distinct from persistent document configuration. Simultaneous playback/export cannot share mutable MIDI/FX instances. Shared immutable banks/samples are permitted while asset generations remain pinned.

A Rust-owned control/editor service handles native plugin editor changes. It captures parameters and opaque state into one normal session transaction; UI code never serializes it. A descriptor edit or opaque state change increments revision once and invalidates the proper audio stage. The editor uses a separate control/audition instance, not the pinned instance of an export stream.

On revision conflict, a pending editor capture does not overwrite newer committed state; the control instance restores/reconciles against Rust and reports the conflict. Save/export either uses a fully committed capture or reports `state_capture_pending`/capture failure; it cannot claim current settings while saving older state silently. Opaque state blobs have backend/schema version, byte size and content digest; restoring a rejected blob produces `plugin_state_rejected` without substituting plugin defaults silently.

On open, unavailable executable/bank/sample/processor leaves configuration and IDs intact. Relink is an explicit `asset.relink` Rust command with asset ID, host-resolved source token and expected content identity. A different-content replacement requires an explicit `allow_content_change` intent, updates the immutable asset generation and invalidates affected contributors. Host tokens do not become serialized absolute paths. Strict rendering fails if an audible contributor is unresolved. Initial `.rustx` has no automatic frozen-audio fallback; the project can still be saved/edited without resolving every asset.

## 11. `.rustx` v2 and USTX interoperability

### 11.1 Candidate versioned layout

This proposes `.rustx` container version 2; current version 1 is unchanged. Rust serializes/validates all musical/extension/identity data. Host adapters perform storage, not a competing codec/model.

```text
manifest.json
project.ustx
project.ids.json
project.ext.json
media/{asset-id}/{file-name}
assets/{asset-id}/... 
```

`project.ustx` remains canonical for supported existing voice/wave musical fields. `project.ids.json` uses the existing identity sidecar semantics and is bound to those canonical USTX bytes. `project.ext.json` has its own schema version and contains only extra track roles/instrument configs, instrument parts/MIDI, new mixer data, asset references and linkage by stable IDs. It does not duplicate existing vocal notes or USTX `MixFx`.

The manifest records format/container version, required extension versions, entry references, canonical USTX SHA-256 and asset inclusion diagnostics. Rust reconstructs one document from USTX+identity+extension. Conflicting IDs/definitions or a hash/linkage mismatch returns a typed corruption/compatibility error rather than silently attaching an instrument to the wrong track. Save/export derives all entries atomically from one committed revision; server filesystem saves use write-temp/validate/rename, and browser downloads publish only a complete validated bundle.

Candidate manifest shape (the illustrative zero hash must be replaced by the actual canonical-byte digest):

```json
{"format":"rusted-utau-project","version":2,"project":{"ustx":"project.ustx","identity":"project.ids.json","extension":"project.ext.json","ustx_sha256":"0000000000000000000000000000000000000000000000000000000000000000"},"required_extensions":{"rusted_utau":1},"assets":[],"portability":{"complete":false,"external_dependencies":[]}}
```

Candidate extension root:

```json
{"schema_version":1,"project_id":"project-01","track_extensions":[],"instrument_parts":[],"mixer":{"strips":[]},"asset_references":[]}
```

Each `track_extensions` entry links a base track ID to its `track_role`, source enabled state, optional explicit MIDI channel mapping and configuration from §3, including every SoundFont channel binding and construction option. Each instrument part uses §4's view. Mixer strips link strip IDs to track/main targets and §9's ordered insert list. Asset-reference entries contain asset identity, kind and content digest; the manifest controls bundle inclusion/path, while the extension controls musical/backend use. Conflicting asset identity metadata is rejected. Bank file paths must remain within their asset entry and obey negotiated count/expanded-byte limits.

Unknown required extension/container versions reject without overwriting a file. Optional unavailable backend descriptors are preserved as unresolved. Version 1 opens without instrument/mixer extensions; writing v1 from a document that needs v2 requires explicit lossy export and its loss report. The Rust v1 codec migration retains its existing validation/limit behavior before adding v2.

### 11.2 Asset inclusion/exclusion and portability report

Asset records contain opaque asset ID, kind (`audio`, `voicebank`, `soundfont`, `sample`, `plugin_state`), size, content digest, logical reference and availability/inclusion status. Directory banks have a bounded file list with safe relative paths and per-file size/digest. Bank paths and display names never replace content identity.

Export receives explicit per-asset include/exclude overrides. A normal profile embeds available project audio and committed plugin state; banks/SoundFonts/sampler collections remain references unless inclusion is selected. A portable profile attempts to include all required permitted data, subject to explicit exclusions. Inclusion requires the user's permission/rights declaration for that asset; crate source licenses do not grant rights to bank/plugin content. The engine records policy/unknown eligibility and never asserts legal permission itself.

Excluded assets retain descriptors, references and all editable notes/settings. Embedded voicebanks and SoundFonts use the same logical resolution path as external assets. Native plugin executable binaries are not bundled in initial v2; a plugin track still requires compatible installed code. The portability report lists every external/missing/excluded dependency and required backend. A bundle with unresolved requirements is described as incomplete rather than claiming it opens and renders on every host.

Missing dependencies do not prevent opening/editing. Rendering the affected audible graph requires explicit relink. No automatic track render cache, frozen take or fallback audio is embedded under this decision. Audio files explicitly authored/imported by the user and audio stems produced by an explicit USTX export remain legitimate project/export assets.

### 11.3 USTX export modes

`export.ustx` is snapshot-based and does not modify the editable `.rustx` source. All modes return a structured compatibility/loss report and the list of written audio assets. Main-output nonlinear/shared processing cannot be reproduced by independently applying it to stems.

- `compatible`: retain original USTX voice/wave data where representable. Render each instrument's source to an audio part with release tail; preserve its representable USTX fader/pan/legacy FX settings so they apply once. If new mixer FX, routing or other non-USTX state would be lost, return `export_requires_bake` with concrete affected IDs instead of publishing incomplete output.
- `track_bake`: render each track through its complete track strip (new inserts, fader/pan, USTX FX), export one stereo wave stem per track and neutralize the exported USTX track fader/pan/FX. Vocal editing is lost in this derivative export; the source `.rustx` remains editable. Main/bus/send processing that cannot be represented independently requires `mix_bake`.
- `mix_bake`: render the complete main output and export it as one neutral stereo USTX audio track, preserving the rendered sound while intentionally flattening editing.

Frame-zero-aligned stems start at project zero and include leading silence, which avoids tick/sample rounding shifts. Requested ranges are specified explicitly; filenames/media refs point to a colocated asset directory. Tail length, sample rate/layout and offline processing policy match the rendered reference. Export rejects unavailable required assets/backends; it does not select a replacement instrument.

Reimporting a standalone USTX uses its audio parts and never guesses original MIDI/backend data. Reopening `.rustx` uses its explicit extension/identity linkage. These are distinct, predictable lifecycle outcomes.

## 12. Errors and examples

The inherited error envelope remains `{error:{code,message,details}, revision}`. UI branches on `code`. New errors include `stream_not_found`, `stream_closed`, `stream_busy`, `duplicate_request`, `stream_limit_exceeded`, `backend_unavailable`, `plugin_state_rejected`, `state_capture_pending`, `asset_unavailable`, `cursor_stale`, `catalog_conflict`, `channel_unassigned`, `unsupported_midi_channel`, `unsupported_midi_event`, `preset_unavailable`, `backend_render_failed`, `export_requires_bake` and version/integrity errors. Per-channel failures identify the track/channel/preset and expected asset generation.

```json
{"request_id":"req-05","error":{"code":"asset_unavailable","message":"Required SoundFont is unavailable","details":{"asset_id":"sf2-01","track_id":"track-01","retryable":true,"stream_open":false}},"revision":12}
```

Rejected project commands preserve document/history/revision/render inputs. Runtime failures report whether the stream remains open and its terminal state. No successful-completion reply may conceal an unresolved instrument, failed processor, dropped MIDI data or substituted silence.

## 13. Capability/conformance phases and task boundaries

| Increment | Required group | Gate and scope |
|---|---|---|
| Base progressive voice/wave playback | `stream.pcm.v1` | T033: revision-pinned ordered PCM, pending/terminal lifecycle, bounded buffers and stopped-edit policy. Approved sequential processing required for existing active FX; otherwise host explicitly reports that combination unavailable. |
| Sequential stateful FX | `stream.sequential_fx.v1` | Coordinate T018/T044; persistent state and no reset per block. Legacy USTX `MixFx` remains source-bounded; only components with a declared tail policy drain post-source. Does not require seek. |
| Seek/checkpoint reconstruction | `stream.seek.v1` | T034 after base streaming: use versioned, session-local FX-state checkpoints; restore the nearest compatible checkpoint and process/discard to the requested frame, falling back to replay from frame zero. Checkpoints are invalidated by render/asset/FX identity or processor-state-version mismatch. Cache is capped at 32 MiB/session with deterministic oldest-checkpoint eviction and close-time clearing. An open-ended start beyond source end terminates with `end` and no PCM. Legacy USTX `MixFx` remains source-bounded; only separately declared new-insert tails may extend output. Source-path ownership and implementation evidence remain required before advertisement. |
| Continuous edit scheduling | `stream.live_edits.v1` | T035; not required for the selected first-version stop/replay behavior. |
| Instrument rendering | Independent backend groups | SoundCraft built-ins, hosted plugins and RustySynth each pass their own vectors. T033 does not require all three backends to be implemented first. |
| New inserts and later routing | Separate mixer groups | T044/owner handoff. Routing/automation do not become implied by insert support. |
| Versioned bundle and USTX bridge | Separate bundle/export groups | Rust codec, asset validation, linkage and explicit bake modes. Not a prerequisite to implementing the base PCM producer. |

T045 reviews the common contract and resolves conflicting requirements before T033 begins. Contract approval is separate from implementation authorization and adapter readiness. T033 still needs its existing T024/T011/source ownership dependencies; the host acceptance matrix is updated to include the Android local-WebSocket adapter explicitly. No unimplemented route or feature is declared ready through this document.

## 14. Concrete conformance vector catalogue

These are required specifications for future tests, not claims that tests already exist or pass:

| ID | Input/scenario | Expected observation |
|---|---|---|
| C01 | Existing track has voice and wave parts | Identity/order/shared fader/legacy FX preserved; no implicit split. |
| C02 | Select SoundCraft builtin synth/drums/sampler | Correct tagged config; sampler asset validated; no external plugin format required. |
| C03 | Switch SoundCraft to RustySynth and undo | MIDI/track/part IDs unchanged; prior config/state restored exactly. |
| C04 | Invalid parameter, note bounds, MIDI value or duplicate event ID | Atomic rejection; identical document/history/revision. |
| C05 | Events at one timestamp and at block end | Deterministic ordering; boundary event occurs exactly once in the next block. |
| C06 | Missing SoundFont/plugin on open | Editable config retained; strict render reports required assets without substitution. |
| C07 | Next deadline expires before voice PCM is ready | Pending; sequence/cursor unchanged; cancel still accepted. |
| C08 | Workers finish out of order | Emitted frame ranges remain contiguous and increasing. |
| C09 | Short block, empty range and natural end | Short data is not EOF; empty ends without data; terminal reply stable. |
| C10 | Concurrent next, duplicate request, cancel and close | Defined typed responses; no duplicate data/mutation; isolated stream cleanup. |
| C11 | Disconnect with queued PCM | Old-generation sink rejects data; resources bounded and new stream required. |
| C12 | Metadata/control/binary lengths mismatch | Bounded rejection before unsafe allocation; no arbitrary decode. |
| C13 | Consumer slower than producer; underrun then resume | Queue bounds enforced; underrun silence does not advance project cursor or alter exported PCM. |
| C14 | Cosmetic rename during playback | Playback continues; pinned revision preserved and Rust reports audio compatibility. |
| C15 | Note/FX/fader edit during playback | Old generation stopped/flushed; explicit Play creates a new snapshot. |
| C16 | Restart after fader/FX change | Eligible source chunks reused; changed downstream processing reapplied. |
| C17 | Sequential FX across blocks, instrument release tails and declared insert tails | Concatenated output matches offline reference under the approved measured tolerance; legacy USTX `MixFx` remains source-bounded. |
| C18 | New insert plus USTX FX | New insert precedes fader/pan; legacy FX remains afterward; no double processing. |
| C19 | Parallel export/playback and native editor capture | Separate mutable instances; committed parameter/state snapshot used; capture conflict cannot overwrite newer state. |
| C20 | Save/open `.rustx` v2 and v1 | Correct version handling and stable linkage; no duplicated authoritative musical fields. |
| C21 | Exclude VB/SF2 and reopen elsewhere | Data editable, unresolved dependencies reported; no fallback PCM embedded/played automatically. |
| C22 | Portable request includes forbidden/unknown asset | Explicit inclusion report/action needed; no inferred rights or hidden inclusion. |
| C23 | USTX compatible/track_bake/mix_bake | Instrument/audio placement and tails retained; loss/bake requirements explicit; fader/FX applied once. |
| C24 | Wrong hash/IDs, unsupported required extension or unsafe bank path | Typed rejection; no overwrite/extraction/guessed linkage. |
| C25 | Each advertised backend/host capability | Real build/assets/audio behavior verified separately; unsupported groups remain unavailable. |
| C26 | Catalogue/preset pages while asset/plugin list changes | Generation-bound cursor or typed stale error; no index-based selection or mixed pages. |
| C27 | SoundFont preset absent or unmappable for a channel | Exact validation failure before PCM; RustySynth GM/default fallback never changes the selected sound silently. |
| C28 | All 16 SoundFont channels with independent programs/CC/bend | Per-channel state retained through save/reopen/undo and render; no cross-channel preset overwrite. |
| C29 | Replace SF2 where one assigned channel preset is missing | Atomic rejection unless explicit unresolved intent; no arbitrary first-preset selection; MIDI IDs retained. |
| C30 | Sampler sample replace, preset apply, state load and undo | Complete validated prior state restored; non-sampler/sample and incompatible component state reject. |
| C31 | Unsupported CC/program on a SoundCraft builtin or VST3 host | Descriptor-based rejection before claiming success; no generic no-op accepted as support. |
| C32 | Arbitrary transport pull lengths with RustySynth block timing | Identical effective event grid and offline PCM; requested/effective frame resolution reported. |
| C33 | Short note quantizes into one backend quantum | Effective note-off is later than note-on; no hung voice; duration extension is diagnosed. |
| C34 | Polyphony/block size/source effects setting changes | Supported construction settings persisted/rebuilt and source invalidated, not silently treated as mixer FX. |
| C35 | Audition panic/disconnect and editor gestures | No stuck notes; handles isolated from exports; editor gesture becomes one Rust transaction without frontend serialization. |
| C36 | SoundFont multichannel content switched to a hosted instrument; nonzero-channel host delivery | Patched capable host preserves channels; independently incapable plugin requires explicit compatible mapping or typed rejection. No silent channel collapse. |
| C37 | Hosted VSTi state plus parameter overrides, native programs and scheduled program messages | State restored first, overrides next; patched supported program mapping reaches the intended plugin/channel; unsupported plugin features reject; committed snapshot matches save/undo. |
| C38 | Hosted plugin needs unsupported event/audio bus or additional outputs | Structured preparation failure or explicit supported mapping; no silent port/output loss. |
| C39 | SoundCraft-hosted note-on/off on nonzero MIDI channel and at block offsets | Channel and frame offset survive every adapter; event at block end is delivered at next block frame 0; no channel-0 collapse. |
| C40 | Hosted scheduled CC/program/bend at equal and adjacent frames | Supported messages preserve channel/order/offset; unsupported mapping fails before PCM and does not become a successful no-op. |
| C41 | Hosted per-channel program/bank selection with missing mapping | Exact plugin/format mapping is used when verified; otherwise preflight returns a typed unsupported/unmappable error without substituting a preset. |
| C42 | Hosted plugin state restore or editor capture fails/conflicts | Existing committed state and history remain unchanged; render instance keeps its pinned generation; diagnostic names the failing component/operation. |
| C43 | Hosted plugin changes sample rate, block size, bus layout or latency at prepare | New generation is accepted only after complete negotiation; unsupported layout rejects atomically; latency/tail metadata matches processing behavior. |
| C44 | Same hosted MIDI/render snapshot through realtime stream and offline export | Rust event trace and state snapshot match; PCM agrees within a measured, explicitly approved per-plugin tolerance or capability declares nondeterminism. |
| C45 | Native editor unavailable or invoked from the wrong thread/host | Host capability reports unavailable or marshals correctly; no fake editor success, deadlock or audio-thread UI call. |
| C46 | Catalogue or plugin changes while a hosted track is unresolved/loading | Generation-bound selection rejects stale results; editable project state remains; runtime readiness transitions are reported without partial config mutation. |
| C47 | A compatible feature is added after the MK3 base, or a client lacks that feature | Existing base behavior and vectors remain unchanged; optional capability may be unavailable without changing base semantics; required unsupported capability fails negotiation; breaking behavior requires a new major and explicit migration. |

Before setting product thresholds, measure representative cold/warm multi-track projects on target devices: first valid block, actual audible start, producer completion, peak memory, queue occupancy, cancellation latency and underruns. PCM comparisons report mismatch count/max/RMS error and approve any tolerance from actual engine/backend results. External nondeterministic plugin output requires an explicit capability/tolerance policy, not an automatic promise of bit equality.

## 15. Remaining review items

The product questions on edit playback, missing-asset fallback, FX placement, initial 16-channel SoundFont support and acceptable RustySynth block timing have been answered and recorded. They are not reopened as unanswered decisions. Backend/channel/preset mapping constraints and timing remain visible capability data, not hidden substitutions.

2026-10-14 completeness audit: user intent is covered by §§1–14 and C01–C47, including the newly selected rule that MK3 is the stable base and future features are additive. The versioning direction is logical contract 3.0; the document itself remains a draft until explicit acceptance. Requirement coverage does not mean implementation-ready or runtime-proven.

Before implementation/advertisement, remaining engineering work is to validate concrete DTOs against Rust serialization, pin processor/backend descriptor versions from the crate audit, assign source owners, run phase-specific vectors, and choose measured host limits and PCM tolerance. SoundCraft hosted VSTi has a capability matrix and C39–C46 acceptance targets, but no adapter patch or runtime capability is complete on the basis of this draft. Android `/api/v3/ws`, framing details and the `.rustx` v2 layout remain proposed pending acceptance and implementation evidence; arbitrary numeric examples are not performance promises.

## 16. Primary references

- [Existing frozen logical standard](../CONTRACT-v1.md#6-project-session-api-draft-v1--implementation-ready-partially-implemented).
- [v2 product readiness record](contract-v2-freeze-proposal.md).
- [T032 feasibility and original candidate API](t032-full-project-streaming-feasibility-wave1.md).
- [Current `.rustx` v1 format](../architecture/rustx-format.md).
- [SoundCraft pinned built-in registry](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/dsp/src/plugins/mod.rs) and [DSP contract](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/dsp/src/lib.rs). Crate audit remains a prerequisite to dependency selection.
- [RustySynth official repository](https://github.com/sinshu/rustysynth): SoundFont MIDI synthesizer; source license MIT. SoundFont assets have independent permissions.
- RustySynth 1.3.6 [synthesizer source](https://docs.rs/crate/rustysynth/1.3.6/source/src/synthesizer.rs), [channel source](https://docs.rs/crate/rustysynth/1.3.6/source/src/channel.rs) and [settings source](https://docs.rs/crate/rustysynth/1.3.6/source/src/synthesizer_settings.rs). Audited crate archive SHA-256: `e3ca93af923df5fc03beddbf464242620fd24daa9e10f9ecd56eb9571eb7ba38`.
