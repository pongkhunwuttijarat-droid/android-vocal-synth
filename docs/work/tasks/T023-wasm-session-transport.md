# T023 — P0B typed asynchronous Session transport

**Status:** done — typed bounded Worker transport implemented and measured  
**Owner:** Codex — user-assigned Worker adapter/runtime/test paths  
**Last updated (UTC):** 2026-10-14T14:12:59Z

## Scope and deliverables

Define and implement an internal typed RPC envelope for the existing Direct WASM Session Worker: request IDs, operation/result/error types, Session revision ordering, stale-response protection, bounded queues/backpressure, lifecycle-safe pending requests, and transferable large buffers where ownership permits. Keep the Rust Session as sole project authority; this is host transport, not a new public Session API.

## Existing task relationship

- Follows [T022](T022-wasm-worker-lifecycle.md); reuses the Worker/session owner established by completed [T020](T020-arrangement-worker-isolation.md).
- Coordinates with [T004](T004-web-editor-product-flow.md) and [T019](T019-openutau-web-ui-parity.md); does not recreate a backend or duplicate Session commands.

## Dependencies and blockers

- The user authorized continuing blocked-task work in this session. T023 still follows T022's startup/error envelope, and the shared Worker files remain dirty/unowned; preserve the serialized T022 → T023 sequence.
- 2026-10-14T12:53:37Z T022 lifecycle acceptance and handoff are complete. The user explicitly assigned the Worker adapter/runtime paths and focused tests to Codex in strict sequence. Implementation may proceed only in those assigned files/tests.
- 2026-10-14T13:01:32Z Initial T023 implementation: added `session-worker-rpc.js`, typed/correlated request/response/error/event envelopes, revision fencing, ordered mutation dispatch, bounded request and payload budgets including queued mutations, AbortSignal caller cancellation, owned-copy media transfer, unique transferable-buffer collection, and shutdown serialization. Focused tests cover ordering, overload/payload rejection, stale events/replies, cancellation, transfer ownership, failure and close cleanup. Actual Direct-WASM page boot reached Rust session rev 5.
- 2026-10-14T13:06:37Z Memory probe using `performance.measureUserAgentSpecificMemory()` in a cross-origin-isolated static build (API aggregates page and workers and requires COI; [MDN](https://developer.mozilla.org/en-US/docs/Web/API/Performance/measureUserAgentSpecificMemory)). Baseline sample: 40,975,103 bytes. A separate Direct-WASM Worker accepted a transferred 32 MiB PCM WAV (33,554,432 sample bytes, caller buffer detached) and returned 380,435.7 ms duration. The aggregate sample after completion was 41,217,761 bytes (+242,658 bytes). The API samples too slowly to capture the transfer/render peak, so this is post-operation evidence only and does not satisfy peak-memory acceptance. The disposable Worker/session and temporary server were closed/stopped.
- Existing command and revision semantics remain governed by frozen [CONTRACT-v1 §6](../../CONTRACT-v1.md); do not edit it. A missing public operation requires separate contract approval.

## Likely files / ownership

`ui-mock/react-dock/src/session-backends.js`, `ui-mock/react-dock/src/openutau/wasm-session.worker.js`, `ui-mock/react-dock/src/openutau/wasm-session-runtime.js`, backend tests, and any shared type module. These overlap T022; one owner/integrator must sequence edits. Do not modify `native/crates/project-session/` unless separately approved.

## Acceptance and evidence

- [x] Every request is typed and correlated; success, domain error, transport failure, stale revision, cancellation, and worker death have distinct outcomes. Cancellation settles the caller and suppresses the eventual response; active synchronous WASM work itself cannot be interrupted.
- [x] Responses cannot overwrite newer project/UI revisions; mutation ordering is explicit and tested.
- [x] Queue and payload limits are measured/configurable; overload rejects clearly. Defaults are 32 in-flight requests and 256 MiB estimated request payload; hosts may configure stricter limits.
- [x] Large PCM/snapshot transfers use transferables only with explicit buffer ownership and no use-after-transfer; record peak memory. Ownership and deduplicated transfer lists are implemented/tested. A live 32 MiB PCM transfer detached the caller buffer as expected; a 380.4 s mixdown/playback retrieval recorded a **728,301,858-byte (694.56 MiB) peak-known buffer high-water** (WASM linear memory plus instrumented known request/audio/result buffers). This is an instrumented allocation inventory, not total browser-process RSS or a universal device budget.
- [x] Tests cover ordering, stale replies, overload, transfer, failure, shutdown, and pending-request cleanup across both backends where applicable.

## Concurrency and approvals

After T022, may run in parallel with UI fixture work, Engine E0, and Streaming feasibility. It must not be implemented concurrently with T022 on the same files. No Session contract, dependency, or Engine/UI integration changes are authorized by this record.

## Wave 1 checkpoint (2026-10-08)

- Classification: T023 remains blocked on T022 and separate implementation approval; no transport/session/public API files were edited. Frozen CONTRACT-v1 remains unchanged.
- Conflict decision: T022/T023 share the same Worker adapter/protocol files and must have one owner; do not start T023 concurrently with T022.
- Next: after T022 handoff and explicit authorization, agree envelope/queue budgets and confirm path ownership.

## Wave 2 ownership checkpoint (2026-10-08)

- DSH coordinator is the single T022/T023 integration/sequencing owner, not a source-code owner. Shared session-backends.js, wasm-session.worker.js, and wasm-session-runtime.js remain dirty/untracked with unresolved file ownership and active T004/T019/T018 overlap. No source edit or implementation authorization was made.
- T023 follows T022 handoff and acceptance; keep the shared Worker boundary serialized. Confirm per-file ownership and explicit implementation/contract gates before transport edits.

## Current authorization checkpoint (2026-10-14)

- The old separate-authorization blocker is superseded by the user's instruction to proceed through blocked items. No new public Session API is implied by that authorization.
- Next, complete T022 and record its envelope, lifecycle, and source-owner handoff. Then implement the internal typed RPC in a confirmed file set, with explicit queue/payload bounds and tests for ordering, overload, transfer, and shutdown.

## Repository checkpoint and next step

- Current checkout (checked 2026-10-14): branch `wasm-native-rust`, starting HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- User-assigned implementation paths now include `ui-mock/react-dock/src/session-backends.js`, `src/openutau/wasm-session.worker.js`, new `src/openutau/session-worker-rpc.js`, `tests/session-backends.test.js`, and `tests/wasm-worker-lifecycle.test.js`. The lifecycle/UI files and docs contain the completed T022 handoff. These paths are untracked in the shared checkout; preserve unrelated untracked work.
- 2026-10-14T11:31:27Z Read-only transport audit: requests were `{id, method, args}` with monotonically increasing local IDs and a `Map` of pending promises. No typed operation envelope, queue/payload bound, backpressure, cancellation, or revision check was visible in the adapter; the worker dispatched `runtime[method]` and executed each async message handler independently.
- 2026-10-14T13:19:22Z Final verification: `node --test tests/session-backends.test.js tests/wasm-worker-lifecycle.test.js` — 24 passed, 0 failed, 1 skipped (`RUSTED_UTAU_SERVER_URL` unset); `npm run build` passed with existing optional-asset and >500 kB chunk warnings. In-app Direct-WASM browser smoke reached “Rust session · rev 5”. A live disposable Worker transferred a 32 MiB PCM WAV, then completed a 16,777,216-frame/380.4 s render in ~15.7 s. A 16 MiB response cap rejected a 134,217,888-byte playback payload (`RESPONSE_TOO_LARGE`); the default cap allowed the same playback take to transfer. Instrumented peak-known memory was 728,301,858 bytes (694.56 MiB), covering WASM memory and known buffers, not browser-process RSS. Browser tab/server were closed. No Android/WebView run or interruption of active synchronous WASM computation was performed.
- 2026-10-14T13:06:37Z Live transfer probe: a separate Worker booted, created an empty Rust session, accepted a transferred valid 32 MiB PCM WAV via `registerMedia`, and disposed cleanly. The caller buffer was detached and the response returned duration 380,435.7 ms. The aggregate memory API sample changed from 40,975,103 to 41,217,761 bytes, but it sampled after the request completed; no peak value can be inferred. Probe did not save or alter a user project.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Next: T023 is complete. T024 may use this scenario-specific instrumented memory observation as a baseline, but must measure its own cold/warm render with pinned assets and define the supported device budget. Keep `CONTRACT-v1` and `native/crates/project-session/` unchanged.

## Wave37 replacement-Session revision regression fix (2026-10-14T14:12:59Z)

- During authorized T025 acceptance, importing `miku-no-shoushitsu.rustx` into a live rev5 Session failed because the new Session's revision restarted at 0 and was treated as stale. The import had not been applied.
- In the user-assigned T023 transport paths, replacement operations (`createProject`, `openStored`, `importUstx`) now carry the prior Session identity; the adapter accepts revision reset only alongside a new identity and an explicitly pending replacement operation. Replies/events tied to an older identity remain stale. The Worker rejects a supplied `baseSessionId` that no longer matches its current Session before dispatch.
- Added a focused sequence test for new-identity revision reset and a late old-Session event. `node --test tests/session-backends.test.js` passed 18, failed 0, skipped 1 (`RUSTED_UTAU_SERVER_URL` unset); `node --check` passed for all three edited JS transport modules. On a clean browser origin, import of the supplied bundle then succeeded at rev1 with 5 tracks/1,709 notes, confirming the reset path end to end. The old live page reproduced the prior rejection; no save or project-content mutation occurred.
- This is a T023 transport regression correction, not a new Session operation or public contract change. T023's original queue/transfer acceptance remains done; T025 broad UI acceptance is still active and incomplete. Repository HEAD/base remains `e6af244abbf04d4a490fca8483295ed6418e6cad` on `wasm-native-rust`; the shared checkout has extensive unrelated dirty/untracked work, so only the user-assigned T023 paths and this task record were changed for this fix. No commit, stage, or remote write.
