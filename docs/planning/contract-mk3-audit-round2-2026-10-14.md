# MK3 audit round 2 — instrument control coverage

**Date:** 2026-10-14  
**Reviewer:** Codex  
**Scope:** the revised MK3 draft, especially SoundCraft VSTi/builtin and RustySynth/SoundFont control paths.  
**Outcome:** the prior draft had basic backend/parameter commands, but not a complete discover/select/control/editor/audition API. This round adds those logical control contracts and corrects assumptions found in library source. Contract acceptance and actual backend compatibility remain unverified.

## Product decisions supplied during this audit

1. SoundFont must support multiple presets through all 16 MIDI channels in the first release. The old single-preset draft configuration is replaced with independent channel bindings/controller defaults. One SF2 asset per track remains the initial model; per-channel SF2 assets are not silently implied.
2. RustySynth block timing is acceptable when the actual resolution and requested/effective event frames are declared. Sample-accurate behavior is not claimed for the unmodified library.

Earlier decisions remain: stop/replay after audible edits; relink missing assets without automatic audio fallback; new track FX before fader/pan, followed by legacy USTX FX.

## Findings and draft-level corrections

| ID | Finding in the pre-audit revised draft | Correction in MK3 |
|---|---|---|
| A2-01 (P1) | Generic `set_backend`/`set_parameters` did not define catalogue/preset browsing, parameter reset, sampler binding replacement, opaque state restore or track readiness. | §4.3–§4.4 specifies read methods, exact control commands and revision/catalogue/asset-generation validation. |
| A2-02 (P1) | RustySynth config stored a single bank/program/channel, conflicting with the user's clarified multitimbral requirement. | §3/§4 stores up to 16 independent bindings, preset/CC/bend state, assignment/enable status and source render settings. SoundFont replacement validates all bindings atomically. |
| A2-03 (P1) | Merely validating an SF2 preset catalogue did not prevent library fallback when recorded program/bank events selected a missing preset or native percussion mapping differed. | §4.5 requires exact per-channel mapping and preflight of the effective preset at each note-on. Unmapped/missing selections fail instead of allowing the library's GM/default substitution. |
| A2-04 (P1) | Calling RustySynth `render` with short output slices does not make its MIDI events sample-accurate: internally synthesized blocks may already contain unconsumed PCM. | §4.1 declares backend event quantum, a project-zero-anchored grid, internal-block scheduling/FIFO independent of transport pull size, and requested/effective-frame diagnostics. |
| A2-05 (P1) | Independently rounding a short note's start/end to one quantum can put its note-off before its note-on in the tie order, leaving it held. | §4.1 enforces at least one quantum of effective duration and reports the release extension. C33 covers the case. |
| A2-06 (P1) | SoundCraft's generic MIDI method and CLAP/VST3 format labels do not establish support for all MIDI messages/channels. | §4.5 explicitly declares per-component message/CC/channel capabilities and rejects unsupported messages. The pinned VST3 source emits channel-0 notes and does not implement MIDI Program Change through `midi()`. |
| A2-07 (P2) | Native editor and audition were described as optional concepts without request/handle/close/panic semantics. | §4.6 defines open/note/midi/release/panic/close and native editor handles; no document mutation for transient operations, and captured editor edits use Rust transactions. |
| A2-08 (P2) | Hosted VSTi readiness did not explicitly validate instrument role, event input, audio layout/ports or state/parameter restore order. | §4.8 requires preparation validation, instance-private state and explicit port/mapping capabilities. Preset enumeration/native program selection is optional and not assumed to exist in the generic SoundCraft trait. |
| A2-09 (P2) | Construction settings, internal source effects and current runtime programs could be confused with mixer FX or saved defaults. | §3/§4 separates persisted options and initial bindings from derived playback state and mixer FX. Controller/program state persists across parts; enabled channels and missing bindings have explicit outcomes. |

New specification vectors are C26–C38. They are test requirements, not passing test results.

## Library source evidence

SoundCraft reference: commit `c0cb99340891d306e216582db465f3594b89dc1d`.

- [DSP Plugin interface](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/dsp/src/lib.rs): `prepare`, parameter control, sample binding, note/MIDI events, state reader and editor are separate operations; MIDI has a default no-op implementation.
- [Builtin synth](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/dsp/src/plugins/synth.rs): handles a limited MIDI subset. [Drum synth](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/dsp/src/plugins/drums.rs): one-shot note-off behavior. [Sampler](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/dsp/src/plugins/sampler.rs): explicit sample binding and note offsets.
- [VST3 plugin adapter](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/vst3-host/src/plugin.rs): `midi()` handles selected CC/bend/pressure mappings, with no Program Change branch. [VST3 event conversion](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/vst3-host/src/ffi.rs): note events use channel 0 and event bus 0.
- [CLAP adapter](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/clap-host/src/plugin.rs) and [event conversion](https://github.com/storytold/soundcraft/blob/c0cb99340891d306e216582db465f3594b89dc1d/crates/clap-host/src/ffi.rs): native CLAP note path uses channel 0; raw MIDI forwarding is separately gated.

RustySynth reference: published crate 1.3.6, archive SHA-256 `e3ca93af923df5fc03beddbf464242620fd24daa9e10f9ecd56eb9571eb7ba38`.

- [Synthesizer source](https://docs.rs/crate/rustysynth/1.3.6/source/src/synthesizer.rs): `process_midi_message` supports specific CC cases, program and bend; `note_on` falls back when a preset is absent; `render` caches internal blocks and requires equal channel-buffer lengths; backend master gain initializes to 0.5.
- [Channel source](https://docs.rs/crate/rustysynth/1.3.6/source/src/channel.rs): channel 9 is percussion; bank selection adds 128 there. CC 32 has no handler in the synthesizer's MIDI dispatch.
- [Settings source](https://docs.rs/crate/rustysynth/1.3.6/source/src/synthesizer_settings.rs): defaults are block size 64, polyphony 64 and internal reverb/chorus enabled; allowed source ranges are block size 8–1024, polyphony 8–256 and sample rate 16,000–192,000. These are library limits, not Rusted UTAU performance guarantees.
- [Preset API](https://docs.rs/rustysynth/1.3.6/rustysynth/struct.Preset.html): exposes names, bank and patch. The state/config/undo/preset wrapper in MK3 is Rusted UTAU's proposed adapter contract rather than an existing RustySynth UI API.

## Verification and limits

- Repository branch/base remains `wasm-native-rust` / `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- Read-only source download used `https://crates.io/api/v1/crates/rustysynth/1.3.6/download`. Python `urllib.request` downloaded the archive, `hashlib.sha256` recorded its digest, and `tarfile` copied only selected source/manifests into `/tmp/rustysynth-audit-1.3.6` without extracting archive paths. This is source inspection, not dependency installation/build/inference.
- Documentation checks parse all JSON examples, resolve local Markdown file links and check balanced fences. Exact repeatable command/results are recorded in T045. These checks do not validate actual Rust serde types or plugin behavior.
- No product source, Cargo manifest, frozen contract, native plugin installation or SoundFont asset was modified. No backend runtime/audio tests were run. Platform/channel mapping, actual scheduling error, memory/polyphony behavior and native editors need per-host execution evidence before capability advertisement.

## Remaining gates

The initial control surface is specified for SoundCraft built-ins/hosted instruments and multitimbral SoundFont tracks. Remaining work is exact Rust DTO/descriptor validation, implementing/proving each capability slice, host limits/timing tests, and final contract acceptance. Source-library restrictions are visible capability limitations, not unresolved permission requests or promises that every VSTi already works.

## Integration interpretation update — 2026-10-14T09:21:28Z

The user clarified that SoundCraft is expected to receive library/adapter modifications. Its audited channel-0 and Program Change gaps are therefore baseline integration findings to address, not permanent limitations imposed on MK3. Revised §4.8 and C36/C37 specify preserving channel-aware events, verifying format/plugin program mapping and supplying Rust control services. Actual plugin/platform limitations remain capability-gated. No SoundCraft source patch or dependency change is claimed by this document update.
