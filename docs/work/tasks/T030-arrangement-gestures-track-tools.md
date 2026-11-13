# T030 — P3 Arrangement gestures and remaining track tools

**Status:** blocked — Arrangement UI ownership/handoff and implementation scope unresolved
**Owner:** unassigned — proposed Arrangement UI lane
**Last updated (UTC):** 2026-10-15T03:00:29Z

## Scope and deliverables

Implement the remaining reviewed Arrangement interactions (timeline seek/range, part move/resize, marquee, wave fades, context actions) and only track-header/tools gaps still present after source audit. Preserve the completed Arrangement → Piano Roll handoff, project-tick scrolling, and connected track controls.

## Existing task relationship

- Depends on regression gate [T025](T025-arrangement-piano-roll-regression.md) and uses [T021](T021-openutau-ui-page-by-page.md) inventory.
- [T004](T004-web-editor-product-flow.md) and [T019](T019-openutau-web-ui-parity.md) already record project-tick scrolling, real track/mix controls, and other working product flows; [T020](T020-arrangement-worker-isolation.md) is done. Do not recreate them.

## Dependencies and blockers

- Blocked on separate implementation authorization. Any track/part mutation not covered by existing reviewed Session commands waits for T014/contract approval.

## Likely files / ownership

`ui-mock/react-dock/src/openutau/arrangement-panel.jsx`, Arrangement-specific geometry/gesture modules, and focused browser tests. `arrangement-panel.jsx` is already modified by T004/T020 and is a single-owner file during implementation; no concurrent edits without coordination.

## Acceptance and evidence

- [ ] Each remaining gesture has a behavior spec, Rust command mapping, and regression tests; previews remain ephemeral until commit.
- [ ] Selection, geometry, tick range, zoom/scroll, undo/redo, and track/part identity remain correct on long/multi-track real projects.
- [ ] T025 Arrangement → Roll gate and current connected track controls pass; no duplicate track-create/mixdown features.
- [ ] Browser evidence distinguishes completed T004/T019/T020 history from fresh acceptance.

## Concurrency and approvals

May run after T025 with separate geometry/gesture file ownership. Changes to the shared Arrangement panel are serialized through the UI integrator. No contract, dependency, or integration approval is implied.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`; existing dirty/untracked files remain unowned absent a recorded owner.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Next: confirm an Arrangement UI integrator and exact source ownership/implementation scope; then take the listed gaps in isolated increments. T025 is now done and no longer blocks T030.

## Wave 3 read-only gesture gap audit (2026-10-15)

- **Audit scope:** inspected current arrangement panel/geometry sources, T019 connected track-control record, T020 completion record, and the P3 inventory. No source edits, builds or tests.
- **Present in source:** click-to-select a part and set the active part; click an empty track lane to create a four-measure part; horizontal scroll/zoom; resize the track-header column; audio waveform and voice-note miniatures; connected track add/delete/reorder, singer selection, mute/solo, volume, pan, and FX dialog. T025 Wave44 now closes the cross-view regression gate.
- **Still missing from the inspected Arrangement path:** ruler/timeline seek or range selection; part drag-move and edge resize; Arrangement marquee; wave-part fade handles; part/track context actions and overflow tools. Track settings remain visibly disabled; rename/color/duplicate/settings must first be checked against existing Rust Session semantics. Double-click-to-open/focus remains optional per the P3 plan.
- **Ownership:** arrangement panel and related controls are shared with T004/T020 and no ownership handoff is recorded. This audit does not authorize editing those shared paths or inventing frontend project state. T030 therefore remains blocked pending a named integrator, exact path handoff and implementation scope; no T014 contract work is implied.
- **Verification:** source/document inspection only; no command tests/build run. Evidence is source presence/absence and prior task records, not runtime acceptance of these gaps.

## Wave 4 Session command and media-state audit (2026-10-15T03:00:29Z)

- Confirmed `part.move` and `part.resize` are already in frozen CONTRACT-v1 §6.4.4 and the current Rust Session planner (`native/crates/project-session/src/v2.rs:1512-1603`). Their existing semantics cover stable-ID timeline move and atomic non-clipping resize, so these Arrangement gestures do not need new contract/API work.
- Marquee selection is ephemeral presentation state and can remain in the Arrangement UI. Ruler seek is transport state; the assigned T033 playback path currently streams from frame zero and T034 owns nonzero streaming seek, so do not implement a second seek model in T030.
- Wave-fade data exists as `UWavePart.fadein/fadeout` ticks (`native/crates/domain/src/part.rs:114-116`), and the media renderer applies the corresponding sample ramps (`native/crates/media/src/lib.rs:395-459`). However, the current `PartView` and Session command set do not expose a fade read/write command; no `part.set_fades` path was found. Fade-handle UI therefore needs an additive Rust Session read/write/render contract, coordinated with T014/T015 before UI wiring.
- Current UI file `arrangement-panel.jsx` is shared with T004/T020; `clone-state.jsx`/transport integration is T033-owned, and T034 owns stream seek. This audit narrows the only T030-owned UI handoff candidate to Arrangement gestures/geometry and focused tests, while seek and wave-fade API paths need their existing owners. Read-only source audit only; no implementation, build or tests were run.
