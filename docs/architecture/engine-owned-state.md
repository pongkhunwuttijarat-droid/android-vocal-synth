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

The current browser `Driver` wraps `engine-wasm`; project load, command application, undo/redo, note/curve snapshots and YAML serialization are Rust-owned. The browser `PianoRoll` keeps interaction state (selection, viewport, active tool, pointer state) and emits commands. This is the intended pattern. OPFS is a host storage adapter that writes the Rust-produced YAML.

### Flutter / Android

Flutter is **not yet UI-only**. In `app/lib/screens/editor_screen.dart`, `_editedNotes` and `_editedCurves` are mutable authoritative Dart maps; `_editableTracks` merges them into sample tracks; `_buildCurrentUstx()` builds render input in Dart; note/curve callbacks mutate those maps; `_saveProject()` stores a Flutter-created `ProjectDoc`. The screen also owns demo track definitions and project metadata. `app/lib/widgets/piano_roll.dart` mutates its own `_notes` and `_curvePoints` and implements musical edit algorithms. These are migration targets. Selection, tool, viewport, focus and in-progress gestures remain legitimate UI state.

`app/lib/editor_ustx.dart` also contains Dart USTX building and hand-written parsing. The Rust codec is used elsewhere, but the Flutter editor still supplies its own document data and editing semantics. `Note`/`Track`/`PitchPoint`/`ProjectDoc` Dart classes may temporarily remain only as transport/view DTOs or disposable caches; they cannot be authoritative.

The native `synth-server` currently has stateless project codec save/load endpoints and render-by-path. That is insufficient for authoritative edits with command history or render-from-current-session.

## Migration architecture and sequence

### Shared Rust boundary

Extract the platform-neutral `ProjectDoc`/command protocol/view generation currently in `native/crates/engine-wasm/src/project.rs` into a small `project-session` crate. Keep dependencies narrow: `domain`, `commands`, `serde_json`, and `serde_yaml`; no `wasm-bindgen`, renderer, worker, filesystem, or HTTP dependency. It owns UProject, undo/redo, atomic command application, canonical YAML, and read-only snapshots. `engine-wasm` becomes a host adapter which depends on/re-exports this crate. The native server uses the same session semantics and keeps HTTP/session locking and filesystem persistence in its adapter.

The extracted first-slice command API still uses part/note/track indices and does not yet implement stable IDs. Introduce stable project/track/part/note identifiers before Flutter relies on them across mutations; indices can remain an explicitly temporary in-session adapter only. Grouped operations currently apply child commands as they arrive; if a child fails, the host must issue `group_cancel` to roll back the in-progress gesture. Per-command failures are atomic; an open multi-command group is not automatically rolled back.

### Phases

1. **Session contract:** define stable identifiers, command schema (including curve editing), snapshot schema, session lifecycle, error semantics and grouping. Preserve existing Web API compatibility.
2. **Shared Rust crate:** extract ProjectDoc without changing semantics; move/adapt applicable `engine-wasm` tests to the shared crate and keep wasm facade tests.
3. **Native bridge/server:** add versioned session create/load/snapshot/command/group/undo/redo/save APIs. Render an immutable snapshot from that same session rather than independently reloading a stale path. Ensure simultaneous sessions are isolated.
4. **Flutter notes migration:** replace `_editedNotes`, `_editableTracks` and local note mutation with engine commands + returned snapshots. Convert Flutter PianoRoll gestures to intent dispatch; group drag into a single undo step. Wire toolbar undo/redo to the Rust session.
5. **Flutter curves migration:** add Rust curve commands and snapshots, then remove `_editedCurves` and `_pencilWrite` as authoritative mutation. Keep curve drawing gesture coordinates in UI.
6. **Save/open/render migration:** make open/load, autosave, Save As, export and render consume the same engine-owned session. Remove Dart `buildUstx`/`parseUstx` and duplicate project conversion paths once no caller needs them.
7. **Remaining feature parity:** move project/track/part metadata, mixer settings and any musical/render-affecting state under the same contract. Keep presentation-only dock layout in frontend.

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

A first logical API draft for native HTTP, WASM direct calls, and future JNI adapters is in [CONTRACT-v1.md §6](../CONTRACT-v1.md#6-project-session-api-draft-v1--implementation-ready-partially-implemented). It is intentionally not frozen. Identity, the sidecar table and the per-change revision rules are implemented in `project-session`; stable-ID commands, group transactions, bounded reads and the HTTP routes are not. All transports should reuse `project-session` semantics; only transport/session-lifecycle/storage adapters differ. Review stable IDs, session lifetime, group transactions, curves and async render progress before bumping/finalizing the contract.

## References

- [Architecture overview](README.md)
- [Rust/WASM project session](../../native/crates/engine-wasm/src/project.rs)
- [Flutter editor](../../app/lib/screens/editor_screen.dart)
- [Flutter Piano Roll](../../app/lib/widgets/piano_roll.dart)
- [Browser driver](../../web/app/driver.js)
- [Android HTTP contract](../CONTRACT-v1.md)
- [Flutter/Web product roadmap](../planning/roadmap/ROADMAP.md)
