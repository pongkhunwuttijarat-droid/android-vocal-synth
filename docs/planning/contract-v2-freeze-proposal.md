# CONTRACT-v2 Freeze Proposal (Draft for Review)

**Status:** Logical standard frozen in `CONTRACT-v1.md` §6 on 2026-09-30; this document tracks adapter/product readiness and does not claim adapters meet it.
**Date:** 2026-09-30
**Scope:** Identify the remaining decisions and acceptance work needed to freeze the shared UI ↔ Rust Project Session contract as CONTRACT-v2.

**Decision record (2026-09-30):** The user chose to include `part.*`/`project.*` commands in the first required command surface so the editor flow can complete; retain `group_failed` for a failed child command; use native polling as the baseline progress transport and keep WASM async session render capability unavailable until its worker adapter exists. T010 closes after the logical schemas and decisions are fully specified in §6. This does **not** claim full CONTRACT-v2 product readiness; unimplemented commands/adapters stay pending.

### Section-level freeze ledger

| Area | State after T010 | Evidence / remaining gate |
|---|---|---|
| Rust ownership, stable identity/sidecar, snapshots, revisions/history, groups and note/track/curve command semantics | Freeze-ready where §6 labels the behavior FROZEN and the written examples/vectors are complete | Existing identity/session tests, T009 curve parity, T010 shared adapter vectors; actual serializer conformance is an implementation gate |
| `part.*` / `project.*` mutation families | Required in the frozen v2 logical standard (§6.4.4/§6.20) | Implement and run adapter conformance separately (T004) |
| Project lifecycle adapters | Logical requirements decided; native coverage exists; Web/WASM adapter not complete | T004 completes WASM lifecycle, browser storage and cross-adapter round trips |
| Render progress | Native polling baseline decided; WASM async session jobs may report unavailable until worker adapter | Native job tests exist; RenderPool product wiring tracked in T011; full adapter acceptance remains before release |

The §6 logical contract is frozen, including required command behavior. This does not declare CONTRACT-v2 product readiness.

This record summarizes the frozen §6 standard and current implementation evidence in `project-session`, `engine-wasm`, and `synth-server`. It preserves the architectural decisions already made: Rust is authoritative for persistent project/musical state; browser/frontend owns only ephemeral presentation and gesture previews; stable IDs, revisions, grouped transactions and host-specific persistence adapters remain shared logical semantics.

## 1. Summary

**Recommendation:** keep the logical semantics already drafted; do not redesign the contract. The logical specification and examples are frozen in §6; implementation conformance vectors are the next gate. Adapter implementation and execution of conformance vectors follow the frozen standard; missing implementation does not make the written contract incomplete, but no adapter may claim support before it passes.

**Product scope correction (user decision 2026-09-30):** Do not narrow away project-open/create/import/save/export or `part.*`/`project.*` commands just because browser WASM and native HTTP have different transport shapes. A usable editor needs these lifecycle and edit operations. Native should keep using API/routes; WASM should expose typed in-process methods over the same logical semantics. The transports do not need identical URLs/functions. Author the shared vectors as part of the standard; run them against adapters after implementation, and never claim a vector passed before it runs.

The code evidence shows:

- `project-session` already implements identity/sidecar payloads, snapshot DTO, stable-ID note/track commands, `expected_revision`, change results, typed errors, bounded reads, isolated tentative groups and `curve.apply_stroke` (`replace` / `clear`).
- `engine-wasm::Engine` stores the shared `Session` and exposes direct methods for new/open, snapshot/capabilities, v2 commands, groups, history, bounded reads and sidecar payload generation. The old index-addressed facade remains for compatibility.
- Native `/api/v2` session, save, group, bounded-read and render-job routes are present in the current worktree; they are not in this proposal's definition of “verified” until the exact current tests are rerun from the integration checkpoint.
- The mock under `ui-mock/react-dock` is **not an adapter**: it uses local example note/curve/track values and does not load WASM or send session commands.
- The source contract itself records that part/project commands are unsupported, WASM render jobs are unavailable, and several vectors are skipped.

## 2. Decisions to retain as frozen semantics

No new product decision is proposed for these items:

1. **Ownership:** Rust owns canonical document, identity, validation, mutation, undo/redo, serialization and render-affecting state. UI selection, layout, pan/zoom, active tool and transient pointer previews stay frontend-owned.
2. **Gesture model:** preview pointer moves locally; pointer-up submits one semantic commit. A single curve stroke is one mutation/undo step. Raw preview samples can never be saved/rendered/exported before commit.
3. **Identity:** Rust mints opaque stable IDs. Legacy USTX identity is recovered through `lilt.ids/1` sidecar matching; embedded YAML metadata is not used. Import creates a new identity domain; Save As forks project ID and retains entity IDs.
4. **Revision:** starts at 0 per session; increases only for committed document changes, successful undo/redo or replace/import; save/export/render do not increment. Rejected commands change neither document, history, revision nor render input.
5. **Groups:** at most one isolated tentative group per session; public reads see committed state only; child failure discards the tentative group; cancel restores the pre-group state; commit is exactly one undo step and one revision transition.
6. **Mutation responses:** every successful mutation returns revision plus authoritative patch/invalidation changes; full snapshots are supported but are not required after every mutation.
7. **Curves v2:** stable `curve_id`; existing domain representation is authoritative; supported modes stay `replace` and `clear`; Rust validates, snaps/deduplicates/clamps, applies OpenUtau-compatible whole-curve simplification at the single stroke-commit boundary, and returns canonical data. No blend mode or alternative simplifier is added.
8. **Lifecycle:** Native sessions are process-local/non-durable with the decided 30-minute idle TTL and no expiry while a render is queued/running. WASM session lifetime follows the `Engine` object. No restart recovery is promised.
9. **Render:** a job binds to immutable committed revision R; cancellation is per render ID; completion for stale revision is never silently presented as current audio. Native and WASM transport may differ while logical semantics match.
10. **Legacy:** CONTRACT-v1 §0–§5 and the current index-addressed WASM facade keep their existing behavior. v2 adoption is additive until migrated clients no longer call legacy APIs.

## 3. Narrow decisions to resolve before freeze

### D1. Request/result names and discriminator spelling

The Rust implementation and the draft already use `command.type`, `kind`, `authority`, and the typed error codes. Freeze the exact spelling and closed enums directly from the actual Rust DTOs/serializers. Do not introduce alternate aliases such as `command_name`/`name`, `kind`/`type`, or `curve_id`/`abbr` identity.

**Proposal:** canonical wire fields are the ones serialized by `project_session::v2`; add golden JSON fixtures for each request/result/error. Existing prose examples are illustrative and should be corrected if they disagree with Rust serialization.

### D2. Group failure error code

§6.18 notes a possible choice between `group_failed` and `invalid_group_state`. Rust already returns `group_failed` for an accepted child that fails and closes/discards the tentative group; the code is semantically distinct from operations against a nonexistent/closed group.

**Decision:** retain `group_failed` for a child failure, with the group immediately closed/discarded. Use `invalid_group_state` for commit/cancel against an absent or already-closed group. Add/retain vectors proving each case.

### D3. Progress transport

The native adapter currently exposes job polling; WASM has no async session render job surface. This is host transport, not shared document semantics.

**Decision:** polling is the required native v2 baseline. SSE/WebSocket remain optional capabilities. WASM declares `async_jobs:false` for the session render-job adapter until a worker adapter exists; it must not fake async support. A future WASM worker transport implements the same logical descriptor. This leaves direct RenderPool/render functionality to its host-specific implementation task.

### D4. Command surface scope

The v2 command planner currently supports stable-ID note/track commands plus curve strokes. `part.*` and `project.*` are recognized but unsupported; the draft includes part/project change kinds and a logical `import/replace` revision row.

**Decision:** `part.*` and `project.*` mutation families are in the first required command scope, in addition to stable-ID note/track/curve commands. Their exact command schemas, atomicity, revision/history effects, changes and shared vectors must be specified before their implementation is advertised; current planner rejection is a known implementation gap, not the intended final capability. Project `new`/`open`/import/save/export and Save As identity semantics also remain required and map to each host's actual storage/transport adapter. Freeze completed sections where their schema and conformance evidence are complete; do not label the whole CONTRACT-v2 ready while these required families remain unimplemented.

**Draft inventory in CONTRACT-v1 §6.4.4:** project metadata, tempo-map and meter-map edits; voice-part create/remove/move/resize/metadata/split; and a wave-part variant using a logical media reference rather than a host path. Every operation uses stable IDs, `expected_revision`, atomic validation, one committed revision/undo step, group transaction semantics, canonical patch/invalidation data and render invalidation rules. The schemas are proposals, not implemented DTOs.

Contract freeze is independent of implementation completion. The logical rules, edge cases, result DTOs and canonical examples are now frozen in §6; adapters must implement and pass those requirements before the product advertises support.

This is a bounded first-release mutation surface. The project and part operations are required for a complete editor flow; their contract details and Rust implementation remain follow-up work before full release freeze.

### D5. JSON DTO format vs host transport

**Decision:** JSON is the canonical serialized format for Project Session request/result/error/capability DTOs on every host. Native carries those DTOs over HTTP with JSON bodies/responses. WASM remains direct/in-process and does not use HTTP, but its adapter maps to/from the same JSON DTO shapes (current v2 composite calls use JSON strings); raw USTX YAML and sidecar strings remain their documented lifecycle payloads. Function names and call signatures can differ by host, but JSON field names/types/enums and logical outcomes do not. Inline import limits remain host capabilities.

### D6. Capabilities schema

`project-session::v2::capabilities()` currently declares versions/features/command byte limits/stroke points and render flags. Native server adds engine availability and idle TTL. The illustrative JSON in §6.15 combines these fields but does not exactly match the serialized Rust structs.

**Proposal:** publish two layers: (a) a frozen shared logical descriptor from project-session; (b) host capability extensions (`engines`, native idle TTL, host import limits, transport methods). Include golden examples produced by serde and specify unknown-field/version behavior. Each feature name must be listed only if that adapter passes its associated conformance group.

## 4. Must-do implementation/verification gates (not all are spec changes)

These gates test the common logical contract through distinct host adapters: HTTP routes for native/server and direct in-process calls for WASM. A shared vector file is required; identical endpoints/function names are not.

These are conditions for declaring adapter/product readiness after the logical spec freeze:

### G1. Align examples and wire schema

- Generate/copy canonical JSON examples from Rust serialization for snapshot, mutation, group, error, capabilities and bounded reads.
- Correct `CONTRACT-v1.md` §6 prose where examples or “implemented/remaining” summaries drift from actual serialized shapes/routes.
- Ensure native HTTP wraps errors exactly once and WASM returns the equivalent logical error object.

### G2. Shared conformance harness through adapters

- Extend the current runner so vectors run against: `project-session` reference, WASM `Engine` adapter, and native HTTP adapter.
- Keep `skip`/not-run reporting explicit. Freeze requires zero skipped vectors for every **required logical operation**. Optional host capabilities may be not-applicable only when explicitly declared unavailable and the product workflow does not require them.
- Cover revisions/groups/curve semantics/IDs/source/open/save/export/reopen/stale-read behavior and capabilities. The current project-session test runner alone does not satisfy adapter parity.
- Execute parity tests before an adapter advertises support; adding a test harness is not itself completion of parity.

### G3. Project lifecycle round-trips and identity

- Add adapter tests for `new`, `stored_project` open with sidecar, inline USTX import with new IDs, Rust-produced canonical USTX export, save + sidecar write, reopen continuity, and Save As identity fork.
- Native calls these through HTTP/API + host store; WASM calls typed in-process methods + browser store. Verify they normalize to the same logical results and error meanings; do **not** require identical transport/API function shapes.
- Project open/import/save/export are required for the usable editor flow; a host may have a capability unavailable only when the product explicitly accepts a reduced host workflow. For CONTRACT-v2 product readiness, the supported product adapters must pass lifecycle round-trips.
- Keep identity round-trip rules byte-stable with OpenUtau-compatible USTX.

### G4. Render consistency and availability

- Native job route tests must prove immutable snapshot capture, per-job cancel, stale-result behavior, terminal retention/error mapping and TTL protection while queued/running.
- WASM must advertise async jobs as unavailable until a worker/job adapter exists. If CONTRACT-v2 makes async render mandatory for all adapters, this gate blocks freeze; proposal instead treats it as an optional capability with identical logical semantics when advertised.
- Render input must come from the Rust session snapshot, never the React/Pixi mock or raw frontend preview.

### G5. Curve stroke and simplification parity

- Implement the newly accepted OpenUtau-compatible **whole-curve** simplify rule in Rust `project-session` at `curve.apply_stroke` commit; do not simplify frontend preview batches or mutate the committed curve on each batch. Permit only within-tolerance redundant-point removal outside the replaced interval; leave other curves/entities untouched.
- Add vectors for strict tolerance boundary, tie breaking, endpoints, spike preservation/removal, uneven ticks, negative values, affected/outside span behavior, determinism and byte-exact undo.
- Run identical curve fixtures through native and WASM adapters; outputs must be structurally identical, including retained points and invalid-input atomicity.

### G6. Bounded reads and patch payloads

- Verify all returned `authority:"patch"` entries have complete canonical entity data and all invalidations are readable with revision-tagged bounded calls.
- Verify native and WASM reject oversized commands atomically and report consistent logical `payload_too_large`; native HTTP maps the body-limit case to 413.

### G6. Compatibility and regression

- Keep legacy `engine-wasm` project command tests and native legacy endpoints passing.
- Verify new session calls do not mutate legacy state paths unexpectedly and vice versa; reject/synchronize cross-surface mutations deterministically.
- Verify production Web Driver/OPFS migration is tracked as follow-up; do not claim it is already consuming the new session API.

## 5. Freeze acceptance checklist

Contract freeze and implementation readiness are separate milestones.

### Spec freeze — complete the standard

- [x] D1–D6 and the user’s decisions are written into §6.
- [x] Required command families and logical DTOs, including `part.*`/`project.*`, are specified.
- [x] Lifecycle and host mappings are specified without requiring identical route/function names.
- [x] Capabilities, limits, errors and version/unknown-field behavior are specified.
- [x] Canonical JSON shapes/examples and the conformance requirements are written in §6.20/§6.17. Runtime serializer fixtures for new commands remain implementation conformance work.
- [x] v1 compatibility and migration boundaries are clear.

### Adapter/product readiness — implement the frozen standard

- [ ] Rust `project-session` implements the required commands and matches the authored vectors.
- [ ] Required vectors pass against reference, direct WASM `Engine`, and native HTTP with equivalent normalized outcomes; no required vector is skipped.
- [ ] Lifecycle save/export/reopen and sidecar identity pass on native and browser storage.
- [ ] Golden serialization tests confirm actual DTOs match the frozen examples.
- [ ] Native render-job behavior and declared host capabilities match the logical contract; unavailable WASM async jobs are reported accurately.
- [ ] Legacy v1 regressions, workspace tests, WASM target build, server API tests, and adapter conformance commands pass and are recorded.

## 6. Explicit non-blocking follow-ups

These do not block the frozen **logical** contract unless product requirements change:

- `/post-fx` request-body size limit and a versioned large-audio transfer path (not part of v2 session render audio retrieval).
- Render-cache eviction/retention policy.
- Native render thread scaling/performance SLOs.
- Third-party USTX consumer matrix beyond OpenUtau.
- Implementing additional `part.*` / `project.*` mutation commands beyond the declared first required set.
- React/Dockview/Pixi UI polish; layout/presentation does not belong to the Rust session contract.

These are implementation/migration work and must not be misreported as contract completion: migrating the production Web Driver/OPFS path from the legacy facade to WASM Session, implementing/testing save/open/export/reopen in supported adapters, and implementing the required part/project command families remain prerequisites to claim a usable Web product and a complete CONTRACT-v2 release. They do not prevent closing T010's decision-record scope; they do prevent claiming the whole contract is frozen/release-ready.

## 7. Recommended next coding slices

1. Add golden JSON serialization tests for snapshot, mutation/change, typed error and capabilities output. This locks D1 before client adapters depend on spellings.
2. Add the OpenUtau-compatible simplifier to `project-session` at curve commit; first resolve/test the degenerate zero-length endpoint case in §6.7.2, then add strict-boundary/tie/shape/outside-span/undo vectors before UI gesture work.
3. Add a **WASM conformance adapter runner** in `native/crates/engine-wasm/tests/` that instantiates `Engine`, opens the same fixtures, applies shared command vectors through `session_apply_v2`, calls bounded reads/capabilities and normalizes only explicitly host-specific fields. This reveals adapter drift without coupling it to React, OPFS or frontend-owned project state.
4. Add typed WASM/session methods for required project lifecycle semantics: new, open stored/imported USTX, committed YAML export/save payload plus identity sidecar, reopen, and Save As identity fork. Map the same logical lifecycle vectors onto native HTTP and WASM in-process methods separately; do not add HTTP to WASM.
5. Add a native HTTP vector runner using current router/handlers and execute the same required vectors through reference session, WASM and native adapters. Unsupported/optional capability cases remain explicit not-run/not-applicable, never pass.
6. Test client stale-read/cache revision handling at the adapter boundary; this is a behavior check, not a frontend-owned project model.
7. Implement the required part/project command families and their cross-adapter vectors, then connect the supported Web flow to WASM + OPFS; this is required product migration before claiming the production Web app is engine-backed, but it is separate from choosing a common wire transport.

T010 records and freezes the logical contract. Implementation and product migration are tracked in [T004](../work/tasks/T004-web-editor-product-flow.md) and [T011](../work/tasks/T011-render-pool-product-wiring.md). Do not migrate the React mock to real project state until the WASM adapter gate passes; keep the UI mock-only meanwhile.

## 8. User decisions recorded

The user has clarified the following scope (2026-09-30):

1. **Transport:** Native should use API routes; browser WASM is called directly/in-process. Do not add HTTP to WASM just for API-shape symmetry. Share logical semantics and conformance vectors, map them through host-specific adapters.
2. **Project lifecycle:** open/create/import/save/export are required to make the editor usable; Save As/sidecar continuity must be represented and tested for supported host storage. These lifecycle operations are distinct from `part.*`/`project.*` document mutation commands, which are also required in the first command scope.
3. **Parity tests:** execute shared conformance tests through adapters before freezing; do not postpone parity testing until after freeze.
4. **Curve simplification:** product accepted OpenUtau-compatible simplification; current task T009 records it as pending implementation.
5. **WASM async render:** user expects WASM/render functionality to be extended later; async session render may be capability-unavailable until its worker adapter exists.

6. **First-release mutation surface:** include `part.*` and `project.*` alongside note/track/curve commands; complete their logical command schemas and cross-adapter behavior before full release freeze.
7. **Group child failure:** retain `group_failed` and fail-closed group behavior; use `invalid_group_state` for absent/closed groups.
8. **Progress transport:** native polling is the baseline; WASM async session render remains unavailable until the worker adapter exists.

No product decision remains open in this record. `CONTRACT-v1.md` §0–§5 remains frozen as v1; §6 is the frozen v2 logical standard. CONTRACT-v2 product readiness remains pending until required commands, lifecycle adapters and cross-host acceptance gates are complete.
