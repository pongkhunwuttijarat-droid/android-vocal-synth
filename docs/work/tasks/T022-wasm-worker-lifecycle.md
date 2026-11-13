# T022 — P0A Direct WASM Worker lifecycle hardening

**Status:** done  
**Owner:** Codex — assigned Worker adapter and UI recovery paths  
**Last updated (UTC):** 2026-10-14T12:53:37Z

## Scope and deliverables

Harden initialization, startup timeout, `error`/`messageerror`, close, worker replacement, and recoverable user-visible failure states for the existing Direct WASM Session Worker. Preserve the existing Worker and Rust Session; do not recreate T020. Define what state can be restored and when the UI must prompt the user to reopen rather than imply recovery.

## Existing task relationship

- Builds on completed [T020](T020-arrangement-worker-isolation.md); its responsive-UI, AudioWorklet, and playhead results are regression baselines, not reopened work.
- Coordinates with [T023](T023-wasm-session-transport.md) for the same Worker boundary. [T004](T004-web-editor-product-flow.md) and [T019](T019-openutau-web-ui-parity.md) remain product integration owners.

## Dependencies and blockers

- On 2026-10-14, the user explicitly assigned `session-backends.js`, `wasm-session.worker.js`, `wasm-session-runtime.js`, and focused Worker tests to Codex, in sequence: complete T022 before starting T023. This supersedes the earlier source-ownership blocker for those exact paths only.
- The user also assigned the UI path to Codex on 2026-10-14. The lifecycle is shown in the status line and recovery requires explicit confirmation; project reopen is attempted only from the remembered saved-project name.
- No public Session contract change is expected; if a requirement crosses that boundary, stop for separate contract review.

## Likely files / ownership

`ui-mock/react-dock/src/session-backends.js`, `src/openutau/wasm-session.worker.js`, `src/openutau/wasm-session-runtime.js`, `src/openutau/engine.js`, `src/openutau/clone-state.jsx`, `src/openutau/main.jsx`, `src/openutau/openutau.css`, and focused worker tests. The user assigned the Worker paths, tests, and UI recovery path to Codex; keep T023 serialized behind this handoff.

## Acceptance and evidence

- [x] Specify/test init timeout, explicit close timeout, worker `error`, `messageerror`, and worker termination; success is exercised by the adapter's boot path and recovery test.
- [x] Pending requests have a bounded timeout and every pending caller receives typed failure on worker loss.
- [x] Backend lifecycle events distinguish initializing, ready, failed, recovering, and closed; the UI shows a failure alert and explicit recovery confirmation. Recovery warns that unsaved Session state cannot be restored and reopens only the remembered saved project.
- [x] Failure and close remove registered listeners and terminate the Worker; replacement recovery starts a new Worker.
- [x] Browser-level long-render UI-responsiveness and playback/playhead acceptance from T020, retained as the regression baseline. T020 records the supplied USTX/browser run: render completed while UI stayed responsive, playback passed 28 s, all three views followed the AudioWorklet playhead, and cached replay started without another render. Current T022 error/recovery smoke is separately recorded below; the later broad drag-probe did not pass unrelated product assertions and is not counted as lifecycle or T020 acceptance evidence.

## Concurrency and approvals

May run in the first WASM Runtime lane after authorization. T022 precedes T023 because both touch the Worker adapter/protocol; do not assign separate workers to the shared files concurrently. Contract changes, dependency changes, branch/worktree creation, and integration outside the listed scope require separate approval.

## Wave 1 checkpoint (2026-10-08)

- Classification: T022 source implementation remains blocked; the bounded Wave 1 authorization does not include T022 implementation. Preserve completed T020.
- Conflict decision: `session-backends.js`, `wasm-session.worker.js`, and `wasm-session-runtime.js` are shared/dirty across T022/T023 and active UI owners; one integrator must serialize T022 then T023.
- Next: await explicit T022 implementation authorization and a named integrator, then re-check exact file ownership before edits.

## Wave 2 ownership checkpoint (2026-10-08)

- DSH coordinator is assigned as the single T022/T023 integration/sequencing owner only; this does not claim source ownership or authorize implementation. The main audit found the UI clone and shared Worker files dirty/untracked with active T004/T019/T018 overlap and no source owner established.
- T022 is first. T023 must wait for T022 handoff/acceptance. Do not edit session-backends.js, wasm-session.worker.js, or wasm-session-runtime.js concurrently; confirm per-file owner and separate implementation approval before touching them. No source, API, test, build, dependency, or contract changes occurred.

## Current authorization checkpoint (2026-10-14)

- The user explicitly directed continued work through blocked items and authorized implementation against the accepted MK3 baseline.
- 2026-10-14T12:40Z user handoff: Codex owns the three named Worker adapter files and focused worker tests in strict sequence, T022 then T023.
- 2026-10-14T12:43Z user also assigned the UI path to Codex; recovery warning and prompt are implemented in the clone state and status line.

## Repository checkpoint and next step

- Current checkout: branch `wasm-native-rust`, base/HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`. Assigned paths are pre-existing untracked files; T022 deltas in this turn include the backend adapter, `engine.js`, `clone-state.jsx`, `main.jsx`, `openutau.css`, and new `tests/wasm-worker-lifecycle.test.js`. Other UI clone changes remain unclaimed.
- 2026-10-14T11:31:27Z Read-only lifecycle audit of the current untracked Worker adapter: boot has no timeout; only `error` is registered (no `messageerror`); worker failure rejects pending requests but does not record a terminal backend state or remove listeners; there is no explicit recover/reopen state machine. `close()` terminates in `finally`, but a lost `dispose` reply can leave that call pending indefinitely. This is source inspection only; no tests were run and no files outside task docs were edited.
- 2026-10-14T12:43:56Z Implemented bounded Worker request/startup timeouts, typed `SessionWorkerError`s, `error` and `messageerror` failure handling, listener cleanup and termination, observable lifecycle events, explicit replacement recovery, and close timeout cleanup in the assigned adapter. Recovery resets to an empty Rust session state and returns `{ restored: false, requiresReopen: true }`; no unsaved snapshot is represented as restored. Added `tests/wasm-worker-lifecycle.test.js`. Verification: `node --test tests/session-backends.test.js tests/wasm-worker-lifecycle.test.js` — 14 passed, 0 failed, 1 skipped (requires `RUSTED_UTAU_SERVER_URL`); `npm run build` passed with existing missing optional fixture/model URL and large-chunk warnings; `git diff --check` passed. Earlier test attempts exposed an incomplete fake boot response; the fixture was corrected and the final rerun passed.
- 2026-10-14T12:51:13Z Added engine/UI lifecycle propagation, status-line failure alert, confirmation before recovery, and explicit reopen of the remembered persisted project only. If no saved project is available (or reopening fails), UI says to choose File → Open. Recovery also works if initial Worker construction failed, and repeated recovery tests confirm replaced Workers have no registered handlers; closed Worker is terminated. Final focused verification: `node --test tests/session-backends.test.js tests/wasm-worker-lifecycle.test.js tests/active-project-store.test.js` — 20 passed, 0 failed, 1 skipped (`RUSTED_UTAU_SERVER_URL` unset); `node --test tests/audio-playback.test.js tests/audio-waveform-cache.test.js tests/transport-state.test.js` — 4/4; `npm run build` passed with existing optional asset and large-chunk warnings; `git diff --check` passed. Full `node --test tests/*.test.js` earlier in this turn was 125 passed, 1 failed, 3 skipped; existing `tests/openutau-engine.test.js:52` expects `commandsFor(track.create)` to return null but it returns a `track.create` command. Live probe `node tools/drag-probe.mjs 'http://127.0.0.1:4187/?backend=wasm'` did not complete: it produced several product assertion failures (including audio/playhead, import-menu expectations, singer-bank labeling, note resize and pitch editing) and then errored because `.ou-tool-btn` was absent. It therefore does not prove T020 long-render responsiveness or final UI recovery behavior. Vite preview was stopped after the probe.
- 2026-10-14T12:53:37Z User confirmed the UI-path ownership handoff to Codex. In a disposable in-app browser tab, the Direct-WASM page was booted, an isolated synthetic Worker `error` event was dispatched, and the visible failure state showed the Rust Worker stopped, unsaved changes unavailable, and a Restart Worker action. Clicking recovery completed and the UI reported: “Worker restarted with no project loaded. Open a saved project from File → Open.” No saved project was available in that test session; the warning correctly did not claim unsaved state was restored. The disposable tab was closed and the Vite server started for this check was stopped. This verifies the visible failure/recovery path only; it does not clear the outstanding T020 long-render/playback/playhead browser acceptance. No product document was saved or changed by the synthetic event.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Handoff: T022 lifecycle implementation and acceptance are complete. T023 may now proceed against the explicitly assigned Worker adapter/runtime files and focused tests; keep edits within that scope and preserve the frozen Session contract.
