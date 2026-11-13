# T028 resolved phoneme targets and overrides — proposal

**Status:** review draft; no public route, capability, or runtime behavior is authorized by this document
**Base:** additive follow-up to `project.controls.v1` in [CONTRACT v3.1](contract-v3.1-project-session-controls.md)
**Owner:** T028, pending source-path handoff

## Problem

The Rust Session stores and returns USTX-indexed `phoneme_expressions` and
`phoneme_overrides`, and the current `note.clear_phoneme_data` command clears
them. `UPhoneme::get_expression` consumes some indexed expressions while
building render flags. No Rust path currently maps `UNote::phoneme_overrides`
into the derived render phonemes, so saved alias/timing edits are not shown to
affect render. UI clients also have no stable target to read or write an
individual resolved phoneme.

`phoneme.compose` is not a substitute for the resolved render sequence: its
single-lyric candidates do not include neighbor-aware CV/VC or part-boundary
selection. Targets must be derived from the final Rust phoneme sequence used
by render planning.

## Proposed logical read

`note.resolved_phonemes` is a revision-tagged, read-only **engine service**
operation. It is not a plain `project-session::Session` method: a Session owns
the document and revision, but does not own the voicebank/phonemizer context
needed to derive the final context-aware sequence. Each host adapter must call
the same Rust pipeline resolver using a pinned Session snapshot, selected
voicebank, phonemizer configuration and committed pronunciation choices.

```json
{
  "project_id": "p1",
  "part_id": "part1",
  "note_id": "note1",
  "expected_revision": 12
}
```

Candidate result:

```json
{
  "revision": 12,
  "note_id": "note1",
  "sequence_fingerprint": "sha256:…",
  "targets": [
    {
      "target_id": "ph1:…",
      "ordinal": 0,
      "universal_phone": "k",
      "resolved_alias": "k",
      "alias_status": "exact"
    }
  ],
  "legacy_indexed_data": "none"
}
```

The fingerprint covers the ordered canonical universal phone sequence and
sequence semantics needed to derive its granularity. It excludes voicebank
alias strings so changing banks or alias resolution does not retarget a
universal phone. A target token is a versioned hash of note ID, sequence
fingerprint, ordinal and universal phone. It is transient and is never saved
to `.ustx` or sent back as a project entity ID. The ordinal is returned for
diagnostics only; commands address `target_id` plus the fingerprint.

The read must use the same Rust-owned phonemizer/render-planning route that
produces render phonemes. Today `synth-cli::pipeline::build_phrase_inputs_with_choices`
derives the final `UPhoneme` sequence and immediately proceeds through timing,
grouping and `PhraseBuilder`; extract/reuse a resolver at that boundary rather
than reimplementing it in the Session or adapters. If the current host lacks
the voicebank, phonemizer, or context required to derive the sequence, return a
typed unavailable error; do not synthesize a frontend sequence or expose a
partial list as resolved.

## Proposed mutations

Both commands use the normal stable project/part/note IDs, expected revision,
atomic Session command planner, history, `note.changed` payload and bounded
render invalidation. They also require the read's current
`sequence_fingerprint` and `target_id`; stale/missing targets reject before
mutation.

```json
{
  "type": "note.set_phoneme_expression",
  "project_id": "p1", "part_id": "part1", "note_id": "note1",
  "expected_revision": 12,
  "sequence_fingerprint": "sha256:…", "target_id": "ph1:…",
  "abbr": "vel", "value": 42
}
```

```json
{
  "type": "note.set_phoneme_override",
  "project_id": "p1", "part_id": "part1", "note_id": "note1",
  "expected_revision": 12,
  "sequence_fingerprint": "sha256:…", "target_id": "ph1:…",
  "patch": {"phoneme": "k", "offset": 4, "preutter_delta": 2.5}
}
```

`value: null` clears the matching expression entry. Override patches distinguish
omitted fields (leave unchanged) from explicit `null` (clear that field); remove
the stored override when all fields are empty. Map the validated target to its
current rendered ordinal only inside Rust, then update the existing USTX index
arrays so `.ustx` round-trips. Reject duplicate/conflicting entries or invalid
descriptor units/ranges atomically. A target never grants authority by itself:
project, part, note, revision and sequence fingerprint all must match.

## Render behavior and tests required

- Apply an alias override before voicebank alias/OTO resolution and use the
  resulting phoneme in the render input. OpenUtau `UPart.UpdatePhonemes`
  applies a nonblank override alias over `rawPhoneme` before `UPhoneme.Validate`.
- Apply `offset` to the generated phoneme's raw position in **ticks**, before
  the safety clamp that keeps it at least 10 ticks before the next phoneme and
  before validation. OpenUtau uses `phoneme.position += o.offset ?? 0`.
- Carry `preutter_delta`, `overlap_delta`, `attack_time_delta`, and
  `release_time_delta` onto the derived phoneme before timing/envelope
  validation. OpenUtau computes effective preutter as
  `max(0, autoPreutter + preutterDelta)`, overlap as `autoOverlap +
  overlapDelta`, and applies attack/release deltas to the envelope's fade
  boundaries in milliseconds. These formulas are source-verified; exact Rust
  numerical parity still needs fixture-backed tests.
- Apply per-phoneme expression values through the resolved target and preserve
  existing descriptor defaults and units.
- Shared command/read vectors must pass the same revision, atomicity, identity,
  undo/redo, save/reopen, alias-change, sequence-change and error cases through
  `project-session`, Direct-WASM and native HTTP.
- Golden renders must prove alias and timing overrides affect actual output;
  snapshot DTO visibility alone is not render evidence.

## Legacy USTX migration decision — unresolved

Legacy USTX arrays contain only numeric indices and no original sequence
fingerprint. After open, the engine cannot prove that an old index still names
the same phone under the current phonemizer/bank context. Proposed default:
show entries as explicitly legacy-indexed and read-only; require clear or
explicit user remapping before stable-target editing. Do not silently attach
old values to a newly resolved sequence. The user has not selected a migration
policy yet; see the T028 task record.

## Ownership and compatibility gates

- This proposal does not change frozen CONTRACT-v1 or implement public MK3/v3.1
  routes. The exact logical operation names, DTOs/errors, capability version,
  and compatibility behavior require T028/T014 review before implementation.
- Rust stable-target/render paths need an explicit T028 source-owner handoff;
  note-properties/Piano Roll integration must be serialized with T026/T027/T029
  owners. T009 continues to own `web/app/curve-stroke-transport.js`.
- Until the API and render work are accepted, keep phoneme override editing
  disabled and keep the current T014 clear-before-retarget guard.

## Host and source-path audit (2026-10-15)

The shared dependency graph supports one resolver implementation: both
`engine-wasm` and `synth-server` depend on the `synth-cli` library, while
`project-session::Session` has no voicebank/render context. The current engine
WASM helper already calls `build_phrase_inputs_with_choices` with its attached
voicebank; the native HTTP session handler is in `synth-server/src/handlers_v2.rs`.
This narrows a possible implementation handoff to:

- Shared resolved-phone derivation and override application before
  `TimingEngine.process`: `native/tools/synth-cli/src/pipeline.rs` (or a new
  submodule under that pipeline).
- Rust-owned stable-target state, validation, revision/history and persisted
  USTX index mapping: `native/crates/project-session/src/{session.rs,v2.rs,views.rs}`
  plus the relevant `commands`/`domain` types.
- Host adapters calling the shared resolver: `native/crates/engine-wasm/src/lib.rs`
  and `native/tools/synth-server/src/handlers_v2.rs` (route registration in
  `native/tools/synth-server/src/server.rs`).
- Shared pipeline/render and host parity tests, followed by a dedicated UI
  handoff for `note-properties-panel.jsx`/`clone-state.jsx` and a new phoneme
  editor component.

This is a read-only path proposal, not ownership assignment or implementation
approval. The relevant working-tree files are dirty/untracked with ownership
unknown; do not edit them until the user assigns the Rust path set. The legacy
index policy and exact DTO/capability still require review before exposing any
operation.
