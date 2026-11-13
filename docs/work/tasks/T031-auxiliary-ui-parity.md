# T031 — P4 project menus, dialogs, and auxiliary UI parity

**Status:** blocked — needs separate implementation authorization  
**Owner:** unassigned — proposed Auxiliary UI lane  
**Last updated (UTC):** 2026-10-15T05:14:38Z

## Scope and deliverables

Close the reviewed low-frequency parity gaps for project menus, dialogs, and auxiliary screens using the existing OpenUtau UI inventory. Deliver a prioritized, source-backed feature list and implement only supported, in-scope surfaces after authorization.

## Existing task relationship

- Reuse completed [T021](T021-openutau-ui-page-by-page.md) screen inventory and its auxiliary-surface detail; do not repeat reverse engineering.
- Coordinate visible menus with [T004](T004-web-editor-product-flow.md), [T019](T019-openutau-web-ui-parity.md), and [T029](T029-editor-workflows-shortcuts-batch.md).

## Dependencies and blockers

- The T021 source crosswalk is already complete for all 29 auxiliary surfaces; do not repeat discovery. Its current classifications remain source-only, with no live visual/keyboard verification.
- A focused prioritization from that crosswalk: defer surfaces needing missing engine/session semantics (for example lyrics replace, descriptor CRUD, track color/settings, paste parameters, and note defaults) until their Rust command/service contracts exist. Existing Mix FX and singer assignment are already session-backed; do not duplicate them. A small presentation-only surface could proceed after its source path is explicitly assigned.
- No isolated implementation path is currently confirmed as owned by T031. The React `openutau/` tree is untracked in this checkout, and visible menus/dialog entry points overlap T004/T019/T029 (and File menu work overlaps T016). Preserve these files until the relevant owner hands off exact paths. Any project-editing dialog requiring missing Rust-owned semantics waits for T014/contract review; disabled placeholders must not claim success.

## Likely files / ownership

New auxiliary components/dialog modules under `ui-mock/react-dock/src/openutau/`, menu integration, and isolated UI tests. Avoid shared `main.jsx` and menu files without the T004/T019/T029 integrator's coordination.

## Acceptance and evidence

- [ ] Prioritized surface list maps each item to T021 inventory and documents why included/deferred.
- [ ] Implemented screens use engine/session state for project data; only layout, focus, and transient dialog state stay in UI.
- [ ] Unsupported features remain unavailable with accurate messaging; no duplicate of completed track/media/mixdown controls.
- [ ] Browser acceptance and accessibility/keyboard evidence are recorded; T025 remains green.

## Concurrency and approvals

Can start with isolated auxiliary component discovery after authorization; menu/project operations are serialized with T029 and wait on contract approval where required. No contract or dependency edits authorized.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`. `docs/work/BOARD.md`, this task record, and the React `openutau/` tree are untracked working-tree paths; preserve their unknown ownership except for this task record/board handoff update.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Evidence: [`openutau-ui-detail-pass-auxiliary-surfaces.md`](../../planning/openutau-ui-detail-pass-auxiliary-surfaces.md#web-clone-crosswalk-source-only) maps all 29 desktop auxiliary surfaces to current Web status and nearest source; [`openutau-ui-screen-inventory.md`](../../planning/openutau-ui-screen-inventory.md#section-3) records menu/dialog flows. This is static-source evidence only.
- Next: obtain an exact source-owner handoff for one small P4 slice and coordinate its menu entry with T004/T019/T029 (T016 for File menu). Then implement only the selected surface and record browser/accessibility verification. No product source was changed and no UI/build/test verification was run in this audit.
