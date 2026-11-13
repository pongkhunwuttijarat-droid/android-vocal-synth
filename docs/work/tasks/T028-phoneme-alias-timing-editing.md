# T028 — P1C advanced phoneme, alias, and timing editing

**Status:** blocked — resolved-phoneme stable targets/render consumption and UI source ownership unresolved
**Owner:** unassigned — proposed Phoneme UI lane
**Last updated (UTC):** 2026-10-15T05:13:22Z

## Scope and deliverables

Provide advanced phoneme/alias/timing editing and clear validation/error feedback. The editor may own transient composition text and selection only; phoneme candidates, alias/timing state, commits, save, and render remain Rust-authoritative.

## Existing task relationship

- Reuse completed [T013](T013-phoneme-input-and-mapping.md) composition/mapping and stable-ID commit; do not recreate its IME or alias engine.
- [T014](T014-project-session-openutau-controls-extension.md) defines the v3.1 boundary, readable legacy arrays, explicit clear command, and guard against lyric/pronunciation retargeting. T028 owns the follow-up resolved-phoneme read and stable-target writes. [T018](T018-openutau-flags-and-track-fx.md) owns the separate vibrato/FX work; do not duplicate it.
- Follows P1C in the [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md).
- Read/write and target-identity design draft: [T028 resolved phoneme target proposal](../../planning/t028-resolved-phoneme-target-proposal.md). It is not an approved API or live route.

## Dependencies and blockers

- T014's additive v3.1 implementation is authorized and descriptor/stroke API tests now pass on both hosts. T028 remains blocked because per-phoneme values are currently index-based and render does not consume the stored override path consistently; the shared UI files also have unresolved source ownership. Enable no control whose active HTTP and Direct-WASM paths lack parity.

## Likely files / ownership

A dedicated phoneme editor component under `ui-mock/react-dock/src/openutau/`, `note-properties-panel.jsx`, `clone-state.jsx`, adapter view/command tests, and real-bank fixtures. Coordinate T004/T019/T018 ownership before modifying shared panels or adapters.

## Acceptance and evidence

- [ ] Alias, phoneme index, and timing values are read and committed through the same Rust Session semantics in Direct WASM and HTTP.
- [ ] Composition remains ephemeral until commit; stale revisions, invalid aliases, and bank changes are handled without silent project mutation.
- [ ] Undo grouping, save/reopen, render derivation, and representative supported voicebank behavior have tests/evidence.
- [ ] Unsupported operations remain disabled with actionable reason; do not treat T013 synthetic phonemizer results as real-bank rendering parity.

## Concurrency and approvals

The current UI clone and likely shared panel/test paths are untracked with unresolved ownership. Do not edit them until the owner/handoff is explicit. API-dependent wiring waits for T014 review; shared note-properties components also require coordination with T026/T029.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- 2026-10-14T11:34:33Z Read-only dependency/UI audit: T013 records Rust-side `phoneme.compose` and `note.set_pronunciation` semantics plus native/Direct-WASM save/reopen tests, but T014's extension proposal labels these as outside frozen CONTRACT-v1 and still awaiting review; no approved public phoneme/timing operation is established. Current Piano Roll only toggles labels using `phonemeOf()` from `mock-data.js`; no Session compose/commit call was found in `ui-mock/react-dock/src/openutau/`. That UI tree and shared note panel files are untracked/ownership unresolved. No source or contract files were edited and no tests/builds were run.
- 2026-10-14T11:35:45Z Additional read-only API/data audit: `UNote.phoneme_overrides` persists `index`, optional alias (`phoneme`), tick `offset`, and `preutter_delta`, `overlap_delta`, `attack_time_delta`, `release_time_delta`; the frozen Session command/read surface has no phoneme-override operation or resolved-phoneme read. The render-side `UPhoneme` carries these timing values and `feed::envelope` consumes them, but no mapping from `UNote.phoneme_overrides` into those derived phonemes was found. Therefore T028's alias/timing acceptance needs an explicit stable target/index rule and Rust-owned read/write contract, including whether the stored override fields are in first-release scope; composition/commit alone does not satisfy it. Source inspection only.
- 2026-10-14T11:36:39Z Contract consistency check: frozen CONTRACT-v1 §6.4.4 explicitly includes `note.set_tone`; T014's proposal/checklist still calls it unapproved. Do not ask to ratify `note.set_tone` again—verify its adapter conformance. `note.set_pitch` is absent from the frozen command table and remains a distinct extension-review item. The phoneme composition/pronunciation operations and phoneme-override/timing operations are also absent from frozen §6.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- 2026-10-15T02:41:23Z T014 update: lyric/pronunciation changes now reject while dependent per-phoneme arrays exist; `note.clear_phoneme_data` is the explicit recovery command. The extension specifies expression descriptor units, override offset/timing units, and defers resolved-phoneme stable targets to T028. No UI files were touched. Next: define canonical sequence target IDs, implement resolved reads/writes plus render consumption and adapter parity, then obtain a dedicated UI source handoff.

## Wave 2 read-only render-path audit (2026-10-15T02:54:53Z)

- Confirmed `NoteView` returns the stored `phoneme_expressions` and `phoneme_overrides` arrays (`native/crates/project-session/src/views.rs:114-147`), and `note.clear_phoneme_data` clears them (`native/crates/project-session/src/v2.rs:1076-1079`). This is authoritative persistence/read/clear behavior, not resolved-phoneme targeting.
- `UPhoneme::get_expression` looks up a legacy `phoneme_expressions` entry by abbreviation plus rendered `self.index` (`native/crates/domain/src/phoneme.rs:142-155`); `PhraseBuilder` uses it for SHFT and CLR (`native/crates/phrase/src/builder.rs:188-199,398-402`). Thus some existing index-based expressions affect render, but there is no stable target API.
- Searched all Rust crate sources for `UPhonemeOverride`, `phoneme_overrides`, `preutter_delta`, and `attack_time_delta`. The project-level `UPhonemeOverride` fields are defined/serialized and surfaced in the Session view, but no code maps them into the derived `UPhoneme` before timing or `RenderInput`; the render envelope consumes `UPhoneme` timing fields instead (`native/crates/domain/src/note.rs:284-308`, `native/crates/feed/src/envelope.rs:66,130-133`). Therefore saved USTX override data currently round-trips but its per-phoneme timing/alias changes are not demonstrated to affect this Rust render path.
- This is static source evidence; no files beyond this task record and the board were changed, and no render behavior is claimed. Implementation remains blocked on a reviewed stable target/read-write contract, an owned engine/session/adapter path, and UI ownership for shared panels. Next safe boundary is to specify and assign the Rust stable-target/render integration separately from the shared Piano Roll UI.

## Wave 3 resolved-target boundary (2026-10-15T02:55:37Z)

- T013's `phoneme.compose` read is not itself a resolved render-phoneme view: the existing proposal explicitly excludes neighbor-aware CV/VC and part-boundary alias selection. Stable target IDs must be built from the actual final per-note phonemes produced by the Rust render-planning path, not a frontend reconstruction or an isolated lyric composition candidate.
- Proposed target identity: bind an opaque target token to note ID, canonical universal phone sequence fingerprint, and ordinal; leave bank aliases out of the identity so a singer/voicebank alias change can re-resolve the same universal target. Every write also supplies expected revision and sequence fingerprint; sequence mismatch is a stale-target error with no mutation. This is design guidance, not a ratified DTO.
- Legacy USTX `phoneme_expressions` / `phoneme_overrides` only persist numeric indices and have no stored fingerprint tying them to the original resolved sequence. Automatically attaching those values to today's sequence could retarget them silently. Before implementing T028, decide whether legacy entries should remain readable but read-only until explicit clear/remap, be attached to the current sequence by index for compatibility, or require user mapping.

## Wave 4 review draft (2026-10-15T02:57:12Z)

- Added [the T028 resolved phoneme target proposal](../../planning/t028-resolved-phoneme-target-proposal.md) with candidate revision-tagged read, sequence-fingerprinted target tokens, stable-target expression/override commands, atomic validation, render-stage requirements, cross-host evidence, and a conservative legacy-index migration recommendation.
- No source/API/capability changed. The remaining external decisions are user selection of the legacy USTX migration policy and explicit ownership/review of engine/session/render paths; UI ownership remains separate with T026/T027/T029. The proposal's exact DTOs and render offset semantics still need technical review before implementation.

## Wave 5 OpenUtau override semantics audit (2026-10-15T02:57:53Z)

- Read the checked-in OpenUtau reference `core/OpenUtau.Core/Ustx/UPart.cs:255-275` and `UPhoneme.cs:148-165`. `UPart.UpdatePhonemes` starts from raw position/alias, adds the override offset in ticks, replaces the raw alias when the override string is nonblank, attaches all four deltas, clamps each phoneme at least 10 ticks before the next, then validates. `UPhoneme.ValidateOverlap` adds preutter/overlap deltas to oto-derived values; preutter clamps at zero. `ValidateEnvelope` uses attack/release deltas to shift fade boundaries in milliseconds.
- Updated the T028 proposal with these source-verified semantics. Numerical Rust parity is still untested; no Rust or UI source changed. The stable-target, legacy-index migration, ownership and API review gates remain.

## Current-state recheck (2026-10-15T03:24:07Z)

- Re-audited current Rust sources after T014/T024 changes. `NoteView` still exposes stored index-based `phoneme_expressions` and `phoneme_overrides`, and `note.clear_phoneme_data` remains the explicit clearing operation. No resolved-phoneme target/read API or stable-target write command is present in `project-session`, `engine-wasm`, or native v2 handlers.
- Current Rust render planning derives committed pronunciation choices through `render_pronunciations_for_track`; the source search found no mapping from `UPhonemeOverride` into final render `UPhoneme` timing/alias values. The T024 pronunciation-sample-key helper does not close this gap.
- T028 remains blocked on the legacy index migration policy, reviewed/owned stable-target Session and render integration, and UI handoff. `note-properties-panel.jsx` overlaps T004/T026, so no UI edit was made or claimed.

## Render-boundary recheck (2026-10-15T04:42:46Z)

- Re-read current Rust call paths. `Session::render_pronunciations_for_track` returns only committed sidecar choices aligned by track/part/note; it does not resolve the final context-aware phoneme sequence. `synth-cli::pipeline::streaming` resolves the voicebank and calls `build_phrase_inputs_with_choices`, after which `PhraseBuilder` consumes derived `PhraseGroup.phonemes`. This confirms the T028 resolved-target API cannot be a plain document-only Session read.
- `PhraseBuilder` consumes derived `UPhoneme` values for sample lookup, envelope/timing and render input, while `UNote::phoneme_overrides` is still not mapped into that sequence. A correct implementation needs a shared Rust-owned resolver/service boundary that both native HTTP and Direct-WASM can invoke with the same project revision, voicebank/phonemizer context and final phrase sequence; adapter-local guesses would violate the repository state-ownership policy.
- No source/contract code was changed. T028 still needs the user's legacy-index migration choice and an explicit owner for the shared Session/resolver/render paths; shared UI ownership remains with T004/T026. The new audit narrows the implementation seam but does not make an API or route live.

## Legacy index behavior in OpenUtau (2026-10-15T04:43:28Z)

- Source check of `UNote.AfterLoad`, `UPart.UpdatePhonemes`, and `UPhoneme.GetExpression` found that OpenUtau drops phoneme expressions only when their descriptor is unknown; it does not validate their numeric index against the currently resolved sequence on load. During phoneme update it applies each override by matching the derived phoneme's index; expression lookup likewise matches `UPhoneme.index`. Empty override entries are removed, but unmatched nonempty numeric entries are not remapped to a universal-phone identity.
- This confirms the compatibility risk behind the unresolved choice: attaching legacy arrays to the current sequence by index reproduces OpenUtau's behavior but can change which phone receives a value after recomposition; read-only plus explicit clear/remap is safer but changes editing behavior. No product preference is inferred, and T014's clear-before-retarget guard remains in force.

## Host/service boundary audit (2026-10-15T04:50:32Z)

- Re-read the current dependency graph and call sites. `engine-wasm` and `synth-server` both depend on the `synth-cli` library; `Engine::session_render_sample_keys_json` already calls `build_phrase_inputs_with_choices` with the Session snapshot, committed choices and attached voicebank. The native HTTP session routes live in `handlers_v2.rs`. `project-session::Session` stores document/revision state but does not carry voicebank/render context.
- Corrected the proposal's read operation from a plain Session operation to a shared engine-service operation. The implementation seam is the existing shared pipeline resolver, with host adapters on both sides; Rust Session remains authoritative for persistent commands and undo. The proposal lists candidate source paths, but ownership is still unknown and no source was edited.
- No tests/builds were run. Remaining external inputs: choose legacy numeric-index behavior (compatibility-by-index or read-only/clear-remap), review the logical API/DTO and rendered override semantics, and assign the exact Rust source paths before implementation. UI remains separately owned by T004/T026.

## Stable-identity policy and guard recheck (2026-10-15T05:13:22Z)

- Re-read the current repository policy: frontend intents must use stable project/track/part/note identifiers where available, and Rust remains authoritative for validation, mutation, history and render state. The proposed phoneme commands already use stable note IDs plus a revision-bound, sequence-fingerprinted target; numeric phoneme ordinal is diagnostic only. This supports the proposal's stable-target command shape but does not by itself decide migration of legacy `.ustx` arrays.
- Re-ran `cargo test --manifest-path native/Cargo.toml -p project-session --test pronunciation_session` — PASS (3/3). The tests verify pronunciation/lyric retargeting is refused while legacy phoneme data exists, explicit clearing is atomic, and pronunciation sidecar changes round-trip/undo. This confirms the current safety guard, not per-phoneme target reads/writes or render consumption.
- Recommendation remains: retain legacy numeric-index entries as read-only and require explicit clear/remap before edits. Automatically applying them to a newly derived sequence can silently retarget data; the repository's stable-identity rule reinforces that risk. This remains a recommendation, not the user's selected compatibility policy.
- No Rust/UI/API source changed. T028 remains blocked on the user's legacy-data policy, API/DTO review and exact Rust/UI ownership handoffs; verify shared render consumption and both-host parity after those gates.
