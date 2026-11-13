# T029 — P1D editor menus, shortcuts, and batch workflows

**Status:** blocked — fresh source audit found concrete shortcut and multi-note transaction gaps; implementation still needs a serialized source-owner handoff  
**Owner:** unassigned — proposed Web editor workflow lane  
**Last updated (UTC):** 2026-10-15T05:12:12Z

## Scope and deliverables

Finish the high-frequency editor workflow layer: menu action completeness, keyboard shortcuts, batch editing, and clear navigation/feedback for supported operations. Keep disabled drafts disabled until their Rust-owned semantics exist; do not calculate musical changes in React.

## Existing task relationship

- Builds on the menu/product work already recorded in [T004](T004-web-editor-product-flow.md), the current UI integrations in [T019](T019-openutau-web-ui-parity.md), and page inventory [T021](T021-openutau-ui-page-by-page.md) (done).
- [T016](T016-ui-clone-contract-render.md) owns render menu/preview integration; do not duplicate Render Track or export/mixdown wiring.

## Dependencies and blockers

- Blocked on separate implementation authorization. Operations requiring new Session commands/read models wait for T014/contract review; UI-only layout/shortcut actions may use existing behavior after owner coordination.

## Likely files / ownership

`ui-mock/react-dock/src/openutau/main.jsx`, menu modules, shortcut registration, batch-edit components, and focused tests. These overlap active T004/T016/T019 files; preserve those owners and allocate one integrator before code changes.

## Acceptance and evidence

- [ ] Every visible action is either connected to an existing authoritative Rust operation or clearly disabled with an explanation.
- [ ] Keyboard shortcuts match their menu actions and preserve focus/text-entry behavior.
- [ ] Batch operations are atomic/grouped in Rust undo history; no partial UI-only changes.
- [ ] Menu/shortcut matrix and browser evidence cover both backends where relevant; T025 regression remains green.

## Concurrency and approvals

After authorization, workflow inventory/documentation can proceed with P1A/P1B fixture work. UI source changes to shared menu files are serialized with T004/T016/T019 owners. No contract or dependency changes authorized.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`; extensive dirty/untracked ownership remains unknown. The inspected current menu files (`main.jsx`, `editor-menus.jsx`, `file-menu.jsx`) are untracked and are recorded as T004-owned; `file-menu.jsx` also has T016 render-menu work. Do not edit without a serialized handoff.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Next: assign an integrator and exact paths for the selection-batch transaction and focused shortcut fix; then inventory/menu-test only verified omissions. Any new batch action must map to stable-ID Rust commands and cancel its whole Session group on failure.

## Current source audit (2026-10-15T05:12:12Z)

- **Visible menus:** File has project lifecycle/import and export actions; Edit exposes only Undo/Redo; Project's Expressions and Preserve Timing dialogs are labeled drafts and do not apply; Tools has layout, singer install/selection and fullscreen. No separate Batch menu/component or batch test file was found in the inspected `src/openutau/` tree. T016's Render Track action is already in File, so T029 must not duplicate it.
- **Shortcuts:** `main.jsx` handles Ctrl/Cmd+Z and Ctrl/Cmd+Shift+Z globally, but does not ignore input, textarea, select, or contenteditable targets. Pressing Ctrl/Cmd+Z while typing is intercepted for project undo, preventing normal text-field undo. Transport shortcuts do skip editable fields. `file-menu.jsx` registers Ctrl/Cmd+N/O/S and Ctrl/Cmd+Shift+S.
- **Multi-note edit behavior:** `NotePropertiesPanel` sends lyric, tone, and vibrato intents for every selected note. `engine.js` expands these into a sequence of stable-ID Rust commands, but `clone-state.jsx` dispatches them ungrouped by default; a single property change therefore does not form one batch undo transaction. Its one-shot `group:true` path commits in `finally`, even if an intermediate command rejects, so it may commit a partial multi-note batch instead of cancelling it. This is a concrete gap against T029's grouped/atomic acceptance.
- **Ownership:** T004 records ownership of `main.jsx`, `editor-menus.jsx`, `file-menu.jsx`, and `note-properties-panel.jsx`; T016 also owns render integration in `file-menu.jsx`. These paths are currently dirty/untracked. No source file was changed and no tests/build were run for this read-only audit.
- **Verification:** current-state `rg`/source inspection only; evidence identifies gaps and ownership, not runtime behavior. T029 remains blocked until the integrator/path handoff is recorded; do not claim batch atomicity or shortcut focus parity.
