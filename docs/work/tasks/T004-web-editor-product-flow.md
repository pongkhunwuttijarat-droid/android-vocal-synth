# T004 — Complete Web editor breadth and PWA delivery

**Status:** active
**Owner:** Codex (continued by Codebuff, 2026-10-01; resumed by Codebuff, 2026-10-03; continued by DSH, 2026-10-07)
**Last updated (UTC):** 2026-10-15T02:45:05Z

## Goal

Bring the browser editor from a tested vertical slice to the agreed user flow,
covering remaining multi-track/mixer and expression editing, offline packaging,
and a deployable static build where still required.

## Completion criteria

- [x] Make the OpenUtau-style clone the primary Web editor UI at `/`; keep `/openutau.html` as an alias and the generic workbench at `/workbench.html`. Both remain React + Dockview + PixiJS 8 clients of Rust Session semantics.
- [ ] Complete the OpenUtau-style page's production review and close remaining integration gaps without introducing frontend-owned project state. A disconnected mock may remain only as an explicit preview; unsupported project edits must be disabled in a live Session.
- [x] Resolve the v3.1 API boundary with [T014](T014-project-session-openutau-controls-extension.md): connected UI may use engine-backed track create/remove/reorder, tempo/Preserve Timing, tuning/pitch/vibrato and descriptor/stroke reads. Per-phoneme writes remain disabled until T028 supplies resolved stable targets and render consumption; this check does not claim UI wiring is complete.
- [x] Implement the top-menu UI scope chosen by the user: File has USTX/MIDI import picker shells pending engine wiring; Edit Undo/Redo uses the live Session; Project has Expressions and Preserve Timing draft dialogs pending engine wiring; Tools layout/fullscreen are view-only; Help is deferred.
- [ ] Reconcile current editor features with the Web UI and WASM planning checklists.
- [ ] Migrate the production Web `Driver` from the legacy WASM facade to the stable-ID `Engine` Project Session API; selection, viewport, layout and gesture previews remain disposable UI state.
- [ ] Complete WASM lifecycle adapter methods for create/open/import/save/export/reopen and Save As identity fork, using the shared `project-session` semantics and browser storage adapter. Preserve sidecar IDs on save/reopen and mint fresh IDs on portable import.
- [ ] Implement the required stable-ID `part.*`/`project.*` command families in Rust `project-session`, expose them through native and direct WASM adapters, and define atomicity, revision/history effects, and change payloads.
- [ ] Run shared lifecycle and command vectors—including `part.*`/`project.*`—through reference, direct WASM and native adapters. Do not claim full CONTRACT-v2 release/freeze until required adapter vectors pass.
- [ ] Implement agreed multi-track/mixer and expression-lane interactions through Rust commands/session views.
- [ ] Add and verify service worker/PWA behavior if still in product scope. Build now emits a manifest, icon, and shell precache; an actual offline browser launch remains to verify.
- [ ] Document and verify the static build/deploy artifact and supported offline flow. README documents HTTPS/localhost and path-prefix requirements; static output inspection passed, browser deployment remains open.

## Source planning

- [WASM migration status](../../planning/wasm-migration-status.md)
- [Web UI design](../../planning/web-ui-design.md)
- [Roadmap WASM/Web track](../../planning/roadmap/ROADMAP.md)

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base/current HEAD at handoff: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f` (no commits created for this work).
- Preferred primary page: `ui-mock/react-dock/openutau.html` (React 19, Dockview, PixiJS 8 through `@pixi/react`), based on the user's 2026-10-03 preference after comparison with the generic Dock workspace at `index.html`. It is distinct from the older custom-docking `ui-mock/rusted-utau-dock-workspace.html` and legacy `web/editor-test.html`.
- Known T004 edits recorded across this continuation: `ui-mock/react-dock/index.html`, `workbench.html`, `vite.config.js`, `README.md`, `src/openutau/{main.jsx,arrangement-panel.jsx,note-properties-panel.jsx,piano-roll-panel.jsx,file-menu.jsx,editor-menus.jsx,openutau.css}`, this record and the board. Other dirty/untracked paths belong to concurrent work; do not claim them.
- The checkout has many other modified/untracked paths from concurrent work. Do not assume or overwrite those as T004-owned; inspect current `git status` before editing.
- Durable artifacts: source tree, this task record, and `ui-mock/react-dock/tests/manual-checklist.md`.

## Progress

- 2026-10-07T13:55:03Z (DSH): User asked (m01573): “ใหเดวลาเลื่อนแนวนอนให้มัน sync arrangement กับ paino roll ได้มะ”. Implemented shared project-absolute `timelineScrollTicks` as presentation-only state in `src/openutau/clone-state.jsx`; `src/openutau/timeline-scroll.js` converts each pane's pixel scale to/from ticks, and `src/openutau/engine.js` classifies the viewport intent as UI-only. Arrangement derives its DOM scroll offset from the shared tick; the Piano Roll derives its canvas offset from it, scrolls across all project tracks (not only the selected part), and remains the single playback auto-follow authority. The Arrangement ignores programmatic scrollbar alignment events so pixel round-trip quantization does not nudge the shared tick. Scoped files owned by this subtask: `src/openutau/{timeline-scroll.js,clone-state.jsx,engine.js,arrangement-panel.jsx,piano-roll-panel.jsx}` and `tests/timeline-scroll.test.js`; all other working-tree changes remain unowned/concurrent and are not claimed. Branch `wasm-native-rust`, HEAD/base `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`; no commit created. Browser acceptance at `http://localhost:4187/?backend=wasm`: at 100 px/measure and 68 px/beat, setting Arrangement scrollLeft 0→100 moved the Roll to 272 px; Shift+wheel on the Roll moved it 272→372 px and Arrangement to 137 px; resetting Arrangement returned both to 0. `node --test tests/timeline-scroll.test.js tests/arrangement-geometry.test.js` passed 14/14; `npm run build` passed with existing optional-asset and >500 kB chunk warnings. Full `node --test tests/` reported 87 pass, 1 fail, 3 skipped: existing unrelated contract mismatch in `tests/openutau-engine.test.js:52` (`track.create` returns `[{ type: 'track.create', name: 'New Track' }]` instead of expected `null`); three live-server tests require `RUSTED_UTAU_SERVER_URL`. T004 remains active for its broader production/PWA checklist; this horizontal-sync request is complete.

- 2026-10-07T14:52:04Z (DSH): User asked why USTX audio still requires relink and whether playhead-ordered streaming is better (m02303), then noted the editor did not fill the browser viewport after restart (m02397). The imported USTX had five tracks; its off-vocal wave part still used an external relative source path, not an `asset:` reference, so the browser has no stored audio bytes. `File → Relink Audio` must register the selected file in browser storage. In `/media/DATA/project/android-voice-synth/ui-mock/react-dock/src/openutau/clone-state.jsx:496-508`, media warning is now derived from the live project at load rather than being set only after playback. Selected-track waveform preview is a full one-shot render; measurement for `main` (279.3 s output) was 61.299 s after “rendering” appeared and 66.082 s from request. `/media/DATA/project/android-voice-synth/ui-mock/react-dock/src/openutau/wasm-session-runtime.js:68-129` now plans/loads samples only for the selected track preview; full mixdown still prepares all tracks. `/media/DATA/project/android-voice-synth/ui-mock/react-dock/src/openutau/clone-state.jsx:764-830` records render start time; `/media/DATA/project/android-voice-synth/ui-mock/react-dock/src/openutau/main.jsx:256-285` displays elapsed time. `/media/DATA/project/android-voice-synth/ui-mock/react-dock/src/openutau/openutau.css:5-7` resets page/root dimensions; browser verification showed root (0,0) at 1854×972 with document scroll dimensions exactly 1854×972. Full project playback still waits for mixdown before AudioWorklet playback. Rust already provides chunk-count `ProgressFn`, but WASM passes `None`; counts are not time-proportional. Per-track `plan_project`/`render_chunk` and the legacy pool are not safe substitutes for live Session streaming because the mixer omits stateful post-mix FX. Recommended next architecture: Session-owned finalized PCM chunks, sorted/prioritized globally by timeline position near playhead with lookahead; mix/FX in order and feed the Worklet, cancel/reprioritize on seek or revision. Full streaming not implemented. `npm run build` passed; focused tests `session-backends`, `audio-playback`, `timeline-scroll`: 11 passed, 1 skipped. Broader scoped run had the known unrelated mismatch at `tests/openutau-engine.test.js:52` (actual `track.create` envelope vs expected `null`) and three live-server skips. After HMR the current page reset to mock `New Part` and reported partial OTO mapping; USTX preview was not reverified after these edits. Vite is running at `http://127.0.0.1:4187/` under job `bash-1`. T004 remains active.

- 2026-10-03T04:15:00Z: Compared `index.html`'s generic Dock workspace with `openutau.html` visually at the available 412px-wide browser viewport and through the live AX tree. The user prefers the OpenUtau-style Arrangement / Piano Roll / Expression Lanes / Note Properties composition. The clone opened a real Direct-WASM Rust Session at revision 5 and showed a 2.5s RenderPool waveform; the generic workspace still labels render/playback unavailable. Narrow-screen layout remains cramped in both and needs a desktop-width review. Audited live-session controls: disabled add-track and per-note vibrato (no Rust commands), disabled unimplemented Edit/Project/Tools/Help menus and Track FX/settings buttons, and disabled local-only Portamento/Expressions note-property editors while connected. `npm run build` and scoped `git diff --check` pass; build retains the existing optional asset URL and large Pixi chunk warnings. The preview server later stopped and could not be reopened from the browser, so the last control changes were compile-checked but not visually rechecked. Next: finish the supported-feature audit, service-worker offline launch and static deploy acceptance; then reconcile Web planning docs before marking T004 done and proceeding to T013.
- 2026-10-03T04:17:59Z: Attempted the T004 `npm run drag-probe` regression gate after the UI edits. It exited 1 before running cases because the runner could not start/find Chromium DevTools (`chromium devtools never came up`). This is an environment blocker for the test runner; no probe cases were executed. Keep T004 active and rerun when a Chromium DevTools endpoint is available.
- 2026-10-03T04:19:26Z: Promoted the preferred clone to the default static app route: `/` now mounts `src/openutau/main.jsx`; kept `/openutau.html` as an alias and moved the generic Dock workbench to `/workbench.html`. Updated the README boundary. `npm run build` and scoped `git diff --check` pass; static inspection confirms all three HTML pages and app assets are in the PWA precache, and navigation fallback points to the clone-backed `index.html`. Browser offline reload remains unverified.
- 2026-10-03T04:26:50Z: Contract audit follow-up is tracked in [T014](T014-project-session-openutau-controls-extension.md). The connected clone keeps unsupported track creation and per-note vibrato disabled; tuning is a disabled placeholder, and Portamento/Expressions remain disconnected mock fields. Existing part-level expression curves use `curve.apply_stroke` and are not part of this gap. Frozen §6 was not changed; no implementation or tests were run.
- 2026-10-03T04:52:29Z: Audited OpenUtau's actual MainWindow menu handlers and compared them with the clone's top bar. Proposed T004 scope: keep the working File lifecycle/export-Ustx entries; wire Edit → Undo/Redo to the existing Rust session; add Tools → Dock layout presets/reset and Fullscreen as presentation state; add Help → documentation link. Keep other visible menu entries disabled or omit them until supported. Defer project Expressions (descriptor editing), Adjust Tempo/Preserve Timing (must be an atomic Rust musical operation), track/audio import and audio/multi-format export (model/render/codec work), singer/package/install/preferences (host capability), phonetic assistant (T013), cache/debug/log/update/report actions (host or support tooling). This is a scope recommendation, not implementation; no tests run.
- 2026-10-03T05:10:55Z: Implemented the user-selected top-menu pass in `src/openutau/`: File now presents USTX and MIDI import picker shells without reading/importing files; Edit Undo/Redo invokes the existing Rust Session methods; Project opens expression-definition and preserve-timing tempo draft dialogs with Apply disabled; Tools offers layout reset/ratio presets and browser fullscreen. Help remains omitted for later. No project/model state is changed by the UI-only drafts.
- 2026-10-03T05:12:26Z: `npm run build` — passed. Existing optional asset URL and >500 kB Pixi chunk warnings remain. `git diff --check` — passed. Test suite and browser interaction checks were not run.
- 2026-10-03T05:15:55Z: Consolidated the handoff for the next T004 owner. The user-approved top-menu boundary is: USTX/MIDI imports are UI shells only; Undo/Redo is live; Project dialogs are UI drafts only; Tools layout/fullscreen is view-only; Help stays deferred. Known implementation paths, build result, and remaining acceptance gates are listed above. Do not wire project/import placeholders by putting persistent state in React; use the shared Rust session contract or a reviewed extension.

- 2026-10-01T16:05:00Z (Codebuff): **OpenUtau UI clone started as a separate track (user decision: "เก็บไว้ก่อน ไปลอก UI ของ OpenUtau มาก่อน … แต่ละส่วนมาเป็น dock แยกได้").** New standalone page `openutau.html` + `src/openutau/` in the SAME stack (Dockview + PixiJS 8 via @pixi/react, `themeLight`): `main.jsx` (Dockview shell, menu bar File/Edit/Project/Tools/Help + BPM/time-sig chips, three dockable panels: Arrangement / Piano Roll / Note Properties), `arrangement-panel.jsx` (OpenUtau-style track header card: number badge, portrait placeholder, singer lines, M/S/fx side buttons, volume slider row; PixiJS timeline with measure ruler 1–16 and pink part block), `piano-roll-panel.jsx` (vertical tool rail, PixiJS keyboard C2–B7 with black keys + C labels + pink C-row highlight, note grid with beat/measure lines, playhead, lyric labels, expression lane strip DYN/PTD/CLR/ENG/VEL/VOL with per-lane Pixi canvas), `note-properties-panel.jsx` (accordion Basic/Portamento/Vibrato/Expressions with number+slider rows), `mock-data.js` (mock tracks/parts/notes shaped like the Rust snapshot DTOs so panels can later remount on the real backend without prop changes), `openutau.css` (light OpenUtau-like theme). This coexists with the earlier merged Tracks+Mixer arrangement panel in `main.jsx` (kept, incl. its "Split Mixer" dockable option) — the two approaches will be evaluated against each other later per the user's plan. Build passes (`npm run build`); visually verified in the preview browser: Dockview panels, Pixi timeline + part block, keyboard/grid/playhead. Fixed during verification: Pixi canvases rendered blank because `@pixi/react` needs explicit `width`/`height` on `Application` inside Dockview (no ResizeObserver host sizing yet) and wrapping children in a JSX `<Container>` crashed (`Class constructor _Container cannot be invoked without 'new'`); children are now direct `pixiGraphics`/`pixiText` primitives.
- 2026-10-01T14:45:00Z (Codebuff): User reported notes cannot be stretched. Studied OpenUtau's piano-roll interaction (`desktop-ref/OpenUtau/Views/NoteEditStates.cs`, `ViewModels/NotesViewModelHitTest.cs`, `Controls/PianoRoll.axaml.cs`) and implemented parity in `PixiRoll`: **note drag-move** (4px dead zone, snap-floor, tone/position clamps, multi-select move), **edge resize** with OpenUtau's 8px `ResizeMargin` bands (left edge = move+resize pair, min duration = snap unit or 15 ticks, right edge snaps up, clamped to part duration), **pen draw-then-drag-to-length** (tone change while dragging), **eraser drag-to-delete**, and **marquee selection** with multi-select highlight. Every gesture = ONE engine undo transaction via `beginGroup`/`applyGrouped`/`commitGroup` (§6.8) with a promise queue serializing deltas; a failure cancels the group. Click-level verification is pending because the preview tab stopped being composited mid-session (RAF frozen → Pixi renders nothing → hit test empty; not a code bug — same interactions worked while the tab was watched). Command layer fully verified instead: new node tests cover the exact gesture envelopes (`note.add` → minted id from `note.added` change, grouped `note.move`/`note.resize`, commit) and a **live-server test** (`RUSTED_UTAU_SERVER_URL`) proves part.create → note.add → grouped move+resize → commit → correct state → ONE undo reverts the whole drag.
- 2026-10-01T14:20:00Z (Codebuff): First full click-level browser smoke of the React/Dockview/PixiJS UI (preview browser). It found and fixed four real bugs:
  1. **Blank page on boot** — `DockWorkspace` referenced `onError`/`projectName` and destructured `backend/snapshot/sessionReady/dirty/runIntent`, but `AppShell` never passed them; the component threw `ReferenceError` before first paint. Fixed by passing all of them as props.
  2. **Stale Dockview closures** — panel factories run once per panel, so Projects/Tracks/Mixer panels froze the boot-time `backend = null` and every command failed with "Session backend is not connected" even when connected. Fixed by routing all session state through `EditorViewContext` (`ConnectedProjectsPanel`/`ConnectedTracksPanel`/`ConnectedMixerPanel`), matching the existing roll/inspector pattern.
  3. **BigInt revision crash** — `SessionDriver.undo/redo/beginGroup/commitGroup` passed JS numbers into wasm-bindgen `u64` params → `Cannot convert 3 to a BigInt`. Fixed with `bigRevision()` widening in `web/app/session-driver.js`.
  4. **WASM dev-server 403** — Vite refused `web/pkg/engine_wasm_bg.wasm` outside the project root; `server.fs.allow` added in `ui-mock/react-dock/vite.config.js`. Also guarded Pixi `app.screen`/`app.renderer` access before renderer init (`screenSize()` + renderer checks in `PixiRoll`/`CurveCanvas`).
  Additionally added a **CORS middleware to `synth-server`** (BACKLOG §B gap; permissive origin echo, OPTIONS preflight 204, additive — no route changes). Verified with curl and then a full browser HTTP session: backend switch → create project → part.create → note.add → undo → save → server project list all passed. Console is clean after the fixes. Remaining: PWA offline browser check, narrow-window visual review, and curve commit (blocked by the contract gaps below).
- 2026-10-01T14:20:00Z (Codebuff): **Contract gaps confirmed for curve editing** (answering "ต้องเพิ่ม contract ต่อไหม"): (a) no engine-provided **descriptor read for a curve that does not exist yet** — `part.create` seeds `curves: []` per frozen §6.7, and `session_curve_stroke_context_json` requires an existing `curve_id`, so the client cannot obtain min/max/default/tolerance to draw the *first* stroke even though `curve.apply_stroke` supports curve creation (`curve_id: null` → Rust mints the ID); (b) native HTTP v2 has **no stroke-context route** (only bounded `GET …/curves/{curve_id}`), so HTTP curve editing stays disabled. Recommended additions (§6.10 bounded-reads family): `GET /api/v2/sessions/{id}/parts/{part_id}/descriptors/{abbr}` + WASM binding `session_curve_descriptor_json(part_id, abbr)`, and `GET /api/v2/sessions/{id}/parts/{part_id}/curves/{curve_id}/stroke-context`. No command/schema change required.
- 2026-10-01T11:49:55Z: Added production-only PWA app-shell registration and a generated static artifact: manifest, SVG icon, and service worker precaching built HTML/JS/CSS/WASM/fonts/SVG. The worker serves cached shell assets and navigation offline, and deliberately leaves `/api/` and `/projects` requests network-only. README documents HTTPS/localhost hosting, path-prefix behavior, and online-only HTTP sessions. No browser surfaces were available and port 4187 was not running, so install/offline/browser click smoke remains pending. Build and static artifact inspection passed.

- 2026-09-30T16:56:34Z: Fixed vertical Dockview resizing so the Pixi viewport redraws against the renderer's current dimensions and refreshes its stage hit area on every resize. Verified by dragging the Piano Roll splitter larger: note dimensions and row spacing stayed constant while the visible vertical range increased; clicking note 5 afterward selected the matching note and Inspector values. Unified normal-mode Zoom X/Y into one 35–180% control and one 100% reset; both axes now use the same scale. `npm run build` passes; Pixi vendor chunk warning remains. Narrow-window review remains pending.

- 2026-10-01T02:52:48Z: Prepared the WASM-to-Web Session lifecycle adapter independently of React UI integration. Added `SessionDriver` for create/open (including portable import and stored sidecar restore), stable-ID commands/history, canonical export, save, and staged Save As. `ProjectStore` now loads and persists `.ustx` plus `.ustx.rusted-utau-ids.json`, with best-effort restoration of the prior pair on a write failure. Added WASM export/prepare/commit Save As bindings. `cargo check -p engine-wasm`, release `wasm32-unknown-unknown` build + `wasm-bindgen`, JS syntax checks, and the Rust staged Save As identity test pass. Production React wiring and browser OPFS lifecycle verification remain open.

- 2026-10-01T03:01:46Z: Added a local curve gesture preview inside the Dockview Expression Curves panel. PixiJS captures pointer samples into disposable React state, overlays the draft on the illustrative curve, labels the sample count as “Unsent preview,” and offers Clear preview. Verified in the browser by dragging: 9 samples appeared, with no Driver/Session call in the component. `npm run build` passed. This is visual feedback only; it does not mutate or persist a curve.
- 2026-10-01T03:08:42Z: Added a Prepare pack action to the mock Curve dock. It maps the local gesture to an illustrative `curve.apply_stroke` request, runs the existing JS reduction helper against a mock base/descriptor, and displays point counts, JSON, and byte size. Browser check: a 9-sample drag prepared a 2-point, 375-byte packet. UI marks mock IDs/revision and unsent; no Driver/Session call occurs. `npm run build` passes (existing Pixi 640.5 KB chunk warning remains). The candidate is a mock only and is not parity-approved for production transport.
- 2026-10-01T03:41:07Z: Fixed Pixi resize behavior after measuring the canvas: CSS viewport height was 164px while its renderer backing buffer remained 15px, stretching the entire drawing. Both Pixi panels now observe their Dockview host and resize the renderer buffer to the host dimensions; hit areas follow the viewport. Piano Roll zoom is anchored to the document origin, and curve geometry uses fixed world coordinates, so splitter changes reveal/clip world content rather than re-center or stretch it. Browser resize check changed Piano Roll canvas from 839×164 to 839×232 while retaining the same note dimensions; curve canvas changed from 839×171 to 839×103 with fixed curve geometry. `npm run build` passes; the Pixi chunk warning remains.

- 2026-09-30T16:49:15Z: User asked for Zoom X/Y to behave like viewport scaling and for Dock Manager to toggle existing docks. The first zoom pass exposed that Dockview's cached component closures kept stale values; the piano roll now reads ephemeral view state through React context. X/Y zoom applies a centered transform to the Pixi view, and note hit testing uses the inverse transform. Fresh browser checks at 180%/150% and 35%/60% show zooming in/out; selecting a note while zoomed updates the matching inspector values. The 100% control resets both axes. Dock Manager now has accessible visibility switches; Projects hide/show updates the switch state. The mock graphics, curve lane, focus behavior, and fader preview remain verified. `npm run build` passes; Pixi vendor chunk warning remains. Narrow-window review is still pending because this browser control exposes no viewport override. Preview is running at `http://127.0.0.1:4187/`.

- 2026-09-30T15:57:00Z: User confirmed the latest React + Dockview + PixiJS 8 mock (`ui-mock/react-dock/`) is the intended UI base. Adjusted T004/T011 ordering to finish this mock before the single production integration pass. Added mock-only feedback to project and track CTAs, and ephemeral mixer fader previews with clear no-save/no-render labeling; no WASM or production UI changes were made. `npm run build` passes (main bundle 1,021.97 KB minified; Vite warns above 500 KB). Browser interaction/visual review remains unavailable in this environment; see Blockers.
- 2026-09-30T16:09:10Z: The user clarified the workspace has full filesystem access. After the WASM session seed was changed to avoid `SystemTime::now()` on `wasm32-unknown-unknown`, `bash web/run-editor.sh` completed successfully: browser editor boot/edit/undo/draw/marquee, 639343-sample render, four-worker/six-chunk pool with cache reuse, one-chunk invalidation, cancellation/stale-result handling, and OPFS save/open/autosave/delete all passed. This verifies the existing legacy editor path only; stable-ID Session lifecycle and RenderPool integration in the selected React UI remain outstanding. Added keyboard Escape/restore-focus support and screen-reader status announcements to the Dock Manager/status UI. `npm run build` passes after adding separate React, Dockview, and PixiJS vendor chunks; PixiJS remains 640.50 KB and Vite still reports the >500 KB chunk warning. Do not retain the earlier socket-bind failure as a current blocker.
- 2026-09-30T16:26:06Z: Handoff checkpoint. The in-app tab was `file:///media/DATA/project/android-voice-synth/web/editor-test.html` (legacy editor, no dock UI), which is a different page from the chosen React mock at `ui-mock/react-dock/dist/index.html`. I attempted to open the React mock via `file://`; the browser URL policy rejected that protocol and then explicitly prohibited using localhost/Chrome to open that target as an alternative route. The user has explicitly authorized local server + Chrome preview. No local server was started in this turn and the mock was not shown in the browser. A fresh session should inspect its own available browser tools/policy before attempting preview. This was a browser-tool policy restriction, not a filesystem sandbox issue.
- 2026-09-30: Added from planning notes that identify multi-track/mixer, curves, service worker/PWA, and static delivery as gaps. Several checkpoints predate recent changes; verify each one.
- 2026-09-30: UX prototype `ui-mock/rusted-utau-dock-workspace.html` now has a separate Dock Manager and Vocal Edit / Arrangement & Mix workspace switcher. The Dock Manager controls per-workspace panel visibility and left/center/right/bottom location; users can also drag panel headers, collapse panels, use Compose/Curves/Mix presets, and reset layout. Layout and splitter sizes persist per workspace in browser localStorage. Small-screen open actions move hidden-side panels into the visible center. Canvas note selection updates the mock Inspector through a separate local-only controller. This is a standalone visual prototype; it does not connect project mutations, storage, audio mixing, or rendering to the Rust engine.
- 2026-09-30: The user clarified that the standalone HTML/CSS/JS mock is temporary and asked to use the planned stack rather than build more throwaway prototype UI. Created isolated `ui-mock/react-dock/` on React 19 + PixiJS v8 (`@pixi/react`) + Dockview React; existing production `web/` files were left untouched. The stack prototype has a Vite entry, Dockview panel layout/presets, React Dock Manager, Pixi Roll/curve canvases, and a strict mock-only banner. Build passes. Chromium CDP confirms workspace/preset changes and Dock Manager panel close/reopen; true mobile-window visual review remains incomplete. See `ui-mock/react-dock/tests/manual-checklist.md`.
- 2026-09-30: T010 froze the logical contract. User clarified that `ui-mock/react-dock/` (React + Dockview + PixiJS 8) is the chosen visual base; finish that mock before replacing the production editor. Then batch stable-ID WASM Session/lifecycle changes and RenderPool integration in one product pass so the WASM adapter is fixed once. The current HTML editor already wires RenderPool through its legacy YAML Driver; that path is not the target architecture.

## Progress

- 2026-10-02T11:55:00Z (Codebuff) — Surveyed every control OpenUtau exposes in its top bar, piano-roll
  toolbar and track card, and wrote the candidate assessment to
  [`docs/planning/openutau-control-inventory.md`](../../planning/openutau-control-inventory.md) (Thai, with
  file/line references into `ref-openutau-clean/`). Headline findings: OpenUtau's top bar is menu bar +
  click-to-edit time signature/BPM + four transport buttons (Seek Start / Play-Pause / Pause / Seek End) +
  play-position time + render progress, and it has **no skip-by-measure or skip-by-note at all** — the clone's
  ⏮ ◁ ▷ ⏭ row is ahead of it. The genuinely missing, cheap items are `Seek End`, the Tips overlay (T), the
  zoom strips (top = horizontal, right = track height), auto-scroll/lock-start-time/lock-unselected-notes view
  state, then Batch ▸ Notes/Lyrics (all expressible as one grouped `note.move`/`note.set_lyric` transaction),
  then the click-to-edit BPM / time signature / key that `project.set_tempo`, `project.set_time_signature` and
  `project.set_metadata` already support. Blocked on other work: phoneme view (T013), note-tone playback and
  pitch/vibrato/final-pitch lanes (need render data), pitch tools and clipboard (no §6 command for `track.create`,
  `track.remove` or paste identity). No code changed in this entry.

- 2026-10-02T11:25:00Z (Codebuff) — Project management + transport controls on the clone page
  (`ui-mock/react-dock/src/openutau/`), both verified against the live Rust session:
  - **File menu** (`file-menu.jsx`, new): New project… / Open… / Save / Save As… / Export USTX…, wired
    to the real lifecycle in `session-backends.js` — nothing builds a document in the UI. Dialogs:
    name prompt (New, Save As), project picker (Open, names come from the host store), and an
    unsaved-changes confirmation that stands in front of New and Open only. Shortcuts Ctrl+N/O/S and
    Ctrl+Shift+S. Success and failure both land in a toast; the project name and a dirty dot sit in
    the menu bar next to the menus (`storeName` vs the document's own `project.name` differ after a
    Save As, exactly as OpenUtau leaves the song title alone, so the chip tooltip shows both).
  - `engine.js` gained `newProject`/`openProject`/`save`/`saveAs`/`exportUstx`/`listProjects`, plus
    `sessionId`/`storeName`/`savedRevision`/`dirty`/`busy`. Dirty compares the revision against the
    last create/open/save, so undoing back to it clears the dot. A failed lifecycle call is
    deliberately routed around `onError`, which would otherwise drop the whole page back to mock.
  - `clone-state.jsx` exposes `project` through `useClone()` and clears part/note selection whenever
    the session id changes, because every id the panels held belongs to a document that is gone.
  - **Transport controls**: play/pause, stop (keeps the playhead), restart (rewinds and plays), and a
    skip cluster — previous/next measure and previous/next note. Shortcuts Space, ←/→, `,`/`.`, Home,
    Backspace; typing in a field never scrubs. New `skipTarget`/`skipNoteTarget`/`noteOnsets` in the
    reducer: forward lands strictly past the playhead, backward strictly before, both clamp to the
    document, and backward from 0 stays at 0 instead of wrapping.
  - **Real bug found and fixed while testing: one drag landed as several undo transactions.** The
    engine kept a single "currently open group" slot, and a gesture opened its group on pointerdown
    while the previous gesture's release was still draining. A press could therefore adopt the
    previous gesture's transaction (which then closed under it), or fail to find any group and have
    each pointermove batch open-and-commit its own. The fix makes the **gesture own its transaction**:
    the panel mints a gesture id, `engine.beginGroup(name, id)` stores `{groupId, commands}` under it,
    every batch names that gesture (and awaits its still-in-flight open), and `commitGroup(id)` closes
    exactly that one. `endGesture` registers the close in a promise chain synchronously, so a press
    arriving mid-drain still waits for the previous close — needed because a session allows only one
    open group at a time (a second `beginGroup` is refused with `session_busy`, which used to drop the
    page back to the mock document). A refused open is now non-fatal: the gesture continues ungrouped
    and logs `begin-refused`. The engine keeps a small in-memory `state.transactions` log so the probe
    can assert "one drag = one commit", which is impossible to tell from the UI. Verified against
    synth-server directly: three grouped `note.move`s commit once (rev 2→3) and ONE undo restores
    `(0, 60)` from `(60, 66)`.
  - Also fixed while surveying: the Expression Lanes canvas kept the old theme painted after a
    theme switch. Its parent never re-rendered (only the child read the theme), so the hook that owns
    `app.render()` never ran. `useRenderOnDemand` now subscribes to the theme itself, so every Pixi
    panel repaints on a switch.
  - Verification: `npm run drag-probe` — 39 checks green over 3 consecutive runs (incl. the new
    "one drag is one engine transaction"), and the **same probe now passes against the Direct WASM
    backend** (`node tools/drag-probe.mjs '…?backend=wasm'`, 3 runs), so transport, the File menu and
    the single-transaction drag are checked on both hosts. `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node
    --test tests/` — 8 passed / 0 failed. `npx vite build` — passed. Browser (HTTP backend): Save As wrote
    `Probe Save As` to the host store, Open listed it and reopened it, an edit lit the dirty dot,
    Save cleared it, a dirty New raised the confirmation and cancelling kept the project, Export
    produced `Exported "Probe Clone.ustx"`. Browser (Direct WASM backend): Save As → OPFS, Open listed
    `Wasm Probe` + `Smoke Test` and reopened `Smoke Test` (rev 0, `document name "Untitled"`).
    Scratch projects written under `native/projects/` during probing were deleted afterwards.

- 2026-10-02T03:28:51Z (Codebuff) — OpenUtau clone page made interactive and self-consistent:
  - New `src/openutau/clone-state.jsx` holds the mock document, the reducer, the shared
    geometry (`ARR`/`ROLL` + `rollNoteRect`/`arrPartRect`) and the hit tests, so drawing and
    pointer handling cannot drift apart. Every action names the engine command it should map
    to (`note.move`, `note.resize`, `note.set_lyric`, `part.create`, `track.set_mute/...`).
  - Arrangement: clicking a part selects it and opens it in the roll; clicking an empty lane at
    a snapped measure creates a part; track M/S/volume fader are live.
  - Piano roll: viewport-sized canvas with wheel scrolling, select / shift-add / marquee,
    move (one snapped delta for the whole selection), edge resize, pen draw, eraser drag,
    **split**, **pitch** (semitone retune drag) and **vibrato** (toggle patch) tools, waveform
    and pitch-curve overlays, keyboard shortcut column, measure ruler.
  - Note labels toggle lyric ⇄ phoneme (`phonemeOf`, mock table pending engine phonemizer output).
  - Note Properties reads the shared selection; lyric/tone/vibrato rows write back and the
    Vibrato section reflects the roll's Vibrato tool.
  - Expression lanes moved out of the roll into their own dock panel (`lanes`), and the roll
    status is now an overlay chip instead of a column that overflowed the dock.
  - Three real bugs found and fixed while verifying in the browser:
    1. `dockview/dist/styles/dockview.css` was never imported on this page, so every Dockview
       view fell back to static flow and the split overflowed the window.
    2. `rollHitNote` rounded the pointer to a tone, making the lower half of each note row
       unclickable; it now tests the drawn rectangle.
    3. PixiJS event routing was unreliable (empty overlay graphics, and a cleanup that touched
       a destroyed app). Both gesture layers are now plain DOM overlays over the canvases; Pixi
       only draws.
  - Verification: `npm run build` passed (both `index.html` and `openutau.html` entries);
    `node --test tests/` 2 passed / 1 skipped (live-server variant needs `RUSTED_UTAU_SERVER_URL`);
    browser smoke confirmed part create/select, note select, move (tone 70→72), split (4→5
    notes), pen (5→6), eraser (6→5), vibrato tool ⇄ properties panel, and the phoneme toggle.
  - Still mock-only: the document is local state, so undo/redo and render do not exist yet on
    this page; wiring `createSessionBackend` is the next step (see Next step).

### 2026-10-02T03:48:45Z (Buffy) — clone panels now edit the real Rust session

- Branch `wasm-native-rust`, base commit `e77cc1a6`. New `src/openutau/engine.js` is the only
  place the clone turns UI intents into `project-session` commands (`COMMAND_FOR`), with
  `createEngine({kind, baseUrl})` owning boot, `beginGroup/commitGroup/cancelGroup`,
  undo/redo, save and close, plus `seedNotes()` which creates a voice part and four notes
  through real `part.create` / `note.add`. `clone-state.jsx` now keeps UI state and the
  document apart: when a session is live the document *is* the engine snapshot, otherwise the
  mock document renders and the status line says so. Panels dispatch the same intent shapes in
  both paths, so delta gestures behave identically with and without an engine.
- Session picker in the menu bar (`?backend=wasm|http`, `?server=`), undo/redo buttons with a
  `rev N` chip and Ctrl/Cmd+Z(+Shift+Z), and a status line that reports the live document.
- Three defects found and fixed while verifying against the running server:
  1. `send({group:true})` called `api.beginGroup`, which re-entered the same serial queue and
     deadlocked the session; it now opens the group on the backend directly and commits any
     group it opened itself, so a flag-only intent cannot leave a transaction open.
  2. A gesture released before the group-open response arrived (`pointerup` beats the HTTP round
     trip) found no group id, so the commit silently no-opped and the edit stayed pending until
     the next gesture. `beginGroup` now tracks the in-flight promise and `commitGroup` /
     `cancelGroup` await it.
  3. A grouped command answers with `changes: []`; the minted `note.added` id only appears in
     the commit patch. Drawing and splitting therefore select the note from the commit response
     instead of guessing an id in the frontend.
- Also: `activePart` now falls back to the first voice part (engine part ids differ from the
  mock ones, so properties/lanes said "No part" on a live session), and the roll scrolls to
  the active part's notes (`useFollowPart`) instead of opening above the phrase.
- Verification (synth-server on `127.0.0.1:8765`, Vite on `127.0.0.1:4187`):
  - `node --test tests/` with `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765`: 6 passed / 0 failed
    (new `tests/openutau-engine.test.js` pins every intent→command envelope, asserts that
    `track/add` and `note/vibrato` stay UI state, and round-trips boot → grouped drag → undo →
    redo → pen tap against the live server).
  - `npm run build`: passed (both entries).
  - Browser, `?backend=http`: pen tap → `note.add` (rev 5→6, 5 notes, 1 selected), lyric edit in
    the properties panel → `note.set_lyric` (rev 7, confirmed server-side as `[(66,'あ'),(66,'い'),(68,'う'),(70,'え'),(68,'か')]`), undo → lyric back to `あ` in the engine, redo re-applies, arrangement click on an empty lane → `part.create` (rev 6) and undo removes it again.
  - Browser, `?backend=wasm`: same flow in-process (rev 6, 5 notes, 1 selected).
  - Browser, unreachable server: falls back to the mock document, gestures still work locally,
    undo/redo stay disabled and the status line says "Mock document — no engine session reachable".
  - Not verified in-browser: drag gestures (move / edge-resize / pitch / marquee / eraser drag).
    Synthetic pointer events do not reach React handlers in this preview tab, so the browser
    tools can only click; the grouped-drag command path is covered by the live node test above.
- Still not engine-backed: per-note vibrato/curve edits and `track.create` (no §6 command; the
  vibrato patch stays a local, clearly-marked overlay and `track/add` is mock-only).

### 2026-10-02T05:10:00Z (Buffy) — movable waveform overlay, expressions as tabs

- Read the real OpenUtau UI in `ref-openutau-clean/desktop-ref/OpenUtau/Controls/` rather than
  guessing, and modelled both pieces on it:
  - `PianoRoll.axaml` pins the render preview as a `WaveformImage` strip — `Margin="0,0,0,60"
    Height="60"`, `IsHitTestVisible="False"` — bound to `TickWidth`/`TickOffset`, above the
    `NotesCanvas` but below the selection box. `WaveformImage.Render` paints a per-pixel min/max
    envelope in 50% grey and only when the view is zoomed past
    `ViewConstants.PianoRollTickWidthShowDetails`.
  - Expressions live under the notes in a strip of ten `ExpSelector` slots, each bound to one
    expression descriptor and reachable with Alt+1..Alt+0 (`PianoRoll.axaml.cs:OnKeyDown`), the
    whole strip toggled by `ShowExpressions`; a selector is a 22px header with the descriptor
    combo plus its `TagBrush`-coloured name.
- What changed here:
  - New `src/openutau/waveform-overlay.jsx`: the strip is now its own overlay instead of being
    painted into the roll's grid drawing. It keeps OpenUtau's default placement (60px tall, 60px
    above the bottom edge) and its min/max peak rendering, and adds a grab bar to move it, a
    bottom edge to resize it, a close button and a `〜 wave` toggle in the roll status. Placement
    is presentation state, so it lives in the frontend (`waveform/placement`, `waveform/toggle`).
  - `expression-lanes-panel.jsx` is now one tab per expression (abbr + name in the descriptor's
    tag colour, matching the ExpSelector header) with a single full-height curve instead of six
    stacked 30px strips. Alt+1..6 switch lanes as in OpenUtau, points are draggable, clicking
    empty space adds one, and the footer states plainly that curve edits are local because §6 has
    no project-expression command yet.
  - Fixed a regression the resize work exposed: `useFollowPart` marked a part as followed before it
    had notes to centre on, so a freshly opened session could open scrolled past the phrase. It now
    only remembers a part once it centres on real notes, and re-clamps when the dock resizes.
- Verification: `npm run drag-probe` now 15 checks, all passing — waveform overlay at OpenUtau's
  default placement, draggable, hideable, and restored where it was left; six expression tabs with
  one active and Alt+3 selecting the third; plus the earlier canvas, hit-test and drag checks.
  `node --test tests/` 6 passed, `npm run build` passed.

### 2026-10-02T04:35:00Z (Buffy) — "the screen goes white after moving a note"

- Report: dragging a note left the clone page blank. Three real defects, found by driving the
  page with real pointer input (see the probe below) rather than by clicking:
  1. **The roll canvas never resized.** `<Application width height>` only sizes the renderer on
     mount — @pixi/react forwards those props into the canvas root and never resizes, and Pixi's
     `resizeTo` only listens for *window* resizes, while Dockview resizes panels. The canvas
     stayed at its first 900x420 inside a 1128x361 dock, so the rest of the panel was never
     painted and the DOM overlay (which hit-tests in unscaled units) drifted out of step with the
     drawing. `useElementSize` / `useRendererSize` (new `src/openutau/use-element-size.js`, shared
     by both canvas panels) now measure the dock and resize the renderer, retrying on the next
     frames because the Pixi app boots asynchronously.
  2. **The arrangement had the same mismatch**: CSS stretched its canvas to 100% while the hit
     test used unscaled coordinates, so clicks drifted by ~8% of the width. It now measures the
     dock and derives `measureWidth` from it; `arrPartRect` / `arrTickAt` / `arrHitPart` take that
     width so drawing and hit testing share one scale.
  3. **A drag sent one engine round trip per pointermove.** Each one re-read the whole document
     and repainted, which is what buried the UI. Gestures now send *absolute* targets, only one
     batch stays in flight, and `endGesture` waits for it before committing — so a dropped frame
     cannot leave a note behind. Note adds and erases accumulate in the batch instead of being
     coalesced away. A 20-step drag now issues 3 commands.
- A fourth defect surfaced while verifying that: a grouped command is invisible to a session read
  until the group commits, so every delta in a drag was measured from the pre-gesture position and
  the moves **compounded** — dragging up three rows applied +1, +2, +3 and landed six rows away.
  `engine.js` now tracks what the open gesture has already asked for and drops the overlay when
  the group closes.
- Verification:
  - `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` — 6 passed / 0 failed, including a
    new case that drags in six steps and asserts the note lands on the last requested position and
    one undo returns it.
  - `npm run build` — passed.
  - `npm run drag-probe` (new `tools/drag-probe.mjs`, headless Chromium over CDP with real trusted
    input): arrangement canvas fills its dock, clicking a part at 80% width selects it instead of
    creating a new one (the stretched-canvas regression), roll canvas fills its dock, a note can be
    hit, the drag reaches the Rust session (rev 5 → 6), no WebGL context is lost, the note lands
    where the pointer asked (tone 66 → 69 for a 60px drag) and one undo reverts the whole drag
    (rev 6 → 7, tone back to 66), with no uncaught page errors. All nine checks pass.
  - Note: `ui-mock/react-dock/` and `docs/work/` are untracked in git on this branch, so none of
    this work is committed; the board row is the index, the files are the record.
  - Browser: mock fallback still edits locally and says so; undo/redo stay disabled without a
    session.

### 2026-10-02T06:05:00Z (Buffy) — correction: the engine DOES accept expressions

- Earlier checkpoints in this record (2026-10-01T14:20:00Z and the 2026-10-02T05:10:00Z entry)
  framed curve/expression editing as blocked by a missing §6 command. That is wrong, and the
  clone page's local curve state was a self-inflicted gap. Re-verified live against synth-server on
  `127.0.0.1:8765` this session:
  - session capabilities advertise `features: [..., "curve_apply_stroke", "curve_clear"]` and
    `limits.max_stroke_points: 20000`.
  - `POST /api/v2/sessions/{id}/commands` with
    `{"type":"curve.apply_stroke","part_id":…,"curve_id":null,"abbr":"dyn","create":true,
      "mode":"replace","start_tick":3,"end_tick":1919,
      "points":[{"tick":3,"value":-240},{"tick":900,"value":0},{"tick":1919,"value":120}]}`
    returned `command_name: "apply_curve_stroke"`, `revision 1 → 2`, and a `curve.changed`
    patch (`authority: "patch"`, minted `curve_id`, canonical `points` — the engine inserted the
    boundary anchors at the descriptor default: `[{3,0},{5,-240},{900,0},{1919,120}]`) plus
    `render.invalidated`.
  - `GET /api/v2/sessions/{id}` then returned `project.tracks[].parts[].curves =
    [{"abbr":"dyn","id":"1105a3a7-…","points":[{"tick":3,"value":0},{"tick":5,"value":-240},
    {"tick":900,"value":0},{"tick":1919,"value":120}]}]` — so reads need no new route.
  - one `POST …/undo` removed the curve (`curves: []`, rev 3), and a second stroke on `voic`
    came back canonicalised to the `voic` default of 100: `[{3,100},{480,55},{1919,100}]`.
- Real descriptors (CONTRACT-v1 §6.6.1): `dyn` −240…120/0 · `pitd` −1200…1200/0 (cents) ·
  `genc` −100…100 · `brec` −100…100 · `shfc` −1200…1200 · `tenc` −100…100 · `voic` 0…100/**100**.
  The clone's `mock-data.js` `EXPRESSION_LANES` uses invented OpenUtau-fake abbrs
  (DYN/PTD/CLR/ENG/VEL/VOL) with invented modes and must be mapped onto these.
- Where the clone page actually leaks:
  1. `src/openutau/engine.js` has no `curve.apply_stroke` entry in `COMMAND_FOR`, and
     `exp/point` sits in `UI_ONLY` with the comment "§6 has no project-expression command".
  2. `expression-lanes-panel.jsx` keeps its own point list (`expEdits` in `clone-state.jsx`) and
     never reads `parts[].curves` back from the snapshot.
  3. `HttpSessionBackend.readCurveStrokeContext` still throws, so the older main-editor curve
     panel cannot draw the first stroke over HTTP.
- OpenUtau behaviour to match when this is fixed (from `ref-openutau-clean`, not guessed):
  a drag draws a transient preview and commits **once** on pointer-up
  (`NoteEditStates.cs` → `MergedSetCurveCommand`, overwrite mode), with boundary anchors inserted
  so interpolation never reaches far-away points; points snap to 5 ticks only inside `Set()`
  (`UCurve.cs`); the stored curve is simplified (Douglas–Peucker,
  `tolerance = min(5, (max−min)*0.005)`), so a freeform stroke sends thousands of samples and the
  engine keeps the minimum that looks the same (`ExpCommands.cs:327-385`).
- Phonemizer redo split out to its own board entry: [T013](T013-phonemizer-parity-redo.md).

### 2026-10-02T06:40:00Z (Buffy) — role-token theming, expression curves on the engine, `dyn` audible

Three gaps from the correction above are closed.

**1. The clone page is now themeable (Light / Dark / Midnight / Sepia).**
- New `src/openutau/palette.js`: ~90 role tokens per theme, the 18 OpenUtau track palettes copied
  from `ThemeManager.TrackColors` (name + 4 shades each), a `pixi()` hex→int helper, a `P()`
  accessor for canvas draw code, and `useTheme()` (a `useSyncExternalStore` subscription) so a
  theme switch repaints every Pixi panel. Token NAMES mirror ThemeManager's roles
  (`surface`, `text`, `accent`, `rulerBg`, `noteFill`, `expGrid`, `wavePeak`, …), matching the
  named-resource XAML it replaces.
- `openutau.css` now contains **no hex at all** (was 149): tokens are written onto `<html>` as
  `--ou-*` custom properties. Dockview's own chrome is themed by setting its `--dv-*` tokens on
  the `.dv-shell.dockview-theme-light` element itself — see the 2026-10-02T07:35 entry for why the
  obvious descendant selector could not work.
- Presets are JSON in `src/openutau/themes/*.json`, loaded with `import.meta.glob`, each
  overriding only the tokens it names — the same idea as OpenUtau's installable `.oudep` theme
  packages (`ThemeLoader`), minus the package manager. Choice persists in localStorage
  (presentation state).
- Two real bugs found while verifying in the browser: `@pixi/react` applies its `background` prop
  only at mount, so a theme switch left the canvases' clear colour stale (a black band appeared
  under the expression curve in a light preset). Fixed by having each `draw` paint its own
  backdrop over `max(measured, renderer.screen)` — and `RollCanvas` had to stop painting over the
  keyboard column, which `KeyboardCanvas` owns.

**2. Expression curves now go through `curve.apply_stroke`.**
- `engine.js`: added the `exp/stroke` intent → §6.6 command mapping and removed `exp/point` from
  `UI_ONLY` (its comment claimed §6 had no expression command — it does). `emit()` now also stores
  the mapped tracks on `state`, so `engine.state.tracks[].parts[].curves` is readable.
- `mock-data.js`: `EXPRESSION_LANES` replaced with the seven real descriptors from CONTRACT-v1
  §6.6.1 (`dyn` −240…120/0 · `pitd` −1200…1200/0 · `genc` −100…100/0 · `brec` −100…100/0 ·
  `shfc` −1200…1200/0 · `tenc` −100…100/0 · `voic` 0…100/**100**) plus `clampToLane`. The old
  DYN/PTD/CLR/ENG/VEL/VOL list was invented and the engine would have clamped or rejected it.
- `expression-lanes-panel.jsx` rewritten: the drawn curve IS `parts[].curves` from the snapshot;
  a freeform drag paints a transient `previewLine` (per `NoteEditStates.cs`) and commits ONE
  `mode:"replace"` stroke on pointer-up; boundary anchors are inserted so interpolation never
  reaches far-away points; the footer reports the engine-minted id and the canonical point count.
  A `clear` button writes the descriptor default back.
- Contract detail found the hard way: `curve.apply_stroke` rejects fractional values
  (`points[1].value must be an integer` — `UCurve.ys` is `Vec<i32>`), so points are rounded before
  sending. That was the first thing the browser test hit.

**3. `dyn` was never audible. Now it is.**
- `render_input::Curves::dynamics` was built and hashed but nothing applied it:
  `PhraseSynthRs::set_curves` takes f0/gender/tension/breathiness/voicing only, and
  `runtime::mixer::apply_dynamics` — a documented port of `Renderers.ApplyDynamics` — had **no call
  site anywhere in `native/`**. A volume expression saved to the document and rendered as a no-op.
- New `pipeline::apply_chunk_dynamics` calls it in both chunk render paths
  (`render_planned_chunk` and the cached `render_chunk` closure), after the cache lookup and before
  `trim_chunk_audio`, on the chunk's own timeline origin at `ms_per_interval` spacing. The chunk
  hash already covers `curves.dynamics`, so a volume edit still invalidates the entry.
- Semantics kept faithful: samples past the curve's last segment are untouched, and a project with
  no `dyn` curve samples to the descriptor default 0 dB = linear 1.0, which the early-out makes a
  byte-exact no-op. Note the descriptor is in **tenths of a dB** (`-240` = −24 dB) — the first test
  draft assumed plain dB and failed against the real conversion.
- Reference boundary worth knowing: expressions are sampled on the **pitch grid**
  (`pitch_start() .. last_phoneme_end`), so `dyn` scales the sung span and the renderer's own tail
  past the last note keeps its gain.

Verification:
- `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` — **8 passed / 0 failed** (3 skipped
  without the server). New: `expression strokes become one curve.apply_stroke envelope` and
  `a stroke round-trips through the session as an authoritative curve` (create → canonical points →
  snapshot read-back → edit the same curve id → undo restores the first stroke → undo removes it).
- `npm run drag-probe` — **16 checks, all green**. The two expression checks were rewritten against
  the real descriptors, and a new one drives a real CDP stroke across `.ou-exp-canvas` and asserts
  the footer reports "read back from the session" with the revision bumped by exactly one.
- Browser (`?backend=http`): dark theme end to end (dock tabs, arrangement, roll, lanes, waveform
  peaks), sepia preset, a freeform stroke on `dyn` → `curve 0530ad3e · 9 points read back from the
  session` at rev 5→6, undo → "no curve yet — the flat line is the descriptor default".
- `cargo test -p synth-cli --test dynamics_expression -- --ignored` — 2 passed (real
  `libworldline.so`): a flat −12 dB curve scales the sung span by 0.2512 within 2%, nothing gets
  louder, the tail past the grid is byte-identical, and an explicit 0 dB curve is byte-identical to
  the curve-less render.
- Golden parity after the Rust change: `cargo test -p synth-cli --test plan_convention --test
  render_real -- --ignored` — 9 passed, including
  `planned_chunks_mix_bit_identically_to_the_sequential_render` and the four `rust_*_matches_so_*`
  comparisons against `libworldline.so`. `cargo test -p runtime -p project-session -p synth-cli` —
  all green.
- `npm run build` — passed (the existing >500 kB Pixi chunk warning remains).

### 2026-10-02T07:35:00Z (Buffy) — dock chrome follows the palette; control faces get their own roles

The user reported the gaps between dock panels still white, then that the menu-bar controls did not
follow the theme. Three separate causes, all real.

**1. The Dockview theme class is an ANCESTOR, not a descendant.**
`themeLight` puts `dockview-theme-light` on a `.dv-shell` that *wraps* `.ou-dock-stage`, so
`.ou-dockview .dockview-theme-light` could never match — the override block was dead CSS and
`--dv-group-view-background-color` stayed `white`, `--dv-tabs-and-actions-container-background-color`
stayed `#f3f3f3`. The tokens now go on `.ou-app .dv-shell.dockview-theme-light` (one class deeper than
Dockview's own `.dockview-theme-light` rule, so no `!important`), covering both the light and dark
theme class names, plus `--dv-scrollbar-background-color`, `--dv-floating-titlebar-background-color`,
`--dv-floating-border`, `--dv-edge-dock-indicator-color` and `--dv-icon-hover-background-color`, which
had no token upstream. The void container beside the tabs is transparent, so `.dv-void-container`,
`.dv-tabs-container`, `.dv-groupview > .dv-tabs-and-actions-container` paint the token directly and
`.dv-groupview` / `.dv-split-view-container` / `.ou-dock-stage` take `--ou-surface-alt`.

**2. The expression strip's Pixi app had no `ref`.**
`<PixiApplication>` in `expression-lanes-panel.jsx` never got `ref={appRef}`, so `useRendererSize`
found no renderer and the buffer stayed at the 900×150 mount default while CSS stretched it over the
whole 1469×115 strip — the grid lines looked blurred and a dark dead band sat under the curve. Same
class of bug as the `@pixi/react` background note above, and the fix is the one line the other three
panels already had.

**3. Controls were borrowing the slider-knob role.**
`.ou-history button`, `.ou-theme-select`, `.ou-backend-switch select`, `.ou-transport`,
`.ou-snap-select`, `.ou-label-toggle`, `.ou-exp-tab:hover/.on` and the property-panel inputs all used
`var(--ou-slider-knob)` for their face. OpenUtau's slider knob inverts with the theme (`#e4e7ee` in
dark), so a dark theme painted *light* chrome on the dark menu bar. New roles `controlBg`,
`controlBgHover`, `controlBorder`, `controlText` (LIGHT/DARK plus both presets) are the control face
now; `--ou-slider-knob` is only the slider knob again.

Verification:
- `npm run drag-probe` — **18 checks, all green** (was 16). Two new: `expression canvas fills its
  dock` (buffer vs dock box, the regression guard for the missing ref) and `dock chrome uses the
  palette, not Dockview white`, which collects every `.dv-void-container` /
  `.dv-tabs-and-actions-container` / `.dv-groupview` / `.dv-tab` / `.ou-dock-stage` background and
  fails on anything outside `--ou-surface-alt` / `--ou-surface-raised`.
- `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` — 8 passed / 0 failed.
- `npx vite build` — passed.
- Browser: dark and Sepia screenshots after a hard reload. Every frame between panels, the tab strips,
  the transport/undo/redo buttons and both selects now render from the palette.

### 2026-10-02T08:05:00Z (Buffy) — Dock Manager on the clone page

The user could not find Note Properties and asked for a Dock Manager. The panel was there; what was
missing was any way back once a panel is closed. `layoutPanels` runs once in `onReady`, so a panel
closed with its tab's ✕ stayed gone for the rest of the session — and the watermark only said panels
"can be closed" without saying how to undo it.

- New `src/openutau/dock-manager.jsx`: a dialog listing the four panels with a hint line, the dock
  group (or `Floating`) each one currently lives in, and a visibility switch per row. `REOPEN_POSITION`
  mirrors `layoutPanels`, with a fallback to the arrangement when the reference panel is itself closed
  (Dockview rejects a dangling `referencePanel`). Escape, the Close button and a backdrop click all
  dismiss it, and focus returns to whatever had it before — same accessibility contract as
  `DockManager` in `../main.jsx`.
- `main.jsx`: `⚙ Dock Manager` button in the menu bar, plus the watermark line now says where to
  reopen a closed panel.
- The dialog subscribes to `api.onDidAddPanel` / `onDidRemovePanel`. Without that it renders
  `api.panels` as it looked when the dialog opened, so toggling a switch moved the panel but left the
  switch and the location label stale — verified in the browser before the subscription went in.
- Everything the dialog does is view state: no command is sent, the revision does not move, and the
  dialog says so.

Verification:
- `npm run drag-probe` — **23 checks, all green** (was 18). Five new: the manager opens and lists
  four panels, hiding Note Properties updates both Dockview and the switch, the other three panels are
  untouched, the panel comes back on the second toggle, and the dialog closes. The probe also had to
  fix its own panel-name selector, which matched no elements.
- `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` — 8 passed / 0 failed. `npx vite build`
  — passed.
- Browser: opened the manager, hid Note Properties, confirmed the column and the roll resize, reopened
  it back to the right of the roll, Escape closed the dialog. Dark theme.

### 2026-10-02T08:40:00Z (Buffy) — the page was burning ~9 CPU cores while idle

The user asked why running the server and opening the page ate most of the CPU. It was not the
server. Measured on this machine (WebGL falls back to the SwiftShader software rasteriser, so every
pixel is rasterised on the CPU):

| what | cores |
|---|---|
| `synth-server` | **0.0%** CPU |
| browser gpu-process, clone tab open | **8.94** |
| browser gpu-process, tab closed | 0.67 |
| browser gpu-process, tab open **after the fix** | **0.25** |

Counting `gl.clear()` calls per canvas settled it: **69 per 2 s per canvas (~35 fps) on a page nobody
was touching**, producing pixels byte-identical to the previous frame. Pixi's default ticker redraws
the whole scene every frame; these panels only change when React state changes, so the ticker was
pure waste — multiplied by a software rasteriser.

- New `useRenderOnDemand(ref)` in `use-element-size.js`: stops the ticker once the renderer exists
  and calls `app.render()` once per React commit. The render is deferred to the next animation frame
  because `@pixi/react` applies the `draw` callbacks in its own reconciler commit, which lands after
  this component's effects; the app also boots asynchronously, so the first commits retry. Wired into
  all three Pixi panels. `gl.clear()` while idle: **69 → 0**; after a click: 1 repaint per canvas.
- Theme switching still repaints every panel (verified: sepia → dark, each canvas redrew once per
  switch, screenshot shows all four panels drawn).
- The playhead still animates during playback, because it is a React state update per frame —
  clock sampled 00:00.167 → 00:01.020 over 1 s of playback.

**A real bug this exposed.** With the ticker gone, `the waveform overlay can be dragged` in the probe
started failing intermittently (132px instead of 156px = two of eight 12px steps lost). It was never
the ticker: `WaveformOverlay` computed the new placement from `latest.current`, a ref written during
render. React batches the per-pointermove intents, so every move after the first in a batch read a
stale placement and overwrote the previous delta. The ticker's constant repaints had been masking it.
- `waveform/placement` now accepts a **function** of the current placement and the reducer applies it
  to the fresh state; the overlay sends `current => ({...current, bottom: ...})`. The ref is gone.
- New probe check `a burst of pointermoves in one batch all count` fires eight pointermoves inside one
  batch and asserts the strip travels the full 96px. This is deterministic, unlike the original check
  that only failed once in four runs.

Verification: `npm run drag-probe` — **24 checks, all green**, run 3× consecutively with no flake
(the flaky check previously failed roughly 1 run in 2). `RUSTED_UTAU_SERVER_URL=… node --test tests/` —
8 passed / 0 failed. `npx vite build` — passed.

- 2026-10-02: live `curve.apply_stroke` / read-back / undo / `voic` default check against
  synth-server `127.0.0.1:8765` — all passed (output quoted above). `node --test tests/` 6 passed
  and `npm run build` passed earlier the same day; the curve round-trip node test landed later the
  same day (see the 06:40 entry).
- `npm run build` in `ui-mock/react-dock/`: passed after Dockview panel ordering change; Pixi vendor chunk warning remains.
- Browser: fresh local Vite page visually confirmed dock composition, Piano Roll grid/notes/playhead, expression curve, centered viewport zoom in/out and 100% reset, note hit testing at zoom, Dock Manager hide/show switches, Escape/focus restoration, and mixer fader preview. Narrow-window visual review remains unverified.
- Browser: dragging the Piano Roll's horizontal Dockview splitter changed visible canvas height without stretching note/row dimensions; after resize, clicking note 5 updated selection and inspector. Unified Zoom control is present with a 100% reset. `npm run build` passes after both changes.

- 2026-10-01T14:45:00Z (Codebuff): `node --test tests/session-backends.test.js` — 2 passed (incl. new gesture-envelope test); live variant with `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765` — 1 passed (grouped drag round-trip + single undo, real Rust server). `npm run build` — passed. 2026-10-01T14:20:00Z (Codebuff): browser click-smoke (Direct WASM backend): part.create → note draw on Pixi canvas → select → Inspector lyric/tone edit (tone 68→72, rev 3) → undo (tone back to 68, rev 4) → note.delete → mute/solo toggles → mixer fader −6 dB (rev 10) → Save to OPFS ("Smoke Test", 4465 B) → reopen from list with identity sidecar restored (track + voice part + volume −6 persisted, rev 0). `cargo test -p synth-server --test api_v2_sessions` — 30 passed (CORS middleware regression). CORS verified via curl preflight (204 + origin echo) and in-browser HTTP session above.
- 2026-10-01: `node --test tests/session-backends.test.js` in ui-mock/react-dock — passed (HTTP Session create/edit/revision/list/save/export).
- 2026-10-01: `cargo test -p engine-wasm --test shared_adapter_vectors` — 6 passed; `cargo test -p synth-server --test api_v2_sessions` — 30 passed; `cargo test -p project-session --test session_document_commands` — 8 passed (part.create and reversible document commands).
- 2026-10-01: `node --check src/session-backends.js && npm run build` in ui-mock/react-dock — passed (Vite 7.3.6; app 62.81 KB; Pixi vendor 640.50 KB, existing >500 KB warning). Live Vite transformed main.jsx and session-curves-panel.jsx successfully. Browser click-level smoke remains unverified.
- 2026-10-01: `npm run build && node --check dist/sw.js && node -e ...` in ui-mock/react-dock — passed. Generated standalone manifest and worker; inspection confirmed WASM and manifest are in the 10-entry precache. Pixi vendor remains 640.50 KB with Vite's >500 KB chunk warning.
- `bash web/run-editor.sh` passed after the WASM seed fix. This verifies the legacy HTML editor/YAML RenderPool only, not the React mock or Session-backed RenderPool.
- Chromium CDP smoke checks confirm six Dockview panels render, two Pixi canvases mount, workspace/preset changes trigger Dockview layout updates, and Dock Manager closes/reopens Projects with its action label updating; the manager's own Close button closes it.
- 390px device emulation reports no horizontal overflow and a visible Dockview stage. Mobile modal width still needs visual review in a real narrow window because the headless environment kept a wider layout viewport for modal sizing.
- External GUI peer confirmed the page/canvas loaded, but did not successfully verify manager controls; direct CDP checks were more consistent. No production `web/` files were modified.

### 2026-10-02T13:10:00Z (Buffy) — the waveform strip draws real rendered audio

The overlay was drawing `waveformAt(tick)`, a `|sin(t/240)*0.6 + …|` function
that has nothing to do with the project. It is gone: the strip now paints the
engine's own PCM.

- `ui-mock/react-dock/src/openutau/waveform-source.js` (new) is the render
  source, in the order the user asked for:
  1. **`RenderPool` in the browser** — the same N-worker renderer the main
     editor plays through, fed by `web/fixture/bank`. `exportUstx()` supplies the
     project, so the preview is a projection of engine state, never a second
     model.
  2. **The synth-server §6.13 job** (`POST …/render` → poll `GET
     /api/v2/render/{id}` → `GET …/audio`) when the pool cannot be built, e.g. a
     production bundle with no fixture directory. Only offered on the HTTP
     backend, because the server job renders a session the server owns.
  `buildPeaks`/`peaksInRange` reduce one render to a min/max envelope at a
  64-sample bucket, so scrolling never rescans the take.
- `web/fixture-bank.js` (new) locates the dev bank. The clone cannot reach it by
  a relative path — Vite only serves files outside its root through `/@fs/` — but
  `import.meta.url` of a module in `web/` already is such a URL, so Vite
  resolves the base URL at transform time with no build variable and no second
  copy of the bank.
- The re-render follows the session revision with a 400 ms debounce. Because only
  chunks whose hash changed are re-rendered, an edit updates the strip from the
  same cache playback uses.
- While no render exists the strip is empty and **says why** (`rendering…`,
  `no audio — every phrase were skipped`, or the error) instead of inventing a
  shape. The canvas hover text carries the source, the length, and the cache
  split (`0 chunks rendered / 1 reused`).
- Tick↔second alignment uses `snapshot.project.bpm`, not the UI's 120, so the
  strip stays on the grid if the tempo is not the default.

**One product change this forced.** The seeded phrase was `あいうえお`, and the
dev bank is an English CVVC fixture with no kana aliases: rendering it produced
*zero* audio (`no phoneme maps to an oto alias (あ い う え)`, confirmed natively
with `synth-cli` against both the fixture and the full Teto bank). The seed is now
the words the fixture is built for — `see[s i]`, `led[l e d]`, `ah[3]`, `la[l A]`
(`web/fixture/demo-song.ustx`) — in the engine's `word[phonemes]` lyric form. The
render preview is now real audio (2.5 s, peak 0.99) instead of a flat line.

- `vite.config.js`: `worker: { format: 'es' }`. The clone now imports the real
  `web/pool.js`, which starts module workers, and Rollup cannot code-split a
  worker into an IIFE.
- drag-probe grew 7 checks (39 → 46): the strip shows a real render, it is silent
  before the phrase and loud inside it, and a fader move is both heard and
  reported as a re-mix with nothing re-rendered.

### 2026-10-02T13:45:00Z (Buffy) — ▶ plays audio; a render loop fixed on the way

- `audio-playback.js` (new) streams the playhead's range out of `RenderPool` with
  `mixFrom` and reads the position back from the `AudioContext` clock, so the
  cursor cannot drift away from the sound. Seeks go through `transportTarget()`,
  shared with the reducer. Play button tooltip, status line and the transport
  all say which take is playing (`render 2.5s from render-pool`) and, when the
  transport is moving silently, why.
- Fixed a **pre-existing render loop** in the roll: `useFollowPart` and a
  throwaway `{}` in the provider made every render re-run the effect and
  `setScroll` with a fresh object. It was invisible while the transport used a
  slow timer and became a `Maximum update depth exceeded` error as soon as the
  playhead updated every frame. See T011 for the full account.
- drag-probe grew 11 checks (46 → 57), including the audio-clock sanity check
  that keeps a future "playhead runs too fast" report from being a latency
  artefact again.

### 2026-10-03T05:33:29Z (Codebuff) — probe reconciled with the approved top menu; desktop review; honest descriptor draft

Ran the outstanding T004 browser/adapter gates on this machine.

- **Environment note (unblocks the earlier "DevTools never came up" report).** The probe failure
  was not Chromium: `npm run dev` had already died. A backgrounded Vite launched from a tool shell
  dies with `write EPIPE` when its esbuild child is orphaned. Launching it detached
  (`setsid node_modules/.bin/vite … </dev/null &`) keeps it alive, and `synth-server` on :8765 was
  started the same way. The probe's own headless Chromium then came up normally. Do not retain
  "Chromium DevTools unavailable" as a current blocker.
- **`npm run drag-probe` — 67 checks, all green** (was 57). The stale File-menu assertion
  (`length === 5`) was reconciled with the approved 7-item menu (`New project… / Open… /
  Import USTX… / Import MIDI… / Save / Save As… / Export USTX…`, label matched without its
  shortcut hint so `Save` cannot be satisfied by `Save As…`). New coverage for the previously
  unverified top-menu pass: the USTX and MIDI import shells open a disabled picker and read
  nothing; Edit offers live Undo/Redo and a menu Undo→Redo round-trips through the session
  (rev 6→7→8); the Project Expressions and Adjust Tempo dialogs are disabled drafts; Tools lists
  the layout presets + Fullscreen, a ratio preset resizes the arrangement dock (195→348px) and
  Reset restores it. The probe now defaults to the shipped primary route `/` (the `/openutau.html`
  alias is the same bundle) and passes 67/67 there too.
- `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` — **8 passed / 0 failed**.
  `npm run build` — passed (existing optional-asset URL and >500 kB Pixi chunk warnings remain).
- **Desktop-width review (1440×900, dark):** the clone lays out correctly; the File menu renders all
  seven items with hints; the Project Expressions draft modal renders centred with Apply disabled.
  This closes the "only reviewed at 412px" gap for the top-menu pass.
- **Made the Project Expressions draft honest.** It listed invented `DYN/VOL/PITD` descriptors; it
  now shows the engine's real §6.6.1 curve descriptors (`dyn −240–120/0`, `pitd`, `genc`, `brec`,
  `shfc`, `tenc`, `voic 0–100/100`) and states that the session exposes no descriptor read/write.
  A new probe check pins the seven abbrs so a fake descriptor cannot return.
- **Contract audit** handed to [T014](T014-project-session-openutau-controls-extension.md) with a
  verified inventory: command gaps (structural tracks; per-note tuning/vibrato/pitch/phoneme
  expressions; descriptor editing; clipboard; no MIDI `Source`) and read-model gaps (no
  expression-descriptor read anywhere; `curve_stroke_context_json` requires an existing curve;
  HTTP has no stroke-context route). These are read-only proposals; frozen §6 was not changed.
- **Remaining gates (unchanged):** service-worker install/first load/offline reload and the static
  deploy/path-prefix browser check; the T014 contract dependency for the disabled controls; and the
  lifecycle/import identity audit across both adapters.

### Codebuff verification and fixes — 2026-10-03T06:28:00Z

Reported symptom: notes appear impossible to stretch, and the `waveform` overlay does not look
like OpenUtau. Both were investigated against a live session on the Direct WASM *and* HTTP
backends; the evidence is below.

**Stretch is an affordance gap, not an engine gap.** A real right-edge drag on the WASM backend
passes through the session: the note went `480 → 1080` ticks on one `Resize note` transaction
(revision 5 → 6), and stretching the *last* note `480 → 1560` ticks lengthened the rendered take
from `2.5s → 3.6s` (`Render preview from render-pool`), so the engine really sustains the extra
time. The defect is that the roll showed a **static per-tool cursor** and never advertised the
8px resize band (`ViewConstants.ResizeMargin = 8`, identical to OpenUtau), so the handle was
invisible. Fixed in `piano-roll-panel.jsx`: `RollInteraction` now hover-hit-tests like
`NotesCanvasPointerMoved` and shows `ew-resize` over a note edge and `move` over its body (only
for the Select tool, matching OpenUtau). This is ephemeral presentation state, so it stays in the
frontend per the engine-owned-state policy.

**The strip is engine PCM; the *content* is what differs from OpenUtau.** `waveform-source.js`
renders the session's USTX through `RenderPool` fed by the repo dev fixture bank
(`web/fixture/bank`, English CVVC, 6 wavs) with the English seed phrase (`see[s i]…`). So the
shape can never match a user's own OpenUtau project/voicebank. Two further deviations were found
and the second is fixed:

1. `GET /voicebanks` returns `{"voicebanks":[]}` for the fixture layout (`bank/voice/oto.ini`),
   so the §6.13 synth-server render fallback in `renderWithServer` can never produce a preview —
   the server logs `no voicebanks found under ../web/fixture/bank`. The pool path succeeds on both
   backends, so this only disables the fallback. Needs a bank-discovery fix or a documented
   fallback shape.
2. `drawPeaks` inset the envelope by a `0.94` factor, whereas OpenUtau's
   `WaveformImage.DrawPeak` maps a sample `s` to `(0.5 + s * 0.5) * height` clamped to the bitmap.
   Changed to the exact OpenUtau mapping (`waveform-overlay.jsx`); measured effect: the same take's
   tallest drawn column moved `45px → 48px` in a 60px strip, i.e. the envelope now reaches
   full scale exactly as OpenUtau's does.

**Verification (all runs shown in session):**
- `node tools/drag-probe.mjs http://127.0.0.1:4187/` — **all checks passed**, 73 checks (was 67).
- Same probe against `?backend=http&server=http://127.0.0.1:8765` — **all checks passed**.
- New probe coverage pins the regression: the dragged note maps to the engine's copy, the resize
  band shows `ew-resize`, the body shows `move`, a stretch lengthens the note through the session,
  it is one `Resize note` transaction, and Undo restores the length.
- `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` — **8 passed / 0 failed**.
- `npm run build` — passed (same pre-existing optional-asset URL and >500 kB Pixi chunk warnings).

### Render preview reads the host's real voicebank — 2026-10-03T07:06:13Z

Follow-up to the stretch/waveform pass: the preview hardcoded the repo's dev fixture bank, so a
project that named a real singer still rendered the fixture and the strip could never look like
OpenUtau. Fixed, plus the server-side reason the §6.13 fallback could not render at all.

- **New pure resolver, [preview-bank.js](ui-mock/react-dock/src/openutau/preview-bank.js)**
  (`choosePreviewBank`). It picks the sample source only; which bank a project *uses* stays
  Rust-owned (`tracks[].singer_id`, which IS in the frozen §6 snapshot — the old `engine.js`
  comment claiming otherwise was stale and is corrected). Order, mirroring
  `web/app/editor.js`: `?voicebank=<installed root>` / `?voicebank=fixture` → the bank the
  project's singer names (exact, then case-insensitive; `bank/color` ids match on the head) →
  the only installed bank → the dev fixture, labelled as a stand-in. `fixture: false` yields
  *no* source rather than a wrong shape.
- **[waveform-source.js](ui-mock/react-dock/src/openutau/waveform-source.js)** now opens the chosen
  bank (`OpfsBank` for an installed root, `HttpBank` for the fixture), memoizes the OPFS root
  scan for the page, and **rebuilds the pool when the bank identity changes** — a different bank is
  a different alias table, so it cannot be re-fed. `renderProjectAudio` takes `singerId` and returns
  the bank it used; [clone-state.jsx](ui-mock/react-dock/src/openutau/clone-state.jsx) passes the
  track's `singer_id` through and keeps `bank` in render state;
  [waveform-overlay.jsx](ui-mock/react-dock/src/openutau/waveform-overlay.jsx) names the bank in the
  hover title and shows it on the strip, flagging the fixture as `demo bank · … (not the project's)`.
- **`GET /voicebanks` was empty for the fixture layout**, so the §6.13 server render fallback could
  never run: `load_voicebank` requires `character.txt`, and `scan_voicebanks` only treats the root
  itself as a bank when it has one. Added the missing
  [character.txt](web/fixture/bank/character.txt) (`name=Dev Fixture`). The browser `HttpBank` path
  reads only `voice/oto.ini`, so nothing else changes.
- **`engine.js` no longer substitutes a fake singer name.** It kept
  `track.singer_id || 'Unassigned singer'`, which would look like a real singer (and poison the bank
  match); the panels now render their own placeholder instead.

**Verification (all runs shown in session):**
- `curl /voicebanks` → `[{"name":"Dev Fixture","dir":"bank","aliases_count":40,"wav_count":6,"samples_rate":44100}]`
  (was `[]`), and the startup warning is gone.
- §6.13 fallback driven end-to-end against the running server (`/tmp/fallback-check.py`): create →
  `part.create` → 4 × `note.add` → `POST …/render` with bank `Dev Fixture` → poll `done` →
  `GET …/audio` = **220208 bytes, 110082 frames = 2.50s, peak 0.810** — the same 2.5s and ~0.8 peak
  the browser pool draws, so the fallback and the pool agree.
- **OPFS path proven in the browser:** copied the bank into OPFS as `FixtureCopy` through
  `OpfsBank.importFiles`; after a reload the strip read
  `Render preview from render-pool (FixtureCopy) · 2.5s · 1 chunk rendered / 0 reused · the only installed voicebank`
  with the visible label `bank · FixtureCopy`. The test root was then removed and the page reloaded,
  which returns it to `demo bank · dev fixture (not the project's)`.
- `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` — **13 passed / 0 failed** (5 new
  tests in `tests/openutau-preview-bank.test.js` pin the selection order).
- `npm run build` — passed. `node tools/drag-probe.mjs` — **all checks passed, 75 checks** on both
  `http://127.0.0.1:4187/` and `?backend=http&server=http://127.0.0.1:8765`; two new checks assert the
  strip names its bank and that a fixture preview admits it is not the project's.

**Still open:** the preview always renders track 0. Voicebank import now exists in the clone
(`Tools → Singers…`), so a bank installed there is picked up on this origin.

### Import USTX and voicebank install — 2026-10-03T07:41:02Z

**`File → Import USTX…` is now a real engine import.** It was a picker shell that read nothing.
`engine.js` gained `importUstx(name, ustx)` on the `lifecycle` path (§6.11 `imported_ustx`) and
`'import'` joined `CLEAN_REASONS`; `DirectWasmBackend`/`HttpSessionBackend` already had
`importUstx` and now share one signature (`session-backends.js`). Two deliberate semantics:

- **Rust mints every ID**, so an imported document can never inherit identity from the file it
  came from (§6.2.6). Verified: all four note ids differ after importing the page's own export.
- **An imported document is not in the host store**, so `engine.importUstx` clears `storeName`
  and Save refuses with “never been saved — use Save As first” instead of silently overwriting a
  stored project that shares the name.

The dialog reads the file only to hand it to the engine (`ustx-import.js`: the file's own `name:`
wins, the stem is the fallback) and warns before discarding unsaved edits, like New/Open.

**`Tools → Singers… / Install Singer…`** (OpenUtau's own menu location — `menu.tools.singers`,
`menu.tools.singer.install`) installs a voicebank into OPFS and removes one. A voicebank is host
storage, not project state, so this is a host adapter: it writes the bank, resets the preview's
cached bank scan and bumps a `ui/banks-changed` epoch, which is the only thing the change tells
the project. `importFiles` strips the picked folder name, that name becomes the bank root, and a
bank whose `voice/oto.ini` lists no aliases is removed again rather than left looking installed.
The dialog states plainly that installing a bank does not assign a singer to a track — that is
Rust-owned and still has no v2 command (T014).

**`Import MIDI…` was removed — it was an invented feature.** CONTRACT-v1 §6.11 (frozen) defines
only `new`, `stored_project` and `imported_ustx`, and OpenUtau itself has no MIDI import:
`desktop-ref` has `Format/MidiWriter.cs` (export) and the File menu imports *tracks* and *audio*,
with MIDI export-only. **Decision (owner, 2026-10-03): drop MIDI import and follow OpenUtau.** The
File menu now carries OpenUtau's own import entries — `Import Tracks…`
(`menu.file.importtracks`) and `Import Audio…` (`menu.file.importaudio`) — each stating the engine
work it is missing rather than reading bytes: tracks needs a command that adopts a whole track's
contents (v2 can `part.create` but not adopt), and audio is blocked because a wave part is
creatable (`part.create` with `kind: "wave"` + `media_ref`) yet the phrase/render pipeline never
mixes wave parts, so it would be silent. Both are recorded in
[T014](T014-project-session-openutau-controls-extension.md). No MIDI parser was written in the UI
(a second project model would violate the engine-owned-state policy).

**Verification (all runs shown in session):**
- `node tools/drag-probe.mjs` — **all checks passed, 83 checks** (was 75) on both
  `http://127.0.0.1:4187/` and `?backend=http&server=http://127.0.0.1:8765`. New coverage:
  the File menu is exactly `New project… / Open… / Import USTX… / Import Tracks… / Import Audio… /
  Save / Save As… / Export USTX…` (no MIDI import); the import dialog opens with Import disabled
  until a file is chosen; importing replaces the document through the engine with fresh IDs,
  revision 0 and a clean dirty flag; Save then demands Save As; each OpenUtau import states the
  engine work it is missing; Tools offers both Singers items; the dialog installs a real OPFS bank
  (13 files, 6 samples) and the strip switches to `bank · ProbeBank`; removing it returns the strip
  to `demo bank · dev fixture (not the project's)`.
- `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` — **16 passed / 0 failed** (3 new in
  `tests/ustx-import.test.js`). `npm run build` — passed.

## Blockers and dependencies

- Browser interaction is available. The clone renders a live Rust session and audio and was reviewed at 1440×900 on 2026-10-03; a narrow-window (mobile) pass is still open.
- The drag-probe Chromium DevTools blocker was the Vite dev server dying, not Chromium; with the server launched detached the probe runs 75/75. Retained here only as resolved history.
- The synth-server was restarted at 2026-10-03T06:50:20Z (same args, same port 8765) so it would pick up `web/fixture/bank/character.txt`. It now lists `Dev Fixture` instead of warning that the bank was not found.
- First-load and offline PWA reload, static deploy/path-prefix behavior, and the current service-worker artifact have not been exercised in a browser.
- Track creation, per-note vibrato, and several Note Properties groups have no Rust Project Session commands. They are disabled in a live session; enabling them requires engine/contract work.
- T003 is done. Voicebank import/select/delete is integrated in the browser flow; confirm its presentation in the preferred OpenUtau page as part of the final product pass.

## Next step

1. ~~Review the top-menu pass at desktop width; rerun the probe.~~ Done 2026-10-03: 1440×900 review plus the probe on `/` and `/openutau.html` (67 checks then, 73 after the stretch coverage below).
2a. ~~Add import voicebank / USTX.~~ Done 2026-10-03T07:41:02Z: `File → Import USTX…` is a real engine import and `Tools → Singers…` installs/removes voicebanks in OPFS. ~~Decide the MIDI path.~~ Decided 2026-10-03: **drop MIDI import and follow OpenUtau** — `Import MIDI…` is removed, `Import Tracks…` / `Import Audio…` take its place and state their engine gaps, and no MIDI parser was written in the UI. Their enabling work is now a T014 dependency (adopt-a-track command; wave-part mixing in the render path).
1b. ~~Verify note stretching and the waveform overlay.~~ Done 2026-10-03T06:28:00Z: stretch works through the session and lengthens the render; the missing resize cursor is fixed and the strip now uses OpenUtau's exact amplitude mapping. The preview-bank and `/voicebanks` gaps from that pass are fixed too (2026-10-03T07:06:13Z). Remaining: the clone has no voicebank-import UI, and the preview always renders track 0.
2. **In progress:** finish the lifecycle/import identity audit across Direct WASM and HTTP adapters, voicebank controls, supported gestures and mixer behavior; reconcile the Web UI/WASM planning checklist.
3. Verify service-worker install/first load/offline reload and documented path-prefix hosting.
4. Resolve the T014 contract dependency before enabling connected track creation or per-note tuning/pitch/vibrato/phoneme-expression controls. Update completion criteria only after the remaining browser and adapter gates pass, then proceed to T013.

## Handoff notes

Selection, viewport, tools, and gesture preview may remain frontend state. Project edits and musical validation must use Rust semantics.

## Handoff scope for receiving work

- Accepted UI order: finish T004 browser/adapter acceptance first, then continue T013. T014 is a
  separate queued contract-design dependency for track lifecycle and per-note state; it does
  not block the menu UI draft or wiring Undo/Redo/layout/fullscreen.
- File import dialogs only display a browser file picker/name; they do not read bytes, parse
  USTX/MIDI, mutate a project, or claim success. Wire only after a Rust-owned import path and
  identity behavior are agreed.
- Project Expressions and Adjust Tempo dialogs are presentation drafts. Keep Apply disabled
  until typed Session semantics are reviewed; preserving timing must not be calculated in UI.
- Help was explicitly deferred. Do not add Help actions to this handoff's implementation scope.
- Verification so far: `npm run build` passed at 2026-10-03T05:12:26Z with pre-existing optional
  asset URL and large Pixi chunk warnings. Test suite and browser interaction checks remain
  unrun for the top-menu changes.

## 2026-10-03T09:20Z — Multitrack import showed an empty roll; singer install takes `.zip`

**Reported:** importing a multitrack USTX made the notes "disappear" and the audio
track with them.

**Not a data-loss bug.** Traced end to end before changing anything:

- Rust `Session::open` on the user's own file (`test-ustx-example/golden-the-end-of-hutsune-miku.ustx`,
  5 tracks / 4 voice + 1 wave / 1709 notes) keeps every track, part and note — the
  scratch test that dumped the snapshot printed all five tracks with their notes.
- `File → Import USTX…` through the real dialog, on the Direct WASM backend: 5 tracks,
  1709 notes, revision 0, fresh IDs. The `imported_ustx` path was already correct.

**The real cause was the piano roll's horizontal viewport.** `useFollowPart` centred
the *tones* and never touched `scroll.x`, and the horizontal clamp was a fixed
16-beat canvas width. The imported part `harm` begins at **tick 205200** (bar 60 of a
1036-bar part), so the roll sat at bar 1 and drew an empty grid under a header that
read "319 notes" — the notes were there the whole time.

Fixed in `ui-mock/react-dock/src/openutau/`:

- `clone-state.jsx`: new `clampScrollX(scrollX, viewportWidth, contentTicks)` clamping
  against the part's own length. The hard-coded `ROLL.width` seed is gone (unused).
- `piano-roll-panel.jsx`: `useFollowPart` now also scrolls horizontally, but **only
  when the phrase is not already on screen**. Parking unconditionally on the first
  note would have nudged the seeded demo (first note at tick 480) off bar 1 — that
  regressed the drag probe, so the rule became "move only if it would otherwise be
  invisible". `useViewportGeometry` takes the part's tick span for the wheel clamp.
- `piano-roll-panel.jsx`: dev-only `window.__ouRoll` viewport handle (same pattern as
  `window.__ouEngine`) so the probe can measure the viewport rather than assume it.
- `tools/drag-probe.mjs`: new check *"an imported part that starts far from bar 1 is
  scrolled into view"* — imports a 2-track part whose notes start at tick 205200 and
  asserts the roll's visible tick range covers them. Placed at the end of the probe,
  since it leaves an unsaved document behind.

Verified in the browser: before the fix the roll header read "harm · 319 notes" over an
empty grid; after it, the melody, its pitch curve and the part labels draw. The
arrangement renders all five tracks (a screenshot that looked blank was a capture
artifact of a canvas clipped by a short dock — `gl.readPixels` after `app.render()`
returns the real part fills, so no change was made for it).

### Singer install takes `.zip`

`Tools → Singers…` only had a `webkitdirectory` folder picker, but a `.zip` is how
voicebanks are actually distributed. Added `web/zip.js`: a dependency-free reader that
parses the central directory and inflates stored/deflated entries through the platform's
`DecompressionStream('deflate-raw')`. A shared wrapper directory is peeled, but only
when what remains still looks like a bank — peeling a bank that is *only* `voice/…`
would lift `oto.ini` to the root and leave a bank with no aliases (a test caught this).
Paths that would escape the bank root (`../`, absolute) are dropped, not installed.
The root is named from `character.txt`'s `name=` so a project that references that
singer finds it, falling back to the wrapper directory, then the file stem.
`editor-menus.jsx` gained an **Install from .zip…** button beside the folder picker,
running the same "no aliases ⇒ remove it" gate as the folder path.

Verified in the browser with a real 1.2 MB deflated archive of the dev bank wrapped in
a folder: `Installed "Dev Fixture" from ZippedBank.zip — 14 files, 6 samples`, and the
singer list showed it tagged `preview`. `tests/zip.test.js` (5 tests) builds its own
archives so every case is readable.

### Verification

- `RUSTED_UTAU_SERVER_URL=http://127.0.0.1:8765 node --test tests/` → **21 passed / 0 failed**.
- `npm run build` → passed (pre-existing >500 kB Pixi chunk warning only).
- `node tools/drag-probe.mjs` → **83/83 on Direct WASM and on `?backend=http`**, with
  the new deep-start check passing, before the interference described below.
- No Rust source changed in this pass (only `web/zip.js`, UI files, probe and docs),
  so no `cargo test` was run.

### Blocker observed at the end of the pass (not this task's change)

`web/app/session-driver.js` is being edited **by another session** (mtime moved
16:09:04 → 16:12:10 while this pass ran). It now calls
`session_save_pronunciation_sidecar()` and `note.set_pronunciation`, but the served
WASM is `web/pkg/engine_wasm_bg.wasm` from **2026-10-02 20:06**, which has neither.
Consequences, both reproduced: **Save As fails** with
`A Session save requires canonical USTX and both Session sidecars`
(`project-store.js:135`, because `prepare_save_as_json` returns no
`pronunciation_sidecar`), and after 16:12 the WASM session no longer boots at all, so
the page falls back to "Mock only" and most probe checks fail. Not touched here: the
file is not mine and the rebuild would race an in-flight pronunciation change (T013).
**Resolved by rebuilding the bundle** (user's call, 2026-10-03T09:26Z):
`cargo build --release --target wasm32-unknown-unknown -p engine-wasm` then
`wasm-bindgen --target web --out-dir ../web/pkg --out-name engine_wasm
target/wasm32-unknown-unknown/release/engine_wasm.wasm`. The new `web/pkg` carries
`session_save_pronunciation_sidecar` / `session_open_yaml_with_sidecars`, the WASM
session boots again, and **Save As and the Open dialog now pass**.

**One bug remains, in that same in-flight file, and is not fixed here.**
`session-driver.js:175` wraps the sidecar in `resultOf(...)`:

```js
const pronunciationSidecar = resultOf(this.engine.session_save_pronunciation_sidecar());
```

`resultOf` (line 21) JSON-parses, so the sidecar comes back as an **object**, while
`ProjectStore.saveBundle` (`project-store.js:135`) requires
`typeof pronunciationSidecar === "string"`. Plain `Save` therefore always throws
`A Session save requires canonical USTX and both Session sidecars`, and
`New project` then asks to confirm because the project is still dirty. Measured in
the browser: the raw call returns a 106-char string, `JSON.parse` of it is an
object. `Save As` is unaffected because it reads `prepared.pronunciation_sidecar`
instead. The fix is to drop `resultOf(...)` there, matching `ustx` and `sidecar` on
the two lines above — left alone because the file belongs to the session that is
still editing it.

Probe status after the rebuild: **84/84 on `?backend=http`** (the native server
binary was already current, so it never hit this path). Direct WASM: **82 ok, the 2
failures above.**

## 2026-10-03 — the arrangement timeline scrolls sideways (OpenUtau parity)

**The gap.** The arrangement panel squeezed the whole song into 16 columns:
`visibleMeasures` was a constant, `measure = floor(width / 16)`, the canvas was
exactly the dock wide, so the timeline was *always* exactly 16 bars long and no
horizontal overflow was ever created. `.ou-arrangement` had `overflow:auto` and
nothing to scroll sideways. A track whose part starts at bar 60 was unreachable:
the panel drew 16 bars of nothing and the scrollbar that OpenUtau has under its
timeline did not exist. Vertical scrolling already worked (the canvas grows with
the track count).

**What it is now.** `visibleMeasures` is the *opening view*, not the extent:

- `arrangement-geometry.js` (new, plain JS so `node --test` can import it —
  `clone-state.jsx` holds JSX and the runner cannot) owns the whole horizontal
  model: `arrMeasureWidth` (16 measures across the dock), `arrContentMeasures`
  (as many measures as `projectTicks(tracks)` — the engine's own document — plus
  4 trailing bars), `clampArrScrollX`, `arrFirstMeasure`, `arrVisibleMeasures`,
  `arrZoomScrollX`, and the `arrPartRect` / `arrLaneAt` / `arrTickAt` /
  `arrHitPart` geometry with a `scrollX` parameter.
- The timeline is a real DOM scroll container (`.ou-arr-timeline{overflow-x:auto}`)
  over a spacer (`contentMeasures × measureWidth` wide). The scrollbar, a
  trackpad's sideways swipe and shift+wheel are the browser's, untouched. The
  canvas is `position:sticky;left:0` so it stays pinned to the viewport while the
  spacer travels; the drawn offset comes from `scrollX`, which `onScroll` keeps in
  React state.
- Drawing and hit testing both take the offset, so a press lands on what is
  drawn: measure lines, ruler ticks, measure numbers and part labels are computed
  from the first *visible* measure, and off-screen parts are skipped (the draw
  loop no longer walks the whole song).
- Zoom, in OpenUtau's spirit: `−` / fit-16 / `＋` bottom-right of the timeline,
  10–480 px per measure, each step keeping the centre measure under the centre of
  the view.
- `useFollowArrangement` scrolls to the open part for the same reason the piano
  roll does (`harm` in the Hatsune golden begins at tick 205200), and
  `useScrollbarSync` writes the offset back to the scrollbar — without it the
  canvas moved under a scrollbar that still pointed at the old bar, which the
  probe caught.

**Two bugs found on the way, both fixed.**

1. `project/loaded` cleared `activePartId` unconditionally. The engine replays
   the *same* document after a render, so every routine refresh silently threw
   the user back to the first phrase. It now keeps the open part when that part
   id is still in the document, and clears it only when it really is gone.
2. The offset used to live in one place only (see `useScrollbarSync` above).

**Verification.**

- `node --test tests/` → 30 tests, 27 pass / 0 fail / 3 pre-existing skips;
  9 of them are `tests/arrangement-geometry.test.js` (opening view, content
  length, scroll clamp, visible-measure window, zoom anchoring, part rect under
  scroll, lane hit test, row-first hit test, tick under the pointer).
- `npm run build` → pass.
- probe `?backend=http` → **90/90, all pass** (was 87): the timeline scrolls the
  whole song with the canvas pinned at 0px, a press on a block that is only
  visible because of the scroll opens that block's part and adds nothing, and a
  press on empty lane space while scrolled creates the part at the tick under the
  pointer (clicked px 1218 at scroll 7294 → tick 212160).
- probe Direct WASM → 88 ok, 2 fail — the same pre-existing
  `session-driver.js:175` `resultOf` bug recorded above, untouched.
- In-browser, HTTP backend: ruler numbered 3–18 with the part block shifted under
  it at `scrollX 155` (screenshot), a real click on a scrolled part switched the
  roll from 0 to 4 notes, and a real click past its end created a part at tick
  30720 where the pointer was.

**Harness note for the next agent.** Chromium drops the first synthetic press
after a programmatic scroll: nothing reaches the DOM, not even a plain listener
on the overlay (a `mouseMoved` first does not help). The probe warms the input
pipeline with a press on the ruler, which the product ignores, before the two
hit-test checks. That is a CDP artefact, not product behaviour.

## 2026-10-03 — the Direct-WASM Save bug is fixed (user-requested)

`web/app/session-driver.js:175` wrapped the pronunciation sidecar in
`resultOf(...)`. `resultOf` (line 21) `JSON.parse`s its argument, but the WASM
binding is `session_save_pronunciation_sidecar(&self) -> Result<String, String>`
(`crates/engine-wasm/src/lib.rs:722`) — wasm-bindgen already throws the `Err`
and hands back the sidecar *bytes* on `Ok`, exactly like `session_export_ustx`
and `session_save_sidecar` on the two lines above. So the sidecar arrived at
`ProjectStore.saveBundle` (`project-store.js:135`) as an object, its
`typeof … !== "string"` guard threw, and every plain `Save` failed with
`A Session save requires canonical USTX and both Session sidecars`; `New project`
then asked to confirm because the document was still dirty. `Save As` was
unaffected because it reads `prepared.pronunciation_sidecar`.

Fixed by dropping the wrapper (one line) and saying why in a comment, so the
next reader does not "restore" the envelope. The file belongs to another
session's in-flight pronunciation work, but it has been untouched since
2026-10-03T09:19:37Z and the user asked for this fix directly.

**Verification.** probe Direct WASM (`http://127.0.0.1:4187/`) → **90/90, all
pass, exit 0** (was 88 ok / 2 fail: `Save clears the unsaved marker` and
`New project opens straight away when nothing is unsaved`). HTTP backend
re-run after the change → 90/90, exit 0. `node --test tests/` → 30 tests,
0 fail. `npm run build` → pass. No Rust changed, so no `cargo test` run.

## 2026-10-03 — every canvas view scrolls sideways now

Asked "แล้วไม่ add มาในทุกหน้าที่เป็น canvas เลยหละ??" after the arrangement work, and then
"the piano roll too". Audited every canvas surface in the product:

| Surface | Page | Before | Now |
|---|---|---|---|
| Arrangement timeline | `/` dock | **fixed 16 columns** (fixed earlier this session) | DOM scrollbar + `−`/fit/`＋` zoom + follow |
| Expression lane curve | `/` dock | **no scroll at all** — the canvas was the dock wide and everything past it was drawn off the buffer | DOM scrollbar over a `partTicks`-wide spacer, canvas sticky, offset threaded through the grid, the path, the hover readout and the pointer maths |
| Piano roll | `/` dock | scrolled by wheel only (`shift`/trackpad), no visible affordance | the same offset plus `RollScrollbar`: a draggable strip under the grid, `role="scrollbar"` with `aria-valuenow`, press-on-track jumps, press-on-thumb keeps its grab offset |
| Waveform strip | `/` dock | already followed `scroll.x` | unchanged |
| Workbench piano roll | `/workbench.html` | `const offsetX = 0` — zoom was the only view control, so a phrase longer than the dock was unreachable | `offsetX` is state, clamped against the part's length, driven by shift+wheel / horizontal wheel; `offsetX` was already threaded through `readPointer`, `RollGrid` and `Playhead` |
| Workbench arrangement | `/workbench.html` | DOM strip already sized to the document with `overflow:auto` | unchanged |

**The roll scrollbar could not be a DOM scroll.** The keyboard and shortcut columns
have to stay pinned while the grid moves, which is exactly why the roll's offset
is React state; the new strip is the visible half of that state, not a second
source of truth.

**A bug this pass introduced and the probe caught.** The first version of
`RollScrollbar` called `setScroll(offset)` with a bare number while the roll's
state is `{ x, y }`. That replaced the object, `scroll.x` became `undefined`, and
every downstream drawing computation went `NaN` — notes stopped painting, the
waveform strip went blank, the fader stopped re-mixing: 8 probe checks failed at
once, including "no uncaught page errors" (`left NaN is an invalid value for the
css style property`). Fixed by merging into the object; the three new checks now
pin the behaviour (bar spans the part, drag moves the grid and reports the same
number, drag back returns to bar 1).

**Verification.**

- probe `?backend=http` → **95/95, exit 0**; Direct WASM → **95/95, exit 0**.
  New checks: the roll bar spans the part and its thumb is smaller than the bar;
  dragging it moves `scroll.x` 0 → 2199 and the bar reports 2199 (range 2769);
  dragging back returns to 0. Expression lane: 29,670px of lane for a 1,469px
  dock, canvas pinned at 0, and a stroke painted at lane scroll 11,280 commits at
  tick 81,600 — where the pointer was (expected ≈ 81,120, snapped to the 960-tick
  grid) rather than at bar 1.
- `node --test tests/` → 30 tests, 0 fail. `npm run build` → pass.
- In-browser: screenshots of all three scrollbars at once; a shift+wheel on the
  roll moved it 0 → 400; on `/workbench.html` a horizontal wheel moved the
  Workbench roll's `offsetX` 0 → 400 against a 1,079px range.
- **Not covered:** the probe only drives `/`, so `/workbench.html`'s new wheel
  scroll is verified by the dev handle and a synthetic wheel in the browser, not
  by a probe check. The Workbench page is the older prototype route, superseded
  by the dock.

### 2026-10-03T17:20Z (Buffy) — the curve stroke now reaches the engine intact

**Reported:** draw an expression curve and it comes out coarse and with its ends flattened to the
descriptor default. "Simplify dynamic curve first."

**Rust was never the problem.** `commands::simplify_curve` has always run at commit
(`native/crates/commands/src/curve_commands.rs:175`) with tolerance
`min(5, (max-min) * 0.005)`, computed from the descriptor in
`project-session/src/lib.rs:304`. So the simplifier existed; the Web host was destroying the
gesture before the engine could see it, which made the curve *look* unsimplified:

1. `freeform()` re-gridded the painted samples onto `POINT_INTERVAL` — **960 ticks, half a bar** —
   before sending. A drag across a 4-bar phrase could therefore never produce more than 8 points,
   and the detail between grid lines was simply gone. `EXPRESSION_LANES` draws one point per
   half-measure, so the panel was quantising to its own display spacing and calling it a stroke.
2. `commit()` then added **boundary anchors at tick 0 and at `partTicks` using `lane.default`** and
   set `startTick`/`endTick` to `0..partTicks`. Since `mode: "replace"` edits `(start, end)`
   *exclusively*, that interval covered the entire curve: every stroke wiped the whole phrase and
   re-anchored both ends to the default. That is the flattened-ends symptom, and it hit point drags
   and the clear button too.

Both were frontend decisions about musical state, which is exactly what the engine-owned-state
policy forbids — the host was choosing the stored curve instead of describing the gesture.

**Fixed in `ui-mock/react-dock/src/openutau/`:**

- New `curve-stroke-input.js` (plain JS so `node --test` imports it) exposes `strokeEnvelope`,
  `simplifyTolerance` and `strokeSummary`. It **canonicalizes only** — sort by `(raw tick, arrival
  index)`, snap to `STROKE_TICK_INTERVAL` (5, matching `UCurve::INTERVAL`), clamp to the descriptor
  range, round to integer (the engine rejects fractions), collapse duplicates with the later sample
  winning — and returns the **painted span** as the interval. It contains no simplifier, because
  §6.7.2 makes the committed curve the output of exactly one commit-time RDP pass in Rust.
- `expression-lanes-panel.jsx`: `freeform()` sends the pointer's own samples; point drags become a
  minimal one-point replace instead of a whole-part one; `clear` uses the same envelope helper.
  The footer now reports what was sent versus what the engine stored, plus the tolerance in use.
- The optional client reducer in `web/app/curve-stroke-transport.js` stays **off**
  (`CURVE_STROKE_REDUCER_PARITY_VERIFIED = false`, T009). It is transport compression and needs
  JS/Rust parity vectors first; sending canonical samples and letting Rust simplify is the path
  §6.5 explicitly allows and needs no parity proof.

**Verification** — new `tests/curve-stroke-input.test.js`, 8 cases: 5-tick snap with half-away-from-zero
(`7→5`, `8→10`, `-7→-5`), a back-tracking drag keeping the later sample at each tick, two samples
collapsing onto one grid tick, clamping plus integer rounding, the interval being the painted span
and not the part, ticks clamped inside the part, an empty gesture producing no command, and the
tolerance formula (`dyn` ±240 → 1.8, `pitd` ±1200 → capped at 5).

- `node --test tests/` → **38 pass / 0 fail** (was 30).
- `npm run build` → pass.
- probe `?backend=http` and Direct WASM → **all pass**, including two new checks. The decisive one
  paints a **straight** line: 14 samples go out, the engine stores **3** points. A sine stroke
  stores 11 of 12, which is correct (a curve genuinely needs its vertices) and is why the straight
  line is the real proof — it is the case where RDP must collapse to the endpoints. It fails if the
  panel re-grids again, because then the curve arrives already coarse.
- No Rust code changed, so no `cargo test` was run for this pass.

Superseded 2026-10-15: the expression-descriptor read and native stroke-context route now exist
and have Session, Direct-WASM and native HTTP coverage under T014 v3.1. The panel still takes
min/max/default from `mock-data.js`; replacing that fallback and wiring the live view remains
T004/T027 UI work.

### 2026-10-03T18:40Z (Buffy) — a real voicebank zip actually imports now

**Reported:** the Singers dialog and its `.zip` picker exist, but installing a real bank
"doesn't pass". Both archives in `test-ustx-example/voicebank/` were the test case:
`TETO-English-150401.zip` (83.8 MB, 1123 entries) and `TETO-OUset240323.zip` (497 MB, 3920).

**Four independent bugs, all of which failed silently.** Nothing errored: the import reported
success and the singer then had no samples, which is why this read as "import doesn't work"
rather than as four defects.

1. **Entry names were decoded as UTF-8; the archives store Shift-JIS.** Neither archive sets
   general-purpose bit 11 (verified: flags are `0x0`), so the spec's "not UTF-8" path was being
   taken and Shift-JIS bytes were run through a UTF-8 decoder. `重音テト音声ライブラリー`
   became U+FFFD replacement characters, so the bank installed under a mojibake directory.
   `web/zip.js` now decodes UTF-8 when the bytes really are UTF-8 (or the flag says so) and
   falls back to Shift-JIS, which is what Japanese Windows tools have written since 1995 and
   what OpenUtau's own installer reads.
2. **`character.txt` contents had the same problem.** `name=重音テト（かさねてと）音声ライブラリー`
   decoded as UTF-8 produced a bank root of replacement characters — and that root is what the
   preview matches a project's singer against, so the bank installed and then could never be
   selected again. `characterNameFrom` uses the same fallback now.
3. **`oto.ini` contents too.** `keysFromOto` read aliases as UTF-8, so no key matched a file
   that was genuinely on disk. Fixed in `web/samples.js` alongside `characterNameFrom`.
4. **`commonTopDirectory` counted a stray root file as a second top-level entry.** Both archives
   put `install.txt` at the archive root **beside** the bank folder, so the distinct-top-name
   count was 2, no wrapper was peeled, the bank installed one level too deep, and the loader —
   which asked for `voice/oto.ini` — found nothing. It now ignores top-level *files* and counts
   only directories.

**A fifth problem was structural, not a bug in the archives.** `oto.ini` was assumed to be at
`voice/oto.ini`. That is a convention, not a rule: OpenUtau resolves `oto.ini` from wherever the
bank puts it. `TETO-English-150401.zip` keeps its samples in `重音テト英語音源/`, and
`TETO-OUset240323.zip` has a **0-byte `oto.ini` at the bank root** next to the real ones in
per-colour folders — so "prefer the shallowest" alone would have picked an empty file. New
`findOtoDir(paths, sizes)` prefers the shallowest `oto.ini` **that is not empty**; the install
dialogs and the preview loader (`waveform-source.js` → `OpfsBank#listOtoCandidates`) all go
through it now.

**Verified against both real archives, end to end** (Node, reading the actual files):

| | TETO-English-150401 | TETO-OUset240323 |
|---|---|---|
| wrapper peeled | `重音テト音声ライブラリー` | `重音テト OU用日本語統合ライブラリー` |
| bank root (from `character.txt`) | `重音テト（かさねてと）音声ライブラリー` | `重音テト（かさねてと）OU用日本語統合ライブラリー` |
| `oto.ini` used | `重音テト英語音源/oto.ini` | `重音テト単独音/oto.ini` (not the empty root one) |
| aliases parsed | 558 | 142 |
| **aliases resolving to real files** | **558 / 558** | **142 / 142** |
| wav + `_wav.frq` companions | 558 + 558 | 1822 + 1822 |

Before the fixes these archives yielded mojibake paths and zero resolvable aliases.

**Regression risk caught by the probe, not by the tests.** Changing the install gate's error
messages made two voicebank probe checks fail — because the folder path passed
`webkitRelativePath` (which still has the top directory) to `findOtoDir` while `importFiles`
writes with the top directory **stripped**. The lookup was therefore looking one level above
where the files landed. Fixed by deriving the paths the files were actually written under, and
both probe runs are green again.

- New `tests/zip.test.js` cases (3): a Shift-JIS bank name round-trips using the **real bytes**
  read out of `TETO-English-150401.zip`'s central directory; a stray top-level `install.txt`
  does not hide the wrapper; and `oto.ini` is found where the bank puts it (named folder, root,
  bare root, shallowest-wins, and absent).
- `node --test tests/` → **41 pass / 0 fail** (was 38).
- `npm run build` → pass. probe HTTP **97 ok / 0 FAIL**, Direct WASM **97 ok / 0 FAIL**.

**Not verified in a browser:** installing these two archives through the real file picker. They
are 84 MB and 497 MB and `readZipEntries` inflates every entry into memory before writing, which
is a 0.57 GB peak for the larger one — see the limitation recorded on the board. The parse,
name-decode, wrapper, `oto.ini`-selection and alias-resolution paths are all verified against the
real bytes in Node; the OPFS write of a bank that size is not.

### 2026-10-03T21:45Z (Buffy) — the non-clone UIs are gone; the harness follows

**Reported:** "wrong UI? we don't want that one — delete what isn't the OpenUtau clone."

Owner chose the widest scope: everything that is not the clone, including the legacy
`web/` editor. Backup taken first at `/tmp/rusted-utau-legacy-ui-2026-10-03.tar.gz` (19 files),
because **nothing under `ui-mock/react-dock/` is in git** — `git ls-files` returns zero for
that tree, so a deletion there cannot be undone from history.

**Deleted (19 files).** `ui-mock/`: `index.html`, `index-mixer.html`, `index-om.html`,
`rusted-utau-dock-workspace.html`, `rusted-utau-web-redesign.html`, `vocal-studio.html`, `dock-manager.js`,
`dock-selection.js` — leaving `ui-mock/react-dock/` as the only UI project. Inside it:
`workbench.html`, `src/main.jsx`, `src/session-curves-panel.jsx`, `src/styles.css`.
In `web/`: `editor.html`, `editor-test.html`, `app/editor.js`, `app/driver.js`, `app/roll.js`,
`app/viewport.js`.

**Kept deliberately, with the reason.** `web/app/project-store.js` was deleted and then
**restored from the backup within the same pass**: `web/app/session-driver.js` imports it, and
that file belongs to another in-flight session (T013 pronunciation work). The build failed with
`Could not resolve "./project-store.js"` before it was restored. It is a storage adapter, not a
UI. Likewise `web/zip.js`, `samples.js`, `pool.js`, `fixture-bank.js`, `engine.js`, `worker.js`
are imported by the clone, and `web/app/{session-driver,session-render-contract,curve-stroke-transport,r2-runtime}.js`
are the session/transport layer. Deleting `web/` wholesale would have broken the clone outright.

**Follow-on edits:** removed the `workbench` entry from `vite.config.js` `rollupOptions.input`;
rewrote the README session-boundary paragraph. The remaining test/dev pages under `web/`
(`index.html`, `smoke.html`, `r2-test.html`, `vocoder-test.html`, `pool-test.html`, …) are
engine benchmarks, not editor UIs, and were left alone — say the word if they should go too.

**`web/run-editor.sh` now drives the clone** (owner decision), not a restored legacy page.
The old harness pointed at `web/editor-test.html`, which no longer exists, and it is cited as a
regression gate in the roadmap, the WASM migration status, `web-render-backend.md`,
`web-ui-design.md` and BACKLOG. It now rebuilds engine-wasm + bindings, serves the clone, and
runs `ui-mock/react-dock/tools/drag-probe.mjs` against it, keeping the same contract
(boot → edit → undo → render → non-zero exit on failure).

Two harness bugs found by running it rather than by reading it:

1. **`vite preview` of a production build fails every render/audio check.** The render preview
   reads a real voicebank from `web/fixture/bank/`, which lives outside the clone and is not
   copied into `dist/`, so the built bundle has no samples — 7 checks failed with "no render
   appeared" / "bank null". The harness serves the **dev** server instead, which also makes it a
   regression harness for the source a developer is editing.
2. **`$out` was referenced before assignment** after the rewrite (the server log path), and
   `set -euo pipefail` would have aborted on it.

**Verified:** `./web/run-editor.sh` → **97 ok, exit 0**, `==> OK`. `bash -n` clean.
`npm run build` → pass (only `index.html` + `openutau.html` entries). `node --test tests/` →
**41 pass / 0 fail**. probe on both backends → **97 ok / 0 FAIL** each.

**Behaviour change worth knowing:** the harness no longer prints a `RESULT: PASS` line — the
probe's exit status is the result. Any script or doc grepping for `RESULT` must use the exit
code now. `docs/planning/wasm-migration-status.md` and the roadmap record historical PASS runs
against the old harness; those rows describe what was true when they were written.

### 2026-10-03T22:20Z (Buffy) — dragging a note now follows the pointer

**Reported:** "dragging a note doesn't feel like it goes anywhere" — and yes, a preview should
not be hard.

**What was actually wrong.** `note.move` inside a gesture is a grouped command, and a grouped
command produces **no snapshot change until the group commits**. The commit happens on
pointer-up. So the sequence was: press → drag (nothing moves on screen) → release → the note
jumps to its final position. It looked like dragging was broken.

The data to draw the preview already existed and was already correct —
`engine.js` keeps a `pendingNotes` overlay of what the open gesture has asked for, built by
`trackPending` from each accepted command. It existed to stop drags *compounding* (each delta
was measured against the pre-gesture state, so dragging up three rows applied +1, +2, +3).
`findNote` consulted it; **nothing ever drew it.**

**Fixed by publishing the overlay rather than by inventing a second model:**

- `engine.js`: `publishPending()` puts the overlay on `state.pending` (array) plus a
  `state.pendingNotes` counter, and is called from `trackPending` and after every accepted
  grouped command — with an `emit()` right there, so the preview appears when the engine
  accepts the command rather than when the batch finishes, which would have kept the lag.
  Cleared on commit, cancel, undo and redo alike, exactly like the overlay's existing lifetime.
- `clone-state.jsx`: `activeNotes` overlays the pending entry on the engine's note. The
  dependency is the counter, because mutating a Map in place would not change array identity
  and React would not re-render.

**Why this is not frontend-owned project state** (AGENTS.md): the overlay is a projection of
an *in-flight request the engine has already accepted*. It is never persisted, exported or
rendered, and it is dropped the moment the engine answers — after which the read is
authoritative again. The committed document is unchanged by this.

**Verified with a new probe check that reads the Tone row mid-gesture, before the release** —
the exact moment the bug lived:

- HTTP: `tone 66 → 68 while still dragging, expected ≈ 69`
- Direct WASM: `tone 66 → 69 while still dragging, expected ≈ 69`

98 checks, 0 fail on both backends. `node --test tests/` → 41 pass / 0 fail. `npm run build` →
pass. `./web/run-editor.sh` → **98 ok, exit 0**.

---

## F. Freeform pitch curve + OpenUtau parity (2026-10-03T23:40Z)

The pitch curve was drawable but not editable, and one line of the drawing code did not even
run. Both are fixed, and the parity gap that mattered turned out to be a units trap.

### What shipped

- **A `PitchCurve` tool** in the roll's tool rail (`⌇`), next to the existing `Pitch` tool. They
  stay separate because OpenUtau separates them: `Pitch` retunes in semitones (`note.set_tone`),
  the curve tool draws the note's `pitch.data`.
  - drag across a note body → paints points, one per 3px of travel, capped at
    `PITCH_MAX_POINTS` (120);
  - press on a point → drags it; x is clamped between its neighbours, y over ±12 semitones;
  - alt/ctrl/cmd-click a point → deletes it;
  - right-click a point → cycles its shape through `io l i o sp` (OpenUtau's per-point shape
    menu, as a cycle);
  - hover a point → the cursor says `move`, which is the only affordance for a 3px circle.
  - One gesture is one `note/pitch` intent, absolute like `note/move`, so a dropped frame cannot
    leave the curve behind. One `note.set_pitch`, one undo step.
- **A shape picker, a `snap_first` toggle and a `clear` button** in a bar over the grid
  (`.ou-pitch-bar`), shown while the tool is active. `snap_first` is the field the engine
  stores with the curve, not a local flag.
- **The curve is drawn over the note body, points on top**, only for selected notes. Drawing it
  for every note put a line through every note in the part: the demo notes all carry a flat
  two-point curve.

### Three real bugs found while proving it

1. **`pitchCurves` was a `ReferenceError`.** The memo was scoped inside `RollInteraction` but
   `RollCanvas` is rendered by `PianoRollPanel`, so the roll threw on render. The build passed —
   rollup does not resolve free identifiers — which is why only a real browser found it. The
   memo now lives with its only consumer.
2. **The hit test compared screen pixels with world coordinates.** `pitchPointPx` subtracted the
   scroll while the handler had already added it, so with `scroll.y = 686` **no pitch point was
   ever hoverable or grabbable**. Both sides are world coordinates now.
3. **`domain::PitchPoint.x` is an f32, and the editor was building f64-precision curves.** Two
   points 1e-6 ms apart are distinct in JavaScript and the *same* f32 at a note's 500 ms end, so
   `SetNotePitchCommand` refused the whole curve with "pitch point x values must strictly
   increase" — a rejection with nothing on screen to explain it. The editor now merges at
   `max(1e-3, |x| * 1e-6)` ms (`sameX`) and clamps point drags into a gap the engine can see
   (`xGap`). This is a units bug, not a tolerance knob, and it is pinned by a test.

### One deliberate behaviour worth knowing

`pitchOf` **normalizes on read**: a curve from a file need not span its note (the demo song
stores `x: -40 … 40` as a placeholder), and `feed::PitchComputer` completes both ends of
whatever it is handed. The roll therefore draws and edits the completed curve. Without it the
anchors sat outside the note and every painted sample was dropped — the stroke did nothing.
`normalizePitch` gained `clampY: false` for the read path so a file's curve is drawn as stored
even where it reaches further than a gesture may put it.

`SetNotePitchCommand` now also refuses a **negative** first `x`. Files legitimately contain such
points and the import codec still reads them; a command is a finished curve the editor drew, and
the editor always anchors at 0.

### Verified

*(Counts in this subsection are the state at this point in the work; §F2 below supersedes them.)*

- `tests/pitch-curve-input.test.js`: 30 cases — ms/tenths conversion, shape round trip, anchoring,
  degenerate-curve repair, the f32 collapse, paint/move/delete/cycle, and `samplePitch` compared
  term-for-term against `feed/src/music_math.rs`'s easings.
- `note_pitch.rs`: 6 pass, including the new negative-x refusal.
- Probe: 9 new checks — the five shapes, the default shape, a drawn curve stored as points
  spanning the note with flat ends, mid-stroke preview, one transaction, hover, alt-click
  delete, one undo, clear, one undo.
  - HTTP `?backend=http` → **all checks passed, exit 0**
  - Direct WASM `?backend=wasm` → **all checks passed, exit 0**
- `node --test tests/` → 71 tests, 68 pass, 0 fail, 3 skipped. `npm run build` → pass.
- `./web/run-editor.sh` → **OK, exit 0**.

**Not verified:** no pixel screenshot of the drawn curve. This environment's preview webview is
not composited, and the roll canvas is WebGL without `preserveDrawingBuffer`, so readback comes
back empty; adding it to production code for a test was not worth it. The hover check is the
substitute — it hit-tests the drawn circle's own geometry and found the painted points.

### Also fixed here, unrelated to pitch

`buildPitchCurve` dropped the stroke's final sample position: it pushed only the last *value* at
the note end, losing where the pointer finished. Ends are now anchored by **holding** the first
and last value flat before and after the stroke, which is also what the renderer does.

## F2 — freeform drawing, and why it is not point-based

### The request, and the decision behind the numbers

Freeform pitch drawing was asked for twice: first "ขอวาดแบบไม่ได้เป็น point base เลย" (it did
not work), then "เราไม่อยากได้ pointbase implementation" (a point-based implementation is not
what is wanted). Density was left to OpenUtau. So the sampling rule is not a taste decision; it
is copied from the reference implementation.

`ref-openutau-clean/desktop-ref/OpenUtau/Views/NoteEditStates.cs`, `DrawPitchState.Update`:

```csharp
int tick = vm.NotesViewModel.PointToTick(point);
var samplePoint = vm.NotesViewModel.TickToneToPoint((int)Math.Round(tick / 5.0) * 5, ...);
```

**One sample per 5 ticks, snapped on the absolute tick** — not per pointer-move event, and not a
pixel distance. That is `PITCH_SAMPLE_TICKS = 5` in `pitch-curve-input.js`, and the probe asserts
every painted point lands on it. `PITCHPointEditState` in the same file is OpenUtau's *other*,
point-based mode; it is deliberately not what the tool defaults to. It remains reachable as
grab / alt-click-delete on the points the freeform stroke lays down, which is the useful half of
it.

`feed/src/pitch.rs` completes a curve's ends by holding the nearest value flat (`y: points[0].y`
at the start, the last `y` at the tail). The editor's "the ends follow the stroke" rule mirrors
that, so what is drawn and what the renderer will do agree.

### The f32 trap, again — and this time for real

The earlier fix merged near-equal x values with a tolerance. That works, but it treats the symptom.
`domain::PitchPoint.x` is an **f32**, so the honest fix is to stop producing values that f32 cannot
represent distinctly: every x is `Math.fround`ed as it is built, `sameX` is exact equality (the
engine's own rule), and `xGap(x) = max(1e-4, |x| * 1.2e-7)` is one ulp-ish of slack for drags.
A curve assembled from f64 arithmetic can hold two points a fraction of an ulp apart that are the
*same* f32, and `SetNotePitchCommand` then rejects the entire curve with "pitch point x values
must strictly increase" — invisibly, and reproducibly on repaint.

### Five more bugs, all found by drawing rather than by reading

1. **A press had to land inside the 18 px note rect.** The natural gesture for a rising pitch is
   to start *above* the note, so the press missed and nothing happened. `pitchHitNote` now
   matches on the note's x span only.
2. **`paintPitchPoints` clamped out-of-range samples onto the ends**, flattening the note's tail
   instead of extending the stroke. Out-of-range samples are now dropped.
3. **A sample landing exactly on an anchor was not recognised as painted**, so painting across a
   note added a duplicate anchor. `paintedStart` / `paintedEnd` track what the stroke actually
   covered; anchors inside the stroke follow it, anchors the user painted on stay put.
4. **`buildPitchCurve` was deleted.** It was a second, redundant anchoring implementation that
   could disagree with `paintPitchPoints`. Those two are now the single authority.
5. **My own probe bug:** making `alt` also disable grabbing meant alt-click could never delete.
   The probe was lying about a passing check. Its mid-stroke Tone read was also a one-frame flake
   (one row behind because the last move had not rendered yet) — now polled.

### Verified

- `tests/pitch-curve-input.test.js`: **33 cases**, now including the 5-tick grid and the f32 collapse.
- `node --test tests/` → 74 tests, **71 pass / 0 fail**, 3 skipped. `npm run build` → pass.
- `tools/drag-probe.mjs`: **12 pitch checks** (10 unconditional, plus the alt-click delete and its
  undo inside the hover-found-a-point branch) — every painted point on the 5-tick grid, every
  point an exact f32, hover→`move` cursor, alt-click delete, undo, clear, and that a stroke can
  start above the note body.
  - HTTP `?backend=http` → **all checks passed, exit 0** (re-confirmed 2026-10-04 after a Freebuff
    restart; synth-server and the Vite dev server had to be restarted)
  - Direct WASM `?backend=wasm` → **all checks passed, exit 0**
- `./web/run-editor.sh` → **OK, exit 0**. `git diff --check` → clean.

**Still not verified:** no pixel screenshot of the drawn curve — the preview webview is not
composited here and the roll canvas is WebGL without `preserveDrawingBuffer`. The hover
hit-test against the drawn circle's own geometry is the substitute, not a substitute for a human
eye.
