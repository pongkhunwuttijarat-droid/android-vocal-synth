# T021 — Reverse-engineer OpenUtau UI page by page

**Status:** done  
**Owner:** DSH session  
**Last updated (UTC):** 2026-10-08T05:30:40Z

## Goal

Reverse engineer OpenUtau in detail, one UI/page at a time, beginning with a complete screen/page inventory as requested by the user. Compare checked-in desktop Avalonia UI sources with the Web clone target `ui-mock/react-dock`. This is an observation/specification task first; do not implement UI parity or alter engine behavior under this task. Keep the engine-owned state rule: project contents, musical edits, history, serialization, and render planning stay in Rust; the frontend only owns presentation/ephemeral input state and sends engine commands.

## Completion criteria

- [x] Catalog every desktop screen, embedded editor surface, modal, utility window, and its entry/exit workflow; label which are pages vs dialogs/controls.
- [x] Produce source-linked detail coverage per screen: layout/hierarchy, visible controls, menus/context actions, keyboard/mouse behavior, selection/focus states, validation/errors, dismissal/transitions, and empty/loading/disabled states.
- [x] Crosswalk each reference surface to the Web clone and record present, partial, missing, and clone-only UI without implying a backend route/operation exists unless verified.
- [x] Identify which state is authoritative Rust project state vs disposable frontend presentation state for each interaction.
- [x] Review the page order and findings with the user before opening any implementation-parity work.
- [x] Record the verification boundary separately from static source inspection; no live visual checks were performed or claimed.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`
- Baseline relevant working-tree state at task start: `ui-mock/react-dock/**` was already untracked (including browser-profile data); `docs/work/**` and many planning docs were already untracked; [`README.md`](../../planning/README.md), [`MS.md`](../../planning/milestones/ms3/MS.md), and [`wasm-migration-status.md`](../../planning/wasm-migration-status.md) were modified. Ownership of those pre-existing deltas was unknown. Do not claim, revert, or overwrite them. [`openutau-control-inventory.md`](../../planning/openutau-control-inventory.md) is a pre-existing partial inventory and remains unchanged.
- This task owns its task record, the screen inventory, the auxiliary-surface detail pass, and the T021 board row; it has not changed UI or engine source.
- Durable inventory: [`openutau-ui-screen-inventory.md`](../../planning/openutau-ui-screen-inventory.md); 29 auxiliary window detail sheet: [`openutau-ui-detail-pass-auxiliary-surfaces.md`](../../planning/openutau-ui-detail-pass-auxiliary-surfaces.md). Board: [`BOARD.md`](../BOARD.md).
- Reference: `ref-openutau-clean/desktop-ref/OpenUtau`; confirmed Web target from existing T018/T019 handoffs: `ui-mock/react-dock`.

## Progress

- 2026-10-08T04:14:26Z — Read the task board and existing T018/T019 handoffs; confirmed this is a separate reverse-engineering/spec task, not a replacement for their parity implementation.
- 2026-10-08T04:14:26Z — Catalogued 31 Avalonia `Views/*.axaml` surfaces. Mapped Welcome and MainWindow editor states, clone shell and four dock panels, and the first source-level control/flow notes in [`openutau-ui-screen-inventory.md`](../../planning/openutau-ui-screen-inventory.md).
- 2026-10-08T04:14:26Z — Source inspections used `read`/`grep`; no live desktop/browser screenshot, build, or tests were run. A source-only observation is not yet visual or behavioral acceptance.
- 2026-10-08T04:18:30Z — Created this durable handoff, the inventory, and its board row; no UI/engine source files were changed.
- 2026-10-08T04:31:29Z — Detailed source walkthrough of Splash→Welcome and its New/Open/Recent/Template/Recovery/close paths; documented the MainWindow editor grid and context-menu surfaces. Still source-only; no live screenshot/build/tests.
- 2026-10-08T04:36:01Z — Added a source-linked detail sheet for 29 auxiliary XAML windows/dialogs (MainWindow and detached Piano Roll remain separate primary surfaces); mapped arrangement/timeline/part gestures from MainWindow handlers. Clone crosswalk for the dialogs and live visual checks remain open.
- 2026-10-08T04:37:51Z — Detailed the desktop TrackHeader: height-based visibility, singer/phonemizer/renderer menus, volume/pan input/reset, mute/solo menus, selection and duplicate/reorder/settings context actions; mapped legacy DocManager command/undo flows. Still static-source only.
- 2026-10-08T04:48:52Z — Source-mapped the Piano Roll's lazy embedded/detached lifecycle, grid/toolbar/menus, note/phoneme/expression gestures, shortcuts, Note Properties and React dock contrasts; expanded inventory sections 2.4–2.6. No UI/engine source was changed; no build, test, or live visual run was performed.
- 2026-10-08T04:58:41Z — Mapped MainWindow File/Edit/Project/Tools/Help flows, export preconditions, Project Expressions and Remap Timeaxis, plus React file dialogs; expanded inventory sections 2.7 and 3. Verified Rust v2 primitive tempo/time-signature commands but no `engine.js` mapping; recorded source-level File→Open guard asymmetry and runtime verification boundary. No UI/engine source changed; no build/tests/live visual run.
- 2026-10-08T05:05:57Z — Integrated the source-only Web crosswalk for all 29 auxiliary XAML surfaces into [`openutau-ui-detail-pass-auxiliary-surfaces.md`](../../planning/openutau-ui-detail-pass-auxiliary-surfaces.md#L192-L226). Distinguished actual session routes (singer assignment, Mix FX, vibrato) from host OPFS or UI-only lookalikes; corrected Voice Color Mapping status to missing, with Bright/Dark called out as non-parity. No UI/engine source changed; no build/tests/live visual run.
- 2026-10-08T05:10:03Z — Expanded inventory section 2.3 with a source-linked desktop/Web Arrangement and TrackHeader crosswalk: timeline/part gestures and menus, absent Web operations, static/disabled controls, viewport behavior, and Rust-session vs local/mock state. Corrected stale broad “unverified” text in section 2.2 and stale Web reducer comment about `track.create`; no UI/engine source changed or runtime/build/test verification performed.
- 2026-10-08T05:15:06Z — Closed source-audit coverage for the 31-window catalog, primary editor surfaces, and 29-window auxiliary crosswalk; marked those criteria complete and left user review as the sole open criterion. Removed stale “next track-header pass” and “provisional inventory” wording. Documentation-only; no build/tests/live UI run.
- 2026-10-08T05:30:40Z — User approved the page order/findings and selected moving to parity planning. T021 audit is complete; no UI/engine implementation was authorized or performed.

## Verification

- Source inventory: `glob("ref-openutau-clean/desktop-ref/OpenUtau/Views/*.axaml")` returned 31 files; relevant XAML/JSX components were read and line-anchored in the inventory.
- Build/tests: not run; this checkpoint changes documentation only.
- Static source coverage: 31 XAML windows were catalogued; Welcome/start, MainWindow arrangement/timeline/part gestures, TrackHeader, Piano Roll, MainWindow menus, Project Expressions/Remap Timeaxis and clone workflows were examined. The 29 auxiliary-surface sheet has source-linked desktop notes and a completed per-surface Web crosswalk; primary surfaces are crosswalked in the inventory.
- Visual page-by-page run: not performed; source findings are explicitly separated from runtime behavior.

## Blockers and dependencies

- No blocker to the source-level audit. Live desktop UI access and an interactive walkthrough have not been established in this phase; capture those separately if available before claiming visual details.

## Next step

T021 is complete after user review. The inventory covers all 31 desktop XAML windows plus embedded editor surfaces and their Web crosswalk. Parity planning is the next phase; live visual/interaction checks were not performed, and no parity implementation was started.

## Handoff notes

- The source-only screen inventory and Web crosswalk are complete; the task remains active solely for user review of page order/findings before implementation parity begins. Live visual/interaction verification was not performed.
- Desktop source path: `ref-openutau-clean/desktop-ref/OpenUtau`. Web clone path: `ui-mock/react-dock`.
- Keep the old partial [`openutau-control-inventory.md`](../../planning/openutau-control-inventory.md); do not overwrite it. Preserve existing unknown working-tree changes and register only clearly owned deltas.
- The Rust state-ownership boundary in [`AGENTS.md`](../../../AGENTS.md) and [`engine-owned-state.md`](../../architecture/engine-owned-state.md) is a hard constraint for later UI recommendations.
