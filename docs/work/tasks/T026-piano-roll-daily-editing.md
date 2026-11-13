# T026 — P1A daily Piano Roll editing workflows

**Status:** blocked — T004 currently owns the overlapping editor paths; T026 has no bounded handoff or implementation authorization  
**Owner:** unassigned — Piano Roll UI  
**Last updated (UTC):** 2026-10-15T03:23:24Z

## Scope and deliverables

Complete high-frequency Piano Roll workflows: note selection/create/move/resize, lyrics and practical property editing, predictable undo/redo/grouping, keyboard shortcuts, and editor feedback. Keep Rust authoritative for committed musical state; previews remain disposable.

## Existing task relationship

- Follows the [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md) P1A scope and [T025](T025-arrangement-piano-roll-regression.md) regression gate.
- Reuse existing stable-ID note/part commands and done [T013](T013-phoneme-input-and-mapping.md) phoneme composition only where it covers a specific workflow. Do not duplicate T018's implemented vibrato path or T014's proposal.

## Dependencies and blockers

- T025 is now done (Wave44, 2026-10-15): the Arrangement → Roll handoff and browser playback/export regression gate passed on the saved 5-track project copy.
- Still blocked on a named UI integrator/source-path handoff and separate implementation authorization. Any required note operation missing from the reviewed Session surface requires separate T014/contract review; do not edit CONTRACT-v1.

## Likely files / ownership

`ui-mock/react-dock/src/openutau/piano-roll-panel.jsx`, `note-properties-panel.jsx`, `clone-state.jsx`, `engine.js`, shortcut modules, and focused UI tests. These are shared T004/T019/T018 surfaces; assign one UI integrator and serialize edits to common files.

## Acceptance and evidence

- [ ] Representative daily note/lyric workflows commit through Rust and survive snapshot refresh, save/reopen, undo, and redo.
- [ ] Grouped gestures produce the intended single undo unit; invalid/stale commands fail without partial UI/project changes.
- [ ] Keyboard/mouse behavior, selection and accessible feedback are documented and covered by tests/manual browser evidence.
- [x] Arrangement → Roll gate T025 is green (see T025 Waves43–44); no local React state is used for save/render.

## Concurrency and approvals

Can run after separate implementation authorization alongside independent curve-fixture work only with distinct files or an agreed single UI owner. No simultaneous edits to `piano-roll-panel.jsx` without coordination. No dependency or contract changes authorized.

## Wave 1 checkpoint (2026-10-08)

- No T026 source work was assigned: Piano Roll surfaces are dirty/shared with active T004/T019/T018 work, and the T025 acceptance gate is still blocked.
- Keep committed musical state Rust-authoritative; no local UI workaround or public Session operation was added.
- Next: after a named UI integrator and a green T025 fixture, inventory existing daily workflows and claim only disjoint, explicitly owned paths.

## Wave 2 ownership checkpoint (2026-10-08)

- Read-only audit confirms ui-mock/react-dock has 72 untracked paths and is shared across T004/T019/T018 and blocked T022/T023/T025/T026/T027/T031. piano-roll-panel.jsx and related editing surfaces have no confirmed owner; do not infer ownership from task resemblance.
- T025 remains blocked by the mock-only document/no reachable Rust Session and missing off-zero part. T026 stays blocked and unassigned pending a named UI source owner, a green T025 gate, and separate implementation approval. No source edits were assigned.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`; existing UI working-tree ownership remains unknown.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Next: obtain a named UI integrator and exact owned path set plus implementation authorization; then inventory current controls against only missing P1A acceptance points.

## Wave 3 dependency refresh (2026-10-15)

- T025's formerly blocked acceptance gate is now done. The saved-copy browser run verified same-project selection, stale-render fencing, Rust Session revision remaining clean, non-silent playback output and a freshly downloaded/decoded WAV; see T025 Wave44 for exact evidence and limits.
- T026 remains blocked because `piano-roll-panel.jsx` and related UI paths are shared/unowned and no implementation handoff has been recorded. Separate implementation authorization is also still required. No UI source was edited or claimed.

## Current ownership audit (2026-10-15T03:23:24Z)

- Rechecked the active board and T004 handoff. T004 is still active and its task record lists `piano-roll-panel.jsx`, `note-properties-panel.jsx`, `clone-state.jsx`, and `engine.js` among its owned editor paths. These are also T026 candidate paths; `piano-roll-panel.jsx` is shared with T027. T026 therefore cannot claim them from the untracked state alone.
- T025's acceptance gate is done, so the remaining blocker is a serialized source handoff plus scoped implementation authorization. Safe next step: have the current T004 owner finish or explicitly hand off the overlapping path subset, then sequence T026 before T027 and name the focused test files. No UI source was changed or claimed in this audit.
