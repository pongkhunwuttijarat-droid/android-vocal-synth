# T051 — Server-side HTTP session recovery

**Status:** done — restart/reload acceptance and user-visible checkpoint-failure reporting verified  
**Owner:** Codex  
**Last updated (UTC):** 2026-10-24T10:46:37Z

## Goal

Persist committed Web HTTP session state under server project storage so a page or server restart can reopen the last project for that tab without overwriting named project files.

## Completion criteria

- [x] Rust writes canonical USTX plus identity/pronunciation sidecars to a separate recovery slot after a committed revision.
- [x] Browser-uploaded audio assets referenced by recovered projects are stored on disk and loaded lazily after server restart.
- [x] The HTTP client stores only an opaque per-tab recovery key and asks Rust to reopen the checkpoint on boot.
- [x] Recovery creates a fresh Rust session; revision, undo history, and an unfinished gesture are not claimed as recovered.
- [x] Runtime acceptance confirms edits survive server restart and page reload, while an explicit named project is not overwritten by autosave.
- [x] Verify recovery storage failure is surfaced clearly to the user.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e6af244abbf04d4a490fca8483295ed6418e6cad`
- Relevant working-tree changes for this completion: `native/tools/synth-server/src/handlers_v2.rs`, `ui-mock/react-dock/src/session-backends.js`, `ui-mock/react-dock/src/openutau/engine.js`, and `ui-mock/react-dock/tests/http-recovery.integration.test.js`. The wider repository already contains many uncommitted/untracked changes; no unrelated files were staged, reset, or attributed to T051.
- Durable artifacts: recovery snapshots live under `<projects-root>/.recovery/<opaque-id>.json`; implementation notes are in `docs/CONTRACT-v1.md` §6.12.

## Progress

- 2026-10-17T13:46:14Z Added Rust-owned server recovery snapshots, checkpoints on committed revision changes, an HTTP recovery endpoint, per-tab browser recovery keys, and boot-time reopen. Checkpoints are isolated from named `.ustx` files. Save and Save As update the stored association. Browser-uploaded media is stored as `<projects-root>/.media/<id>.media` and is resolved lazily after restart.
- 2026-10-24T10:46:37Z Added an HTTP response warning when recovery checkpoint persistence fails, forwarded it from `HttpSessionBackend` into engine error state (rendered by the editor's existing `last error` status), and added a real-process integration test for restart/reload recovery and the failure warning. Verified that edits restore into a fresh session while the explicitly saved `.ustx` stays unchanged.

## Verification

- `cargo build --manifest-path native/Cargo.toml -p synth-server --bin synth-server` — PASS, 2026-10-24; existing compiler warnings remain.
- `node --test tests/http-recovery.integration.test.js` (`ui-mock/react-dock`) — PASS, 2/2. Starts the rebuilt server, saves a named project, edits, restarts the server on the same origin, and verifies latest-edit recovery/new session/stable IDs/no overwrite; separately forces `.recovery` persistence failure and verifies the live editor receives the warning while remaining ready.
- `npm run build` (`ui-mock/react-dock`) — PASS, 2026-10-24; existing missing fixture/model/vendor URL and large-chunk warnings remain.
- `node --test tests/*.test.js` — 154 pass, 2 fail, 3 skipped out of 159. The failures are outside T051: `audio-playback.test.js:227` (native stream ended without audio) and `openutau-engine.test.js:52` (pre-existing `track.create` expectation mismatch); three live-server cases require `RUSTED_UTAU_SERVER_URL`.
- `git diff --check -- native/tools/synth-server/src/handlers_v2.rs ui-mock/react-dock/src/session-backends.js ui-mock/react-dock/src/openutau/engine.js` — PASS.
- A Rust package integration-test run was blocked before reaching T051 tests by the existing `engine-wasm/src/lib.rs:526` compile error in a dev dependency (`chunk_render_input` argument mismatch); the black-box runtime test above exercises the rebuilt synth-server instead.

## Blockers and dependencies

- No remaining T051 acceptance blockers. The two failures in the broader Node suite are recorded above and belong to separate streaming/engine work; do not attribute them to this recovery change.
