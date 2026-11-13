# Engine-owned project state policy and migration

> Decision recorded: 2026-09-29. Applies to Flutter/Android, Web/WASM, and future desktop/Tauri hosts.

## Decision

Rust is the single authoritative owner of musical and project state. A frontend is a UI-only client: it presents engine snapshots, sends user intents as commands, and plays or displays engine results. This ownership rule is independent of UI toolkit: Flutter, React, HTML, Canvas, PixiJS, or a dock framework may change without moving project semantics into the presentation layer.

### Engine-owned state and behavior

- Project identity/metadata, tempo/meter, tracks, parts, notes/lyrics, voicebank/singer references, expressions/curves, and mixer parameters.
- Validation and every persistent mutation, including note/track/part/curve/mixer commands, grouping, undo and redo.
- Canonical USTX parsing/serialization and the project snapshot used for save, render and export.
- Musical interpretation such as phonemization/timing, plus render planning/cache invalidation whenever it depends on project correctness.

### Frontend-only state

Ephemeral presentation and input state may remain in the host: selection/hover/focus, active tool, viewport pan/zoom, dock/workspace layout, open panels/dialogs, keyboard/pointer gesture-in-progress, and transient audio device/player handle. A display cache of Rust snapshots is allowed if disposable. It must never be the source for save, render, export or validation.

### Command and snapshot contract

1. The host translates an input gesture into a semantic command. Commands should identify project, track, part and note by stable IDs; index-based addressing is a transitional limitation and must not be mistaken for identity.
2. Rust validates and applies each command atomically. A rejected command leaves the document unchanged and returns a user-presentable error.
3. Rust maintains undo/redo; one user gesture that changes multiple entities is one grouped undo step.
4. The host refreshes or consumes the resulting authoritative snapshot/event. Saving, exporting and rendering read that same engine-owned document.
5. Host-specific storage and audio output are adapters; they do not maintain a competing document model.

## Current architecture and migration gap

### Browser

The current browser `Driver` wraps `engine-wasm`; project load, command application, undo/redo, note/curve snapshots and YAML serialization are Rust-owned. The WASM `Engine` now holds the shared `project-session::Session`, so its document, identity table, revision and undo history use the same Rust data holder as native. The old browser command facade remains index-addressed for compatibility; stable-ID v2 commands and durable identity sidecars still need to be exposed to the browser. The browser `PianoRoll` keeps interaction state (selection, viewport, active tool, pointer state) and emits commands. OPFS is a host storage adapter that writes Rust-produced YAML and still needs to preserve the session identity sidecar.

### Flutter / Android

Flutter is a secondary host in the current product direction. Its project session adapter is being migrated in the working tree; the editor screen now opens a native Rust session, dispatches note/curve commands, and saves/renders through that session. Flutter analysis and host-level tests remain unverified, so it is not yet accepted as UI-only. Any remaining local PianoRoll algorithms and the Dart store/USTX helpers are migration targets only where the Flutter host is still expected to ship.

`app/lib/editor_ustx.dart` also contains Dart USTX building and hand-written parsing. The Rust codec is used elsewhere, but the Flutter editor still supplies its own document data and editing semantics. `Note`/`Track`/`PitchPoint`/`ProjectDoc` Dart classes may temporarily remain only as transport/view DTOs or disposable caches; they cannot be authoritative.

The native `synth-server` keeps legacy stateless codec routes and adds v2 session routes backed by the shared `Session`. Native/server is the primary target for T001; its session lifecycle, command, bounded-read, save, and render paths are being checked against the frozen portions of CONTRACT-v1 §6. A session phonemize route was removed because it is not part of that contract; adding it requires an explicit contract update.

## Migration architecture and sequence

### Shared Rust boundary

The platform-neutral `ProjectDoc` and `Session` now live in `native/crates/project-session`, without `wasm-bindgen`, renderer, worker, filesystem, or HTTP dependencies. `engine-wasm` and the native server use this shared crate. The WASM `Engine` was moved from holding `ProjectDoc` directly to holding `Session`; its old index-command facade adapts into that session for compatibility. The remaining cross-host gap is the browser adapter's migration from index commands/YAML-only OPFS persistence to stable-ID session commands and sidecar persistence.

The older browser `Driver` still uses part/note/track indices; the WASM `Engine` now also exposes stable-ID session commands, groups, history, bounded reads and sidecar operations over the shared `Session`. New integrations should use that API. Legacy grouped gestures preserve CONTRACT-v1 §6.8.3 immediate-apply and undo-stack cancellation behavior; isolated tentative transactions are a separate v2 group API. Migrating the browser `Driver` and OPFS flow to the new API remains follow-up work.

### Phases

1. **Session contract and shared crate:** stable IDs, curve commands, groups, revisions, canonical document and views are implemented in `project-session`.
2. **Native/server adapter (primary):** keep HTTP lifecycle, locking and file I/O in `synth-server`; verify routes, commands, error envelopes, revision rules, persistence and render against frozen CONTRACT-v1 §6 semantics.
3. **Cross-host parity:** run shared command vectors through native and WASM adapters and verify serialization/sidecar identity across reopen.
4. **WASM adapter follow-up:** expose shared session snapshots, stable-ID commands, revisions, groups, history and sidecar operations through `engine-wasm`; migrate the browser `Driver` and OPFS adapter when Web is again in scope.
5. **Flutter adapter follow-up:** continue the Flutter session bridge only if Flutter remains a shipping target; validate and remove Dart-owned conversion/storage paths before calling that host UI-only.
6. **Remaining project state:** move any project-affecting mixer or metadata state into the same contract; keep presentation-only dock layout in the frontends.

## Acceptance criteria for calling a frontend UI-only

- No authoritative note/curve/track/project copy is mutated in the frontend.
- Every project edit is a Rust command with Rust validation and Rust undo/redo.
- Save/export/render all consume the current Rust document, not a frontend reconstruction.
- Rejected commands leave project, undo history and published render input consistent.
- Tests demonstrate state parity for the same command sequence across native and WASM hosts.
- Frontend owns only explicitly ephemeral UI/input state and disposable read models.

## Contributor policy and documentation

Repository-root [`AGENTS.md`](../../AGENTS.md) makes this rule mandatory. Any exception requires an explicit architecture decision recorded here. Update this document and the roadmap when ownership or migration status changes.

## Shared host API draft

The shared logical API for native HTTP, WASM direct calls and future JNI adapters is in [CONTRACT-v1.md §6](../CONTRACT-v1.md#6-project-session-api-draft-v1--implementation-ready-partially-implemented). Follow the sections marked frozen; other draft sections remain subject to the contract's review process. `project-session` implements stable IDs, revision rules, groups, curve commands and read models. Native/server is the primary integration target. WASM now exposes the shared session operations, while the browser `Driver` still needs to migrate from its compatibility facade. All transports reuse `project-session` semantics; only transport, session lifecycle and storage adapters differ.

## References

- [Architecture overview](README.md)
- [Rust/WASM project session](../../native/crates/engine-wasm/src/project.rs)
- [Flutter editor](../../app/lib/screens/editor_screen.dart)
- [Flutter Piano Roll](../../app/lib/widgets/piano_roll.dart)
- [Browser driver](../../web/app/driver.js)
- [Android HTTP contract](../CONTRACT-v1.md)
- [Flutter/Web product roadmap](../planning/roadmap/ROADMAP.md)
