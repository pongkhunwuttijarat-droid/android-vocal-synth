# T046 — SoundCraft hosted VSTi adapter completeness

**Status:** blocked — isolated channel patch is ready for review; upstream integration and product ownership remain unresolved
**Owner:** unassigned
**Last updated (UTC):** 2026-10-15T05:09:54Z

## Goal

Extend SoundCraft's hosted-plugin path as needed so Rusted UTAU can expose an honest, Rust-owned VSTi capability surface across supported hosts. Use [T045](T045-streaming-contract-mk3-review.md) as the proposed logical contract and keep Rusted UTAU's Project Session authoritative. This task covers hosted-instrument integration; it is separate from SoundCraft DSP effect reuse in [T044](T044-mixer-streaming-and-soundcraft-reference.md) and base voice/wave streaming in [T033](T033-session-streaming-product-integration.md).

The audit reference is SoundCraft commit `c0cb99340891d306e216582db465f3594b89dc1d`. Its observed channel-0 note path and VST3 Program Change gap are starting findings, not an assumption that every plugin supports multitimbral programs.

## Work breakdown

### A — Confirm upstream boundary and ownership

- Pin the SoundCraft crate/revision and enumerate the exact CLAP/VST3/AU crates used by each target host.
- Decide whether to use an upstream contribution, a maintained fork/patch set, or a narrow Rusted UTAU adapter. Record provenance, licenses/notices and update strategy.
- Establish owners for SoundCraft host crates, Rusted UTAU adapter/session code and host-specific native UI. Do not edit dirty/unowned source files.
- Initial target decision (user, 2026-10-14): desktop-native VST3 first. Android and Web/WASM report unavailable with explicit reasons; do not extend SoundCraft's desktop host claim to those targets. Verify dependency/MSRV/edition and build feasibility for the selected desktop host separately.

### B — Event and MIDI bridge

- Extend the internal event representation to carry logical channel, event kind/data, deterministic order and frame offset within the prepared audio block.
- Preserve channel and offset through note-on/off plus supported CC, bank/program, pitch bend and pressure events in each format adapter. Return explicit unsupported/mapping failures; remove successful default no-op behavior for claimed controls.
- Verify program/bank conversion per format/plugin. Keep recorded Program Change separate from host preset/state application. Do not advertise 16-channel or multitimbral behavior from the event schema alone.
- Document note identity, overlapping same-key note release, release velocity, controller reset/chase and event-queue bounds.

### C — Component, parameter and preset control

- Expose stable format/component identity, instrument role, catalogue generation and per-host availability.
- Define descriptor reads/writes for stable parameter IDs, units, ranges, defaults, choices, writability/automation support and initialization-only controls.
- Enumerate native programs only when the format/plugin API provides reliable metadata. Provide host-managed preset snapshots separately and preserve opaque state where needed.
- Ensure selection, parameter edits, preset application and reset are validated and committed through Rust transactions with undo/redo and asset/catalogue generation checks.

### D — State, editor and runtime lifecycle

- Add control-thread services for instantiate/prepare, state inspect/load/capture, parameter synchronization and optional native editor open/close.
- Restore opaque state before canonical parameter overrides; capture editor changes as one Rust-owned commit. Failed or conflicting capture must leave committed document state intact.
- Keep render instances generation-private and immutable during processing. Construct, restore and retire outside the time-critical audio callback; bound queues and expose typed readiness/failure diagnostics.
- Report editor support per component/host. WebView/WASM/headless hosts must return unavailable unless an actual compatible editor surface exists.

### E — Audio preparation and parity evidence

- Negotiate instrument role, event input, output buses/layout, sample rate, maximum block, latency and tail before marking an instance ready.
- Define prepare/reprepare/reset/release and plugin-failure behavior. Reject unsupported ports/layouts before publishing audio.
- Route interactive playback and offline export through the same event conversion and order; compare event traces and PCM according to a measured, explicitly approved per-plugin tolerance.
- Produce a component/host capability matrix. Advertise only the tested combination and report unavailable combinations with a reason.

## Acceptance criteria

- [ ] Mapping exists from every T045 SoundCraft-hosted VSTi matrix row and C39–C46 vector to an implementation item and evidence result.
- [ ] Nonzero MIDI channel and sub-block frame offsets survive each adapter path that advertises them.
- [ ] Unsupported MIDI/program mappings fail explicitly before corresponding PCM; no silent channel collapse, no-op success or preset substitution.
- [ ] Parameter descriptors, preset/state operations, editor capture and backend selection round-trip through Rust-owned revision/history semantics.
- [ ] Audio preparation reports supported buses/layout, sample rate/block, latency and tail; unsupported configurations fail before readiness.
- [ ] Streaming and offline render use the same pinned configuration and event schedule for every advertised backend.
- [ ] Native, Android and Web/WASM support claims are recorded independently, with tested component/plugin identities and known limits.
- [ ] SoundCraft dependency/fork provenance, license notices and update procedure are recorded.

## Dependencies and blockers

- T045 accepted the MK3 logical 3.0 implementation baseline. Hosted-instrument implementation remains separate from T033 and requires the source/host owner and per-format fixtures below.
- Implementation owner and source-file ownership must be confirmed because the current checkout has many modified and untracked files with unknown ownership.
- T033 remains scoped to base voice/wave streaming and is not blocked on completing hosted VSTi support; instrument capability is a separate increment.
- T044 owns mixer block/FX reuse decisions. T046 must not import SoundCraft's session/project model or conflate its plugin host with the mixer FX processor API.

## Target and fixture decision (2026-10-14)

- The user selected desktop VST3 as the first supported-host target; Android and Web/WASM are explicitly out of scope for initial availability and must surface as unavailable.
- Candidate integration fixture: Steinberg's official [`note_expression_synth`](https://github.com/steinbergmedia/vst3_public_sdk/tree/master/samples/vst/note_expression_synth) sample, which is an instrument sample and exercises MIDI notes/note expression. `AGain` is an effect and is not an adequate VSTi fixture. The candidate has not been built or run through SoundCraft; pin the SDK commit and verify the applicable SDK license/redistribution terms before treating it as an acceptance fixture or shipping its binary. Steinberg's current [public helper/sample repository](https://github.com/steinbergmedia/vst3_public_sdk) lists MIT, while its [VST3 licensing guidance](https://github.com/steinbergmedia/vst3_dev_portal/blob/main/src/pages/VST%2B3%2BLicensing/What%2Bare%2Bthe%2Blicensing%2Boptions.md) distinguishes GPLv3 and proprietary host/plugin distribution. The test fixture is a development candidate only; it is not approved bundled content.
- CLAP, AU and plugin editor support are not first-release acceptance targets unless separately approved. Keep their capability results out of the desktop VST3 claim.

## Read-only host source audit (2026-10-14)

Pinned checkout: `/tmp/soundcraft-readonly-audit`, commit `c0cb99340891d306e216582db465f3594b89dc1d`; checkout is read-only and clean.

- CLAP host is edition 2024 / Rust 1.90 and depends on `clap-sys` + `libloading` off WASM. Its event queue is bounded by `EVENT_CAP`; the queue push path silently declines events after capacity. Overflow therefore needs a typed failure/diagnostic before the adapter can satisfy T045's no-silent-loss rule.
- VST3 host is edition 2024 / Rust 1.90 and depends on `vst3` + `libloading` off WASM, with platform-specific Objective-C/Windows bindings. `NoteEvent::{On,Off,Pressure}` currently writes `channel: 0` in `crates/soundcraft-vst3-host/src/ffi.rs` (around lines 990–1000), so nonzero logical channels are not preserved by this event bridge. Its bounded event queue also silently declines pushes at capacity. Both are concrete implementation defects for advertised multichannel/robust event support.
- VST3 host documents plugins as in-process and unsandboxed: a plugin crash can terminate the host. Linux editor windows are not hosted; sidechain and `IMessage`/`AttributeList` support are incomplete; Web/WASM host support is a stub. These are host capability limits and should be reported per host rather than generalized as VSTi support.
- AU host is AUv2/macOS-only and uses native FFI. It adds no Android/Web capability and is not an early target absent an explicit macOS requirement.
- These facts are from source inspection only. No upstream code was edited, and no plugin build, test, or named fixture was run. Rusted UTAU's corresponding host/adapter source remains dirty/unowned in this checkout, so no implementation patch is safe yet.

## Runtime fixture evidence (2026-10-14)

- SoundCraft ships its own tiny VST3 test bundle under `crates/vst3-host/tests/fixture`; its manifest identifies it as a test-only `MIT OR Apache-2.0` crate and says it is never shipped. It provides a gain effect and a simple instrument (`Fixture Synth`).
- This is enough to establish a bounded native host smoke baseline without adding an external SDK fixture: SoundCraft's integration test builds/installs the bundle in a temporary directory, scans it, instantiates both classes, processes audio/parameter state, and exercises note-on/off offsets.
- The original pinned baseline fixture did not verify a third-party commercial/free plugin, nonzero MIDI-channel preservation, Program Change support, typed queue overflow, or Rusted UTAU Project Session integration. The isolated follow-up patch below adds channel-preservation coverage; the other gaps remain open. The external Steinberg `note_expression_synth` candidate remains optional follow-up if more complete note-expression/third-party coverage is needed; it is not a prerequisite for recording the bounded host smoke result.

## Isolated VST3 channel patch (2026-10-14)

- Prepared a reviewable patch against SoundCraft `c0cb99340891d306e216582db465f3594b89dc1d` in `/tmp/soundcraft-vst3-work`, branch `codex/t046-vst3-midi-channel`. The clean audit checkout `/tmp/soundcraft-readonly-audit` was not modified. Patch artifact: [T046 SoundCraft VST3 channel patch](../patches/T046-soundcraft-vst3-channel.patch); SHA-256 `813b5f4e2826c08f300b2d132eea6c1fc83a5d7354a2c9b70db3f9d55f0d343b`.
- The patch adds channel-aware note-on/off hooks with typed rejection errors to `soundcraft-dsp`, serializes MIDI channels into VST3 note-on/off/poly-pressure events, tracks held notes per channel for all-notes-off, and extends SoundCraft's own instrument fixture so wrong-channel note-off remains audible and fails the test. The VST3 channel-aware note methods now reject invalid channel/note/offset, missing event input, and a full bounded queue explicitly.
- Verification in the isolated clone: `cargo fmt --all -- --check` PASS; `cargo test -p soundcraft-vst3-host --test fixture_e2e` PASS (12/12); `cargo test -p soundcraft-vst3-host --lib` PASS (14/14); `cargo test -p soundcraft-dsp --lib` PASS (36/36); `cargo check --workspace` PASS; `git diff --check` PASS. The patch also passes `git apply --check` against the clean pinned audit checkout.
- The VST3 adapter preserves channel and frame offsets for note-on/off, and raw FFI conversion preserves channel 15 for note-on/off/poly pressure. The added SoundCraft Mix change now forwards MIDI note channels/offsets through `note_on_channel`/`note_off_channel`, keeps held-note bookkeeping per channel, rejects a block when the plugin rejects a note event, and exposes the typed reason in `StripMeter.plugin_failures.event_error`. It still does not implement channel-aware CC/program routing, typed error results for legacy direct `note_on()`/`midi()` callers, overlapping same-key note identity/release velocity, or Rusted UTAU Session integration. It makes no multitimbral product claim.
- Verification after the Mix change: `cargo test -p soundcraft-mix --lib` PASS (41/41); `cargo test -p soundcraft-vst3-host --test fixture_e2e` PASS (12/12); `cargo fmt --all -- --check` PASS; `git diff --check` PASS. Both focused new Mix tests pass (channel+offset preservation; unsupported-channel failure rejects the block). The patch passes `git apply --check` against the clean pinned audit checkout. Patch SHA was recomputed from both the saved artifact and `git diff --binary` as `813b5f4e2826c08f300b2d132eea6c1fc83a5d7354a2c9b70db3f9d55f0d343b`.
- Repository handoff checkpoint: main project branch `wasm-native-rust`, base `e6af244abbf04d4a490fca8483295ed6418e6cad`. Main checkout has many unknown-owner dirty/untracked files; no product source was changed. The patch is an isolated proposal, not an upstream contribution, vendored dependency, merge, or publication.

## Blocker resolution / next concrete step

The T045 baseline is accepted, so contract acceptance is no longer a blocker. The user selected desktop VST3 first; SoundCraft's fixture verifies the host's channel-aware note API, and Mix now routes note channel/offset and publishes typed rejection in its meter. Remaining gates: (1) assign ownership for the upstream/fork vs local adapter and Rusted UTAU integration files; (2) select/approve a third-party VST3 fixture only if required beyond SoundCraft's own test fixture; (3) route controller/program events by channel and report unsupported mappings/queue overflow; (4) define overlapping same-key note identity and release velocity; (5) integrate into Rusted UTAU Session and prove playback/export parity. Review/apply the isolated patch only after repository ownership is assigned. Do not claim full T046 acceptance.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e6af244abbf04d4a490fca8483295ed6418e6cad`
- Relevant artifacts: T045 proposal/review, T044 mixer roadmap, pinned SoundCraft audit in `/tmp/soundcraft-readonly-audit` (read-only source checkout), and the durable isolated patch linked above.
- Working tree contains unrelated dirty/untracked source and project files of unknown ownership. No source file, dependency, public API or lockfile in the main Rusted UTAU checkout was changed for T046.

## Verification and next step

- Planning/read-only source audit plus an isolated SoundCraft patch and fixture test run. No plugin outside SoundCraft's test fixture was loaded, and no Rusted UTAU adapter/session integration was run.
- `cargo check --manifest-path /tmp/soundcraft-readonly-audit/Cargo.toml -p soundcraft-vst3-host` — PASS on Linux x86_64 at pinned SoundCraft commit `c0cb99340891d306e216582db465f3594b89dc1d`; checkout remains clean. This verifies crate compilation only, not loading or processing a VST3 plugin.
- `cargo test --manifest-path /tmp/soundcraft-readonly-audit/Cargo.toml -p soundcraft-vst3-host --test fixture_e2e` — baseline PASS, 10 passed / 0 failed on Linux x86_64. The isolated patched clone now passes 12/12, including typed invalid/unsupported/overflow results and nonzero-channel behavior. This remains SoundCraft fixture evidence, not integration with Rusted UTAU or proof of third-party plugin compatibility.
- 2026-10-14T11:38:04Z Rechecked patch artifact integrity; corrected an earlier stale hash.
- 2026-10-14T11:42:43Z Added the SoundCraft Mix note-event routing and typed rejection surface in the isolated clone. Focused tests PASS 1/1 each for channel/offset preservation and unsupported-channel failure; full `cargo test -p soundcraft-mix --lib` PASS 41/41; VST3 fixture e2e PASS 12/12; fmt/diff checks PASS. Regenerated the patch artifact and verified its SHA against the isolated diff plus `git apply --check` against the clean pinned checkout.
- 2026-10-14T11:43:16Z `cargo check --workspace` PASS after the Mix change.
- Next: resolve ownership for the SoundCraft VST3 fork/host and Rusted UTAU adapter paths; then review/apply the channel patch and make legacy note/MIDI scheduling report queue overflow instead of silently dropping events, followed by remaining MIDI/program routing. Keep Desktop VST3 as the first target; Web/WASM and Android stay unavailable for hosted VST3, even though SoundCraft's built-in DSP/mixer crates have a separate WASM path.


## Isolated patch revalidation (2026-10-15T01:26:54Z)

- Rechecked `/tmp/t046-soundcraft-work`: HEAD is still pinned at `c0cb99340891d306e216582db465f3594b89dc1d`; the six modified files are exactly the isolated channel patch paths listed above. No new files or commits were created.
- Re-ran `cargo test -p soundcraft-vst3-host --test fixture_e2e` — 12/12 passed; `cargo test -p soundcraft-vst3-host --lib` — 14/14 passed; `cargo test -p soundcraft-mix --lib` — 41/41 passed; and `cargo fmt --all -- --check` plus `git diff --check` — passed. `cargo check --workspace` also passed on Linux x86_64 (2026-10-15T01:26:54Z).
- Recomputed patch content: isolated `git diff --binary` and the durable patch artifact both hash to `813b5f4e2826c08f300b2d132eea6c1fc83a5d7354a2c9b70db3f9d55f0d343b`. This verifies the proposal artifact, not Rusted UTAU integration or third-party plugin behavior.
- T046 remains blocked on main-repository path ownership and controller/program routing; the isolated clone remains uncommitted and unpublished.

## Follow-up artifact integrity check (2026-10-15T04:18:24Z)

- Rechecked `/tmp/t046-soundcraft-work`: HEAD remains `c0cb99340891d306e216582db465f3594b89dc1d`; exactly the same six files remain modified and the working diff still has 303 insertions / 64 deletions.
- Recomputed `git diff --binary` SHA-256 and compared it with the durable patch artifact: both remain `813b5f4e2826c08f300b2d132eea6c1fc83a5d7354a2c9b70db3f9d55f0d343b`.
- The prior `/tmp/soundcraft-readonly-audit` checkout was no longer present. Recreated a fresh checkout at `/tmp/t046-soundcraft-recheck-20261015` from `https://github.com/storytold/soundcraft.git`, detached at the pinned commit; verified its clean status and exact HEAD, then `git apply --check docs/work/patches/T046-soundcraft-vst3-channel.patch` passed.
- This verifies applicability against a fresh checkout at the pinned upstream revision, not runtime behavior beyond the isolated test results already recorded and not Rusted UTAU product integration.
- Re-ran on the isolated patched clone: `cargo test -p soundcraft-vst3-host --test fixture_e2e` passed 12/12; `cargo test -p soundcraft-mix --lib` passed 41/41; `cargo fmt --all -- --check` and `git diff --check` passed. These remain SoundCraft fixture/unit results, not Rusted UTAU integration or third-party plugin acceptance.
- No Rusted UTAU product files were edited. T046 remains blocked on source ownership and the remaining MIDI/program, note identity/release velocity, and Project Session integration work.

## Current ownership/blocker recheck (2026-10-15T04:51:30Z)

- Re-read the board and current task. The isolated patched SoundCraft checkout remains at `c0cb99340891d306e216582db465f3594b89dc1d` with exactly the same six modified paths and 303 insertions / 64 deletions; the clean comparison checkout remains at that exact commit. Recomputed patch SHA-256 is still `813b5f4e2826c08f300b2d132eea6c1fc83a5d7354a2c9b70db3f9d55f0d343b`, and `git apply --check` succeeds against the clean checkout.
- No tests were rerun in this recheck; the prior 12/12 VST3 fixture, 41/41 Mix, 14/14 VST3 library, and 36/36 DSP results remain the recorded isolated-clone evidence. No Rusted UTAU source, dependency, or product adapter was changed.
- T046 still needs an explicit owner and integration-source handoff before applying the patch or adding channel/program/state operations. The product decision between carrying a maintained downstream patch and pursuing an upstream contribution also remains open. Exact Rusted UTAU files are dirty/untracked, so do not edit them until assigned.

## MIDI-control path audit (2026-10-15T04:54:20Z)

- Read the current isolated SoundCraft Mix and VST3 code. `soundcraft-midi::CtrlEvent` contains a channel, but `mix::schedule_notes` calls `ctrl_message` without that channel and `ctrl_message` emits channel-0 status bytes. `chase_controllers` also stores only one last value per control kind (131 slots), so controller chase is not channel-scoped.
- VST3 `Instance::midi_mapping()` queries only bus 0/channel 0, and the adapter keeps one mapping table. `Vst3Plugin::midi` silently returns on Program Change and unmapped controls; its mapped-control queue silently stops accepting events at capacity. Poly-pressure queue errors are discarded. The Mix scheduler still uses the void-returning `Plugin::midi`, so these failures cannot reach its existing typed block-failure path.
- This is a read-only audit of the already modified isolated clone; no source or tests changed. It narrows the remaining adapter scope to a typed MIDI-event result, channel-aware controller/program mapping and chase, bounded-queue failure propagation through Mix, and explicit unsupported outcomes. Product/source ownership and the maintained-patch versus upstream-contribution decision remain unresolved.

## Typed MIDI-control patch and verification (2026-10-15T04:59:17Z)

- Extended the isolated SoundCraft patch with a typed `Plugin::midi_event` result. Mix now preserves controller channels during scheduling/chase, reports unsupported/mapping/queue errors through the instrument failure path, and discards a partially scheduled instrument block after an event rejection. The VST3 fixture maps CC7 on channels 0 and 7 and verifies unmapped controller/program failures, channel mismatch, invalid offset, and queue overflow.
- Added checked MIDI queue insertion and explicit validation to the CLAP/AU adapters, plus typed channel-0 controller behavior for the built-in synth. This is host-crate fixture coverage only; CLAP/AU are not first-release product targets.
- Verification on `/tmp/t046-soundcraft-work` at pinned HEAD `c0cb99340891d306e216582db465f3594b89dc1d`: `cargo fmt --all -- --check` PASS; `cargo test -p soundcraft-mix --lib` PASS (42/42); `cargo test -p soundcraft-vst3-host --test fixture_e2e` PASS (13/13); `cargo test -p soundcraft-clap-host --test fixture_e2e` PASS (8/8); `cargo test -p soundcraft-dsp --lib` PASS (36/36). `git diff --check` PASS. The regenerated patch SHA-256 is `dc0cc2bb041811bf57c0624e515f9230d37efd7fd8ad318631113ed52899be51`, and `git apply --check` passes on a clean checkout at the pinned commit.
- The patch now spans nine SoundCraft source/test files and remains isolated, uncommitted, and unpublished. It does not resolve same-key overlapping note identity/release velocity, complete program/bank support, third-party plugin compatibility, or Rusted UTAU Project Session integration. No source files in the main repository were changed.
- T046 remains blocked: source ownership and the upstream contribution vs maintained downstream patch decision are still unresolved. Next step is still explicit ownership/source handoff, then review the patch for application to the selected integration path; do not advertise multitimbral support from these controller-channel tests.

## Workspace-wide compile verification (2026-10-15T05:01:21Z)

- Revalidated the isolated clone at HEAD `c0cb99340891d306e216582db465f3594b89dc1d`; it still has only the nine expected SoundCraft patch files modified. The durable patch SHA-256 remains `dc0cc2bb041811bf57c0624e515f9230d37efd7fd8ad318631113ed52899be51`.
- `cargo check --workspace` PASS across the full pinned SoundCraft workspace; `cargo test -p soundcraft-vst3-host --lib` PASS (14/14). These supplement the prior Mix (42/42), VST3 fixture e2e (13/13), CLAP fixture (8/8), DSP (36/36), and formatting checks. They are isolated SoundCraft compile/fixture evidence only, not third-party plugin acceptance or Rusted UTAU integration.
- Product source ownership, the maintained downstream patch vs upstream contribution decision, note identity/release velocity, and Rusted UTAU Session integration remain unresolved. No main-repository product source was changed.

## Full test attempt / storage cleanup (2026-10-15T05:04:19Z)

- Attempted `cargo test --workspace` in the isolated SoundCraft clone. Compilation reached workspace UI/video tests, then failed while linking because `/tmp` filled (`No space left on device`; `/tmp` was 16 GiB at 99% use). This is an environment-capacity failure, not a reported assertion failure; the full workspace test result is **not verified**.
- Removed only this clone's generated Cargo `target/` directory using `cargo clean` (13.7 GiB reclaimed). The isolated source diff remains exactly the nine patch paths, and the durable patch SHA-256 remains `dc0cc2bb041811bf57c0624e515f9230d37efd7fd8ad318631113ed52899be51`. Earlier focused tests and `cargo check --workspace` remain valid recorded passes; do not claim the full test suite passed.
- No Rusted UTAU source or unrelated temporary files were removed. T046 remains blocked on product-source ownership, patch-maintenance direction and Session integration.

## Full workspace suite with reduced debug artifacts (2026-10-15T05:09:54Z)

- Retried with `CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 cargo test --workspace --no-fail-fast`. The run completed but the workspace is not fully green: `soundcraft-ui-egui --test mix_narrow_headers` and `soundcraft-ui-egui --test recovery` failed; all other reported test targets, including the modified Mix and VST3 fixture suites, passed. Mix is 42/42 and VST3 fixture e2e is 13/13 in this full run.
- Verified both failures on a clean checkout at the exact pinned upstream commit `c0cb99340891d306e216582db465f3594b89dc1d`: `CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 cargo test -p soundcraft-ui-egui --test mix_narrow_headers -- --nocapture` reproduces the expected `S F-J` vs actual `SENDS F-J` failure; the equivalent `recovery` test run reproduces its three failures (`Recover` prompt absent, `session.close` autosave count 0 vs 1, and replaced recovery count 0 vs 1). These are confirmed baseline failures outside the patch's nine changed paths.
- Reduced-debug full run avoided the prior storage exhaustion. The earlier default-debug attempt filled `/tmp`; only the isolated clone's build output was cleaned. The patch artifact and nine-file source diff remain unchanged. Do not report the workspace suite as fully passing.
- No product source was applied to the main Rusted UTAU checkout. T046 still needs source ownership, a patch-maintenance/upstream decision, note identity/release velocity design, and Session integration.

## Current Rusted UTAU integration-surface audit (2026-10-15T05:26:16Z)

- Read-only search of `native/Cargo.toml`, every `native/crates/*/Cargo.toml`, and Rust sources found no SoundCraft, VST3, CLAP-host, or AU-host dependency/integration. The Rusted UTAU workspace currently has no hosted-instrument crate; its existing audio crates include `mixer-fx` and vocal-render backends, which are separate concerns.
- The durable nine-file patch applies to the isolated SoundCraft checkout only (`au-host`, `clap-host`, `dsp`, `mix`, and `vst3-host`). It does not create the Rusted UTAU adapter, instrument-track Session commands/state, or shared realtime/offline render integration.
- This narrows the required handoff: one owner is needed for the SoundCraft patch/upstream boundary, and a separate scoped owner is needed to add the Rusted UTAU hosted-VST3 adapter plus its Session/render integration. Existing project/session/runtime files are dirty and their ownership is unknown, so no source changes were made. The product decision to maintain the nine-file patch downstream or pursue upstream contribution remains open.
