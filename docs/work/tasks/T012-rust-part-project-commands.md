# T012 — T011 supporting work: Rust Project Session part/project commands

**Status:** done
**Owner:** Codex
**Last updated (UTC):** 2026-10-01T04:52:40Z

## Goal

Supporting implementation for T011: complete the frozen `part.*` and `project.*`
command semantics in shared Rust, then expose and verify them through existing
native and direct WASM adapters. The user's active request remains T011; this
record is a completed prerequisite/handoff, not a task substitution.

## Completion criteria

- [x] Implement all required commands in CONTRACT-v1 §6.4.4 with validation,
  atomicity, stable identity, revision/history and change payload semantics.
- [x] Preserve undo/redo and stable IDs for part operations; split behavior is
  covered for the current voice-part fixture.
- [x] Add exact undo/redo tests for part operations and split curve boundaries.
- [x] Add representative shared vectors for document metadata and grouped part
  metadata; existing shared session vectors cover stale revision, group,
  undo/redo and render invalidation.
- [x] Run shared document/part vectors through reference, native, and direct
  WASM adapters; bounded project/part read tests cover both adapters.
- [x] Refresh stale contract implementation status accurately; explicitly list
  exhaustive per-command error/no-op vectors and WASM host storage as follow-up.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base HEAD: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`.
- The checkout has extensive pre-existing/concurrent edits. Files for this task
  are not presumed owned unless changed in this task; inspect diffs before edits.
- Relevant existing surfaces: `native/crates/project-session/src/{session,v2,views,identity}.rs`,
  `native/crates/commands/src/{part_commands,document_commands}.rs`,
  `native/crates/engine-wasm/tests/shared_adapter_vectors.rs`, and
  `native/tools/synth-server/src/handlers_v2.rs`.

## Progress

- 2026-10-01T03:00:46Z: Started at user direction to finish Rust contract gaps before further Web integration. Inspection confirmed all six required project/part command families routed to `unsupported_operation`.
- 2026-10-01T03:19:39Z: Implemented all §6.4.4 project metadata/tempo/meter and voice/wave part command families in shared `project-session`, with cloned-project validation before atomic commit, revision/history, stable identities and change payloads. Added schema-v2 project/part snapshot fields and stable wave-part IDs. Added bounded `project_json`/`part_json` Session reads, native HTTP routes, and direct WASM methods. Web UI was not changed.
- 2026-10-01T03:35:18Z: Added cross-adapter vectors for project metadata and grouped part metadata; both run against the reference Session, direct WASM Engine, and native HTTP handlers. Added split-curve boundary and exact undo tests across all three, fixed wave `part.move` support to match the contract, and confirmed grouped document commands remain tentative until commit. Updated the stale §6 implementation summary.
- 2026-10-01T04:52:40Z: Corrected task ownership after user clarification: T012 is supporting implementation under the active T011 Session/RenderPool effort. T011 remains active; this Rust prerequisite is complete.
- This task is complete at the Rust Session/adapters layer. Web/Flutter product wiring remains tracked separately; exhaustive project/part-specific invalid/no-op vectors remain a follow-up, not a claim of full §6 conformance.

## Verification

- `cargo test --manifest-path native/Cargo.toml -p project-session` — passed all tests, including 7 document-command tests and 6 conformance-vector tests.
- `cargo test --manifest-path native/Cargo.toml -p engine-wasm --test shared_adapter_vectors` — passed (5 tests), including document/part group vectors, bounded reads, and split curve boundary/undo.
- `cargo test --manifest-path native/Cargo.toml -p synth-server --test api_v2_sessions` — passed (29 tests), including document/part group vectors, bounded reads, and split curve boundary/undo.
- `git diff --check` — passed.
- `cargo fmt --manifest-path native/Cargo.toml -p project-session -p engine-wasm -p synth-server` — completed. The checkout has many unrelated dirty Rust files, so review the working-tree diff before committing.
- Existing compiler warnings remain in unrelated renderer/server crates.

## Blockers and next step

- No blocker for this task. Handoff to the next product-wiring task: connect these shared commands and bounded reads to the intended host flow. Remaining non-wiring conformance follow-up is exhaustive invalid/no-op parity for every project/part command; native render jobs are available, while WASM async render and browser OPFS sidecar I/O remain host integration work.
