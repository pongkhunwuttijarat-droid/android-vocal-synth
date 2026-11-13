# T027 — P1B pitch-curve fidelity and descriptor-backed editing

**Status:** blocked — Wave 1 research complete; implementation remains gated
**Owner:** DSH coordinator (subagent support)
**Last updated (UTC):** 2026-10-15T02:52:03Z

## Scope and deliverables

Validate curve interpolation/display against OpenUtau, preserve expression values and gesture fidelity, create deterministic golden fixtures, and consume authoritative Rust expression descriptors/read context. Separate preview drawing from committed curve data and Rust rendering.

## Existing task relationship

- Reuse [T009](T009-curve-stroke-streaming-simplification.md) for curve stroke transport/reduction; do not duplicate its `curve.apply_stroke` or simplify/commit path.
- Requires the descriptor and HTTP stroke-context reads tracked in [T014](T014-project-session-openutau-controls-extension.md); the v3.1 API and both host adapters now have passing focused tests.
- Runs under the [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md) P1B and the [T025](T025-arrangement-piano-roll-regression.md) gate.

## Dependencies and blockers

- Wave 1 fixture/source research is complete in [the research report](../../planning/t027-curve-fixture-research-wave1.md). T025's Arrangement → Roll acceptance gate is done (T025 Waves43–44), and T014's v3.1 descriptor/stroke-context API is implemented and tested. UI work remains blocked by shared T026/T027 source ownership and a scoped UI handoff. CONTRACT-v1 stays unchanged.

**2026-10-15T02:41:23Z update:** T014 descriptor and first-stroke context reads now pass in the shared Rust Session, Direct-WASM and native HTTP tests. This resolves the API dependency only; no T027 UI files were edited here. The shared Piano Roll owner/handoff remains required before UI integration.

## Likely files / ownership

`ui-mock/react-dock/src/openutau/pitch-curve-input.js`, curve-specific view/component modules, dedicated golden fixtures/tests, and descriptor-read adapters only after approval. `piano-roll-panel.jsx` is shared with T026; agree one integrator or serialize edits. `web/app/curve-stroke-transport.js` remains T009-owned unless its owner agrees otherwise.

## Acceptance and evidence

- [ ] Compare equal curve data at equal zoom for sparse/dense inputs and each supported interpolation shape; identify and resolve mismatches with fixture-backed results.
- [ ] Descriptor min/max/default/color and supported curve list come from Rust, not a frontend static registry.
- [ ] A visual-only preview change cannot alter saved points or render output; committed curve values match Rust snapshots and native/WASM vectors.
- [ ] Golden fixtures cover interpolation, endpoints, dense strokes, stale revision, undo/redo, and descriptor variation; report exact commands and browser evidence.
- [x] T025 Arrangement → Roll selection/viewport gate is green (see T025 Waves43–44).

## Concurrency and approvals

Pure fixture/source research may proceed under the bounded Wave 1 authorization on disjoint files. Changes to shared Piano Roll rendering or public descriptor/stroke-context reads are serialized and require T014/contract approval where applicable. No implementation is authorized now.

## Wave 1 research checkpoint (2026-10-08)

- Fixture/source research completed under subagent `5713f925-5d66-46c6-adac-ea58a3dbc55f`; sole artifact is [the 32-line research report](../../planning/t027-curve-fixture-research-wave1.md). It proposes fixtures for pitch shapes/endpoints, sparse defaults, dense strokes, equal zoom, preview/commit, stale revision, undo/redo, and descriptor variation; separates existing Rust-backed checks from T014-gated UI/API work.
- Corrected `sp` finding: Rust `CubicSplineSegment` and OpenUtau use Catmull–Rom with actual x-distance weighted tangents; UI uses uniform-parameter Catmull–Rom. Uniform knots should coincide; uneven knots may differ in interior samples, while knots stay exact. No tests/builds/Git/source changes. Descriptor-backed reads and HTTP stroke-context remain gated by T014; `piano-roll-panel.jsx` remains shared with T026.

## Wave 2 ownership checkpoint (2026-10-08)

- The read-only audit confirms shared Piano Roll files remain unowned; T026/T027 must serialize edits to piano-roll-panel.jsx. The explicitly assigned web/app/curve-stroke-transport.js remains T009-owned and is not claimed by T027.
- T027 fixture/source research remains complete; implementation stays blocked on T014 descriptor/API review, T025 acceptance, a named UI integrator/clear file ownership, and separate approval. No source or shared UI edit was made.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`; existing UI working-tree ownership remains unknown.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Next: assign one Piano Roll integrator with explicit implementation scope and exact shared/disjoint paths before UI implementation; T014's v3.1 descriptor and stroke-context API gate is already implemented and tested. Use the report and Wave 4 numeric fixture to close only uncovered display/render gaps.

## Wave 3 dependency refresh (2026-10-15)

- T025's previously open gate is done. Its saved-copy browser evidence now covers Arrangement → Roll selection, stale render-result fencing, clean Rust Session state, playback output and mixdown export; see T025 Wave44 for caveats.
- T014's descriptor/stroke-context API is implemented and its Rust, WASM, and HTTP checks pass. Remaining blockers are a named integrator and ownership for shared Piano Roll files, plus scoped UI implementation authorization. The T009 curve-stroke transport path remains out of scope. No source changes were made.

## Wave 4 read-only interpolation audit (2026-10-15T02:52:03Z)

- Compared the UI's `samplePitch()` in `ui-mock/react-dock/src/openutau/pitch-curve-input.js` with the engine's `feed::PitchComputer` and OpenUtau's `CubicSplineSegment` source. UI `sp` uses uniform-parameter Catmull–Rom; engine/reference use x-distance-weighted tangents for non-auto-completed interior spans.
- Deterministic source-formula check using x=[−40, 0, 40, 100], y=[−20, 0, 10, 30] (pitch y units are tenths of a semitone), all `sp`, sampled at x=20: UI returns 5.000; the engine/reference x-weighted formula returns 5.375; the UI is lower by 0.375 pitch units (3.75 cents). This confirms a numeric display/render discrepancy for unevenly spaced controls, but is not an equal-zoom browser measurement, a Rust renderer run, or a committed golden fixture.
- The two formulae still agree at every stored knot; the mismatch is between knots when surrounding x spacing and y slopes are nonuniform. No UI, engine, contract, or fixture files were changed. The bounded research scope was used only to sharpen the gap; implementation remains blocked on shared Piano Roll ownership and authorization.
- Next: assign a single T026/T027 integrator and exact component/test paths, then authorize a separate UI increment. A follow-up owned fixture should compare the rendered curve at equal zoom and confirm preview-only sampling cannot affect committed points or audio render.
