# Project engineering policy

## Engine-owned product state (mandatory)

For Rusted UTAU, Rust is the single source of truth for all authoritative musical and project state across **every host** (Flutter/Android, Web/WASM, and future desktop/Tauri). Frontends are UI-only clients: they present engine-provided views, send user intents as engine commands, and render engine results. Do not implement a second project model, musical rule set, edit history, serializer, or render-planning authority in Flutter, JavaScript/TypeScript, or another UI layer.

### State that belongs to the Rust engine

- Project/document contents and identity: tempo, meter, tracks, parts, notes, lyrics, singer/voicebank references, expressions/curves, mixer parameters, and project metadata.
- All mutations and validation, including add/remove/move/resize/split notes, track operations, expressions, mixer changes, grouping, and undo/redo.
- Canonical project serialization/import/export and render planning/dirty state where it affects project correctness.
- Engine-derived musical data (phonemization, timing, render status/results) must come from the engine or an explicitly defined engine service contract, not frontend guesses.

### State that may remain in the frontend

Ephemeral presentation/input state only: current selection and hover, active tool, focused field, open/collapsed panels, dock/workspace layout, viewport pan/zoom, modal visibility, and transient transport controls such as which audio element is currently playing. A frontend may cache engine-provided snapshots for display, but such a cache is disposable and must never become the source used to serialize, render, or validate a project.

### Required interaction pattern

The draft shared logical API for native HTTP, WASM direct calls and future JNI is documented in [CONTRACT-v1.md §6](docs/CONTRACT-v1.md#6-project-session-api-draft-v1--implementation-ready-partially-implemented). It is not frozen yet and only partially implemented: identity, the sidecar and the revision rules live in `project-session`, while stable-ID commands, groups, bounded reads and every HTTP route are still owed. Do not claim routes/schema are live. New UI/session integrations should preserve the shared Rust command semantics and follow the review/versioning process there.

1. Frontend sends an intent/command with stable project/track/part/note identifiers (avoid relying on UI list indices when identity is available).
2. Rust validates and applies it atomically, records undo/redo, and returns an error without partial mutation on failure.
3. Frontend refreshes or consumes the resulting authoritative snapshot/events and displays it. Render/save always consume engine-owned project state.
4. Grouped gestures (for example dragging a group of notes) are one engine undo transaction.
5. Host-specific storage and audio playback are adapters around the engine; they do not own a competing document model.

### Migration rule

Existing Flutter and Web behavior is not automatically compliant just because Rust renders the audio. When changing a feature, migrate its project state and mutation path to Rust first (or include the migration in the change); do not expand frontend-owned authoritative state. Keep transitional adapters narrow, documented, and covered by parity/regression tests. Update architecture docs and this policy if the ownership boundary changes; exceptions require an explicit architecture decision.

### UI architecture

React/Flutter/HTML and Canvas/PixiJS/dock frameworks are presentation choices. They may own layout, interaction, and disposable view state only. They must call the same Rust domain/command semantics through WASM or a versioned native API so behavior stays consistent across hosts.

## Current known migration gap

The Flutter editor still stores edited notes and pitch curves in `_editedNotes` / `_editedCurves`, derives `_editableTracks`, and constructs USTX/render inputs in Dart. `PianoRoll` also mutates note/curve lists locally, and `editor_ustx.dart` builds/parses USTX in Dart. This is transitional legacy behavior and must be removed by migrating edits into the Rust-owned project session before treating Flutter as UI-only. Extract a platform-neutral `project-session` crate from the existing WASM session semantics, add curve commands and stable IDs, expose native session commands, and make save/render consume that session. See [Engine-owned state policy and migration](docs/architecture/engine-owned-state.md).

The existing Web `Driver` already routes project edits through `engine-wasm`, but UI selection/viewport state appropriately remains in the frontend. Native server project save/load currently uses codec JSON endpoints; it does not yet expose the same stateful edit/undo session contract as `engine-wasm`.

## Repository hygiene

Never write to the mainstream project this repository was forked from. Work locally and publish only to a confirmed repository owned by the user, subject to the global instructions.

## Agent task board

- Read [`docs/work/BOARD.md`](docs/work/BOARD.md) at the start of a task and before taking over work from another harness or agent.
- For work that spans more than one focused editing session, create or update a task record in `docs/work/tasks/` and add its link to the board. Small one-shot changes may stay untracked.
- Update the task record when meaningful progress changes, when blocked, and before handing work off or ending a session. Keep the board row concise; the task record is the handoff source of truth.
- Record the repository branch and base commit, relevant dirty files, exact verification commands and outcomes, blockers, and the next concrete step. Do not claim existing edits unless their ownership is known; describe unowned working-tree changes as such.
- Use ISO 8601 UTC timestamps. An `active` status records the last known state; it does not prove an agent is still running. Include a last-updated time so a receiving harness can check freshness.
- Keep durable task details and artifacts in the repository or link to stable locations. Do not rely on chat history, machine-local temporary files, or harness-specific IDs as the only handoff record.
- Mark work `done` only when its stated completion criteria are met. Move completed task records to `docs/work/archive/` when the active board becomes crowded.

## Verification expectations

- Rust command/model tests cover validation, atomicity, group undo/redo, serialization round-trip, and host parity.
- Host tests verify commands update the authoritative Rust document and that saved/rendered documents reflect those commands.
- UI tests assert presentation and command dispatch; do not duplicate Rust musical-rule tests in frontend code.
- Report any unverified build/test result honestly.
