# T002 — Complete Android WebView SAF project and voicebank import flow

**Status:** stale — Android wrapper/SAF implementation is explicitly deferred by the user; current code/source blockers are preserved for resumption
**Owner:** unassigned (Codex completed read-only audit)
**Last updated (UTC):** 2026-10-15T01:47:16Z

## Goal

Let users of the Android WebView wrapper import and manage project and
voicebank files through Android's Storage Access Framework. Kotlin owns URI
grants and byte transfer; Rust owns project/bank validation and project
content. The Web UI remains a client of the local Rust server/session.

## Completion criteria

- [x] Confirm which project and voicebank import paths are missing in the current app.
- [ ] Import and reopen supported projects through SAF without creating a second authoritative project model in the Web UI.
- [ ] Import/select voicebanks through the documented Android storage flow.
- [ ] Add host/UI coverage for selection, errors, and resulting engine inputs.

## Source planning

- [Roadmap MS4.3](../../planning/roadmap/ROADMAP.md)
- [MS4 productization](../../planning/milestones/ms4/MS.md)

## Repository checkpoint

- Branch: `wasm-native-rust`; base commit: `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- Relevant inspected files: `app/android/app/src/main/kotlin/com/rustedutau/app/MainActivity.kt`, `app/lib/project/file_project_store.dart`, `app/lib/project/server_project_store.dart`, and `app/lib/screens/voicebanks_screen.dart`. These are dirty in this checkout; their source ownership is unknown, and this audit made no changes to them.
- Owned deltas: `native/tools/synth-server/tests/api_v2_session_router.rs` is a new T002-only router integration test file. The `tower` dev-dependency was added to `native/tools/synth-server/Cargo.toml` for `Router::oneshot`; that manifest was already dirty from T033, so only this dev-dependency delta is attributed to T002. Documentation deltas are this record and the T002 board row; they are untracked in this checkout. No Kotlin, WebView, handler, or production router source is claimed.
- Durable references: [engine-owned state policy](../../architecture/engine-owned-state.md), [T045 contract](../../planning/contract-mk3-proposal.md), [MS4](../../planning/milestones/ms4/MS.md).

## Progress

- 2026-09-30: Added from roadmap notes that project save/open/autosave and WAV save exist while SAF import was listed as remaining. Recheck that snapshot before implementation.
- 2026-10-14T10:37:48Z Read-only audit confirms no SAF picker or URI-grant flow in `app/`: `FileProjectStore` reads/writes app documents by filesystem path; `MainActivity` only returns private engine paths and writes WAV through MediaStore; `VoicebanksScreen` still has a placeholder import action and mock retry/delete state. The engine's `/voicebanks` list is server-provided, but the audit found no SAF byte/tree import bridge. Project import must be routed through Rust-owned session/codec operations; the legacy `ProjectStore`/`ProjectDoc` path must not become a competing project authority. No source files were edited.
- 2026-10-14T10:57:00Z Architecture decision from the user's current direction: Android is a WebView wrapper around the local Rust server, with WebSocket/native host transport; SAF stays in the Kotlin host adapter. A picker returns a `content://` URI and persisted grant. The host streams bytes to a private staged asset (or bounded upload) with a stable asset ID/digest; Rust then loads the project through its codec/session and imports voicebank assets through an engine ingestion service. Export uses Rust serialization plus `ACTION_CREATE_DOCUMENT`. Do not convert a content URI to a filesystem path, deserialize/edit projects in Dart/JS, or claim the existing HTTP/WebSocket routes support this until implemented.
- 2026-10-14T11:02:15Z Read-only current-tree audit found project-session handlers and route declarations in dirty/unowned files: `POST /api/v2/sessions` has an inline `imported_ustx` source, `GET /api/v2/sessions/{id}/export` returns canonical USTX JSON, and `PUT …/save-as` saves under the server's project store. The current server body limit is 2 MiB for JSON; imported USTX therefore has an inline-size ceiling. A raw `PUT /api/v2/media/{id}` handler/route has a 128 MiB cap but calls `media::probe` and accepts audio media only; it is not a voicebank-import API. Voicebank server code scans a configured root, with no upload/install route found. Existing tests cover imported-session/export handlers directly, not an Android SAF round-trip or a live router/client path. Per project policy these unowned working-tree routes are not claimed as accepted/live API here.
- 2026-10-14T11:10:22Z User clarified T005's former Flutter target is stale and the intended app direction is Web wrapper + local Rust server. T002's storage work remains applicable because Android still needs a Kotlin SAF adapter: reframe the client side as WebView/native-host responsibilities. This does not establish that SAF pickers, byte streaming, or Rust ingestion routes exist; the audit findings and blocker remain.
- 2026-10-14T11:27:55Z Rechecked the exact current checkout. `native/tools/synth-server/src/server.rs` assembles dirty/unowned `/api/v2/sessions` routes, import/export/save handlers, and `PUT /api/v2/media/{id}`. `CreateSource::ImportedUstx` still carries a JSON string under the global 2 MiB body limit; the separate 128 MiB media route calls `media::probe`, stores audio, and is not project or voicebank ingestion. `voicebanks.rs` only scans the configured root, and router assembly has no bank upload/install route. The integration test file calls handlers directly; `rg 'router\(|oneshot|ServiceExt' native/tools/synth-server/tests` finds no route-level test. `MainActivity.kt` still subclasses `FlutterActivity` and exposes a Flutter MethodChannel (`engineDir`, `engineRunning`, `writeProject`, `saveWav`); fresh search finds no WebView or SAF picker/URI grant. These sources remain modified/unowned and are not accepted as live product routes. Re-ran `cargo test --quiet -p synth-server --test api_v2_sessions an_imported_ustx_session_starts_fresh -- --exact` and `cargo test --quiet -p synth-server --test api_v2_sessions exporting_returns_portable_ustx_without_sidecar_or_revision_change -- --exact` — both PASS (1/1 each); they validate handler behavior only, not router/Android/SAF or voicebank import.
- 2026-10-14T12:34:28Z Added a separately owned router-level test file, `native/tools/synth-server/tests/api_v2_session_router.rs`, and the test-only `tower` `util` dependency. `cargo test --manifest-path native/Cargo.toml -p synth-server --test api_v2_session_router` passes 2/2: an imported USTX traverses the actual router into a Rust Session and returns canonical USTX from the export route; a request one byte over `BODY_LIMIT_BYTES` is rejected with 413. This closes the previous evidence gap about handler-only tests and confirms the current 2 MiB ceiling at router level. It does not add SAF, streaming import/export, Android WebView, voicebank ingestion, or validate that these dirty/unowned routes are accepted product APIs.

### Proposed SAF-to-engine contract gap

- Project import needs a bounded binary/file-stream adapter into Rust USTX parsing/session creation; do not route large USTX through the current 2 MiB JSON source. A successful import must return the new Rust session ID/revision/snapshot and use fresh identity as defined by the session contract.
- Export may use Rust's canonical USTX export result followed by Kotlin `ACTION_CREATE_DOCUMENT`; large exports should stream to the selected URI rather than copy through Dart/JS-owned project serialization.
- Voicebank import needs a separate bounded archive-ingestion service: stream to private staging, reject traversal/oversized entry/count/expanded-size cases, validate with the Rust voicebank loader, atomically publish to the configured bank root, rescan and return engine-derived bank metadata. The media endpoint is not suitable.
- These are proposed service requirements, not frozen schema or live routes. Additive contract review and source ownership are still required before wiring them into the app.

## Verification

- Read-only `rg` inventory for `ACTION_OPEN_DOCUMENT`, `ACTION_OPEN_DOCUMENT_TREE`, `ACTION_CREATE_DOCUMENT`, `ContentResolver`, file-picker/SAF APIs and relevant Android/Flutter project/voicebank stores — no SAF selection flow found. Inspected `MainActivity.kt`, `file_project_store.dart`, `server_project_store.dart`, and `voicebanks_screen.dart`.
- Read-only inspection of `native/tools/synth-server/src/server.rs`, `handlers_v2.rs`, `voicebanks.rs`, `app/lib/api/engine_client.dart`, Android `MainActivity.kt`, and `api_v2_sessions.rs` confirms handler code and router declarations exist in this dirty/unowned tree. The media upload is audio-only; no voicebank ingestion route exists. Existing import/export tests exercise handlers directly, not the router, SAF, or Android-to-engine transfer.
- `cargo test -p synth-server --test api_v2_sessions an_imported_ustx_session_starts_fresh -- --exact` — PASS (1 passed, 30 filtered out).
- `cargo test -p synth-server --test api_v2_sessions exporting_returns_portable_ustx_without_sidecar_or_revision_change -- --exact` — PASS (1 passed, 30 filtered out).
- Both tests were rerun at 2026-10-14T11:27:55Z and pass 1/1 each. They validate Rust handler/session import-export semantics only. They do not verify router dispatch, the 2 MiB body-limit behavior, SAF picker/grants, file streaming, or voicebank ingestion. Earlier non-quiet run emitted existing unused-variable/unsafe warnings in `moe-train`, `diffsinger`, `synth-cli`, and `synth-server`.
- `cargo test --manifest-path native/Cargo.toml -p synth-server --test api_v2_session_router` — PASS (2 passed). Verifies only router dispatch for inline USTX session import/canonical export and the 2 MiB JSON body limit; it is not Android/SAF or voicebank ingestion coverage.
- No product source changed during this documentation re-scope; the two handler-level Rust test results above are from the earlier audit.

Repeatable read-only inventory command from repository root:

```sh
rg -n "ACTION_OPEN_DOCUMENT|ACTION_OPEN_DOCUMENT_TREE|ACTION_CREATE_DOCUMENT|ContentResolver|FilePicker|file_picker|saf|Saf|Storage Access" app
```

Outcome: no SAF selection/import symbol found; the listed store/activity/screen files show private-path storage, MediaStore WAV export, server bank listing and a placeholder import action.

## Resume blockers and dependencies

- Project import must use the Rust-owned project-session/codec contract; the Web UI is not an authority for edits or serialization.
- Android storage adapter direction is decided as recorded above; remaining work is to define/version upload/import/export and voicebank-ingestion operations. `content://` must remain host-owned and must not be passed to Rust as a fabricated path. Dirty/unowned source contains session route declarations, but there is no router-level test or Android/WebView path evidence; do not treat handler tests as proof the route works end to end.
- Voicebank import needs an explicit engine ingestion API that accepts bounded SAF streams or staged bytes. The current server listing alone does not import a bank.
- Source files are modified in this checkout with unknown ownership. Implementation remains unassigned until exact files and ownership are confirmed.
- Following the router-test addition, the next external ownership decision is the production path set: Kotlin SAF/WebView host (`app/android/app/src/main/kotlin/com/rustedutau/app/MainActivity.kt` or a new dedicated host file; `EngineProcess.kt` already has T033-owned launch-credential deltas), Rust routes/services (`native/tools/synth-server/src/server.rs`, `handlers_v2.rs`, `voicebanks.rs`, `state.rs`), and Web call sites under `web/app/` (overlapping active T004). The existing files are dirty/untracked and must not be treated as Codex-owned based on the isolated new test. Keep T002 blocked until a single integrator and exact per-file handoff are recorded.

## Next step

Do not implement T002 while Android remains deferred. When the user resumes Android work, assign one integrator and confirm exact ownership across `MainActivity.kt`/new WebView host, Rust `server.rs` + `handlers_v2.rs` + `voicebanks.rs`, and T004-owned `web/app/` call sites. `EngineProcess.kt` already contains T033-owned credential changes and must be coordinated. Then implement bounded streamed USTX import/export and safe staged voicebank ingestion, wire operations through the router, and verify Android SAF→engine round-trips. No SAF picker, WebView host, or voicebank ingestion route is present. The isolated router test exercises only inline JSON import/export and the 2 MiB limit. Do not add Flutter-specific behavior.


- 2026-10-15T01:47:16Z User reiterated that Android work is not to start yet. Reclassified this item from `blocked` to `stale` so it no longer appears as an active blocker; preserved the SAF/API and exact source-owner requirements for when the Android wrapper is resumed. No Android or production route source was changed.

## Handoff notes

Use SAF as a host storage adapter. Project content and validation remain Rust-owned under the engine-state policy.
