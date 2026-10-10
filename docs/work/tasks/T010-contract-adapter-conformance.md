# T010 — Complete and freeze the Project Session logical standard

**Status:** done
**Owner:** Codex
**Last updated (UTC):** 2026-09-30T15:53:00Z

## Goal

Complete the logical Project Session standard independently from adapter implementation: specify the required command families, lifecycle semantics, DTOs, validation/atomicity, revisions/history, errors, capabilities and examples. Freeze the standard when the written contract is complete; verify implementation conformance afterward in its owning tasks.

## User decisions already recorded

- Native host uses HTTP API routes; browser WASM is called directly/in-process. Share semantics and vectors, not endpoints/function names.
- A usable editor requires project creation/open/import/save/export/reopen. These are distinct from `part.*`/`project.*` document mutation commands.
- Conformance parity gates each adapter advertising support; the user clarified implementation parity does not block freezing the complete written logical standard.
- WASM async session-render jobs remain capability-unavailable until a worker adapter exists.
- `part.*` and `project.*` mutations are required in the first command scope; the normative command inventory is frozen in §6.4.4/§6.20.
- JSON is the canonical serialized representation for logical Project Session DTOs; native uses HTTP+JSON and WASM direct calls map to/from the same JSON shapes.
- OpenUtau-compatible curve simplification is accepted as a requirement, but remains a Rust implementation task (T009); do not mark curve vectors passing until it is implemented.
- User clarified this is a draft-standard pass: work can proceed adapter by adapter. Implement Web UI and the corresponding WASM lifecycle adapter separately when that UI is integrated. The written logical standard is frozen after review; full CONTRACT-v2 product readiness remains pending follow-up adapter parity.

## Completion criteria

- [x] Record the selected first-release command scope, group failure semantics, native progress transport, and WASM async render capability behavior.
- [x] Record canonical JSON DTO encoding while keeping HTTP native-only and WASM direct/in-process.
- [x] Define the full logical semantics and freeze the written contract independently from whether adapters have implemented them.
- [x] Author canonical JSON DTO shapes/examples and normative expected command semantics for all required families, including `part.*`/`project.*`; executable serializer/vector coverage remains adapter follow-up work.
- [x] Allocate implementation follow-ups: production Web/session lifecycle and required command families to T004; RenderPool product wiring to T011.
- [x] Record current reference/native/WASM vector results and explicit `not-run` evidence without describing skipped vectors as passes.

## Implementation/conformance follow-up (outside logical spec freeze)

1. T004 specifies and implements the required `part.*`/`project.*` command families plus WASM lifecycle methods and Web Driver migration.
2. T004 runs lifecycle and command vectors through reference, direct WASM and native adapters; adapters must match the frozen schema before advertising support.
3. Add serializer golden tests and a stale-read adapter-boundary test; resolve explicitly skipped required vectors.
4. T011 completes RenderPool product wiring; keep it separate from project persistence and logical session render-job semantics.
5. Revisit the full release checklist in `docs/planning/contract-v2-freeze-proposal.md` before declaring complete CONTRACT-v2 freeze.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`
- Relevant working tree: checkout has extensive pre-existing and concurrent work with unknown ownership. T010 changes include shared reference/native/direct-WASM command and curve vector runners, native Save As/export routes and lifecycle tests, native render-job TTL coverage, and draft-contract/task-record updates. Newly added WASM Save As/export wrappers were removed after the user clarified WASM lifecycle work should be staged separately. Existing unowned files remain unassigned.
- Durable references: `docs/CONTRACT-v1.md` §6; `docs/planning/contract-v2-freeze-proposal.md`; `native/crates/project-session/tests/conformance_vectors.rs`; `native/crates/project-session/tests/vectors/session_vectors.json`; `native/crates/engine-wasm/src/lib.rs`; `native/tools/synth-server/tests/api_v2_sessions.rs`.

## Progress

- 2026-09-30: Planning task created from user direction.
- 2026-09-30T14:17:37Z: User authorized T009/T010 execution. Added focused curve parity over native HTTP handler and direct in-process WASM, including canonical result and undo comparison.
- 2026-09-30T14:53:22Z: Added shared adapter vectors for note/history/group/read/capability and dense curve operations. Reference, native HTTP handlers, and direct WASM run the shared vectors for currently implemented commands. Added native Save As and portable export lifecycle coverage plus render-job TTL exemption. Per user direction, Save As/export lifecycle wrappers are not being added to WASM in this draft pass; Web UI and WASM lifecycle parity are subsequent adapter work.
- 2026-09-30T14:59:49Z: Assigned the deferred production Web Driver migration and WASM lifecycle adapter/vector work to existing T004. T010 remains the cross-adapter pre-freeze verification gate. RenderPool wiring is kept separate because it owns rendering workers, not project persistence/session identity.
- 2026-09-30T15:04:11Z: User directed RenderPool product wiring into new T011. T010 was focused on resolving contract draft choices and defining separate product gates; implementation ownership for Web lifecycle remains in T004.
- 2026-09-30T15:08:00Z: User selected `part.*`/`project.*` as required in the first command scope, retained `group_failed`, chose native polling as baseline, and kept WASM async session render capability-gated until a worker adapter exists. Updated the decision record and release boundary. T010 records the decision milestone; logical schema completion was still pending at that point, while product readiness remains subject to implementation and adapter gates.
- 2026-09-30T15:27:47Z: User clarified implementation is not a blocker to freezing the logical standard. Reopened T010 to draft the full `part.*`/`project.*` contract first; implementation/conformance stay in T004/T011 and the final verification gate runs last.
- 2026-09-30T15:34:57Z: Drafted §6.4.4 project metadata/tempo/meter and part create/remove/move/resize/metadata/split semantics; added proposed schema-v2 snapshot fields, logical project/part reads, patch behavior and example requests. Draft remains for review; implementation is explicitly separate.
- 2026-09-30T15:38:00Z: Recorded JSON as the canonical serialized format for logical Project Session DTOs across adapters. Native transports JSON over HTTP; WASM remains direct/in-process and maps its calls to/from the same JSON shapes.

## Verification

- `cargo test --manifest-path native/Cargo.toml -p project-session --test conformance_vectors` — passed (4 tests; reference vectors, including 3 explicitly skipped capabilities logged as not-run by the vector runner).
- `cargo test --manifest-path native/Cargo.toml -p project-session --test session_identity` — passed (18 tests).
- `cargo test --manifest-path native/Cargo.toml -p engine-wasm --test shared_adapter_vectors` — passed (2 shared command/curve vector tests).
- `cargo test --manifest-path native/Cargo.toml -p engine-wasm --test project_commands` — passed (8 tests).
- `cargo test --manifest-path native/Cargo.toml -p synth-server --test api_v2_sessions` — passed (26 tests after export coverage; prior full run passed 25 tests before adding the export test; export test also passed independently).
- `cargo fmt --manifest-path native/Cargo.toml -p project-session -p engine-wasm -p synth-server` — passed.
- `git diff --check` — passed.

## Blockers and dependencies

- None for the logical-spec milestone. Adapter implementation and conformance remain open in T004/T011; do not advertise unsupported capabilities.

## Completion

- 2026-09-30T15:53:00Z: Reviewed and froze §6 as the v2 logical standard. Added the canonical JSON DTO catalogue, closed project/part view and validation rules, tempo/meter invariants, wave `media_ref`, split-curve boundary behavior, group/lifecycle/capability shapes, and unknown-field/version rules. Adapter implementation remains a separate milestone.

## Handoff notes

Native route names/status wrappers and direct WASM method names are allowed to differ. The common guarantee is logical behavior and normalized outcomes. Do not start the React mock integration until this adapter seam is tested; do not treat tests run only against `project-session` as cross-host parity.
