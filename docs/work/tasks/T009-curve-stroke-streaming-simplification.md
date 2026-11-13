# T009 — High-frequency curve stroke transport and OpenUtau-compatible simplification

**Status:** blocked — the Rust/adapter reducer gate is complete; live pointer-up integration awaits a bounded T004/T033 source handoff  
**Owner:** Codex  
**Last updated (UTC):** 2026-10-15T05:21:36Z

## Goal

Implement reliable, payload-efficient curve drawing while preserving Rust as the sole authoritative owner of committed curve/project state. Keep the UI preview disposable, allow safe optional pre-send payload reduction, and commit each completed gesture as exactly one Rust mutation/undo step/revision.

## Specification

### Gesture and authority

1. On pointer-down the frontend captures an ephemeral gesture preview against `base_revision`, `part_id`, `curve_id` and the current curve read from Rust.
2. Pointer moves update only the transient preview/raw sample buffer. They do not mutate the committed project, revision, undo/redo, save data, or render input.
3. On pointer-up the frontend submits one semantic `curve.apply_stroke` command (within advertised command/point limits). No mutation is sent per pointer event.
4. Frontend MUST reduce the outgoing payload before every send. It must merge the stroke with an authoritative committed curve at the selected `expected_revision` according to §6.6.2, then use the exact simplification rule in §6.7.2. If the gesture's original base is stale, read the latest curve and rebase the interval replacement onto it before reducing. Never fall back to raw points; if the latest base cannot be read or reduction cannot be completed, resync/abort the commit.
5. Rust always repeats validation/canonicalization/interval merge/simplification against committed state after checking the stable IDs and `expected_revision`. Client reduction is only transport compression, never authoritative state. Rust returns the canonical committed patch or invalidation/read at the resulting revision; frontend discards/reconciles its preview from that response. If a write races after the latest-base read, re-read and recompute against the new revision; never send the stale or raw payload.
6. Stale revision, invalid points, payload limits or adapter failure must not partially alter document/history/render input. Cancel/pointer-cancel before commit drops preview. Save/export/render use committed Rust state only.
7. One completed curve stroke = one document mutation, one public revision transition, and exactly one undo step. Undo restores the pre-stroke curve byte/structure-exactly and redo restores the canonical committed curve.

### OpenUtau-compatible stored curve simplification

After the `curve.apply_stroke` interval operation (§6.6.2), simplify the **entire affected curve**, once at commit, matching `UCurve.Simplify()`'s observed gesture-merge boundary (`ref-openutau-clean/core/OpenUtau.Core/Commands/ExpCommands.cs:327-375`, `UCurve.cs:120-161`):

- `tolerance = min(5, (descriptor.max - descriptor.min) * 0.005)` in descriptor value units.
- Recursive RDP using perpendicular distance to the segment between span endpoints; retain an interior point only when its distance is strictly greater than tolerance.
- A whole stroke may reduce to its two endpoints only when every interior point of that span is within tolerance. Otherwise retain the furthest violating point and recurse until every emitted chord passes tolerance. The resulting command's `points[]` may contain many retained vertices (multiple adjacent chord/interval pairs); never force the complete gesture into one endpoint pair.
- Preserve first and last point. Scan interior points in ascending tick order; on equal maximum distance retain the first point (matching the reference's `height > maxHeight` update).
- Simplification may remove redundant control points outside the replaced interval when they lie within tolerance of the retained polyline, but MUST NOT modify other curves/entities or change the curve beyond the declared tolerance.
- The simplifier receives strictly increasing x ticks after §6.6.2 canonicalization. Fewer than 3 points return unchanged. Zero-length endpoint segment is guarded.
- Native and WASM MUST make identical retained-point decisions. Use deterministic/scaled arithmetic if normal floating-point comparison is not identical across targets; no host-specific epsilon.
- Render evaluation interval (5 ticks) is not a requirement to persist a point every 5 ticks; stored curves remain sparse controls plus linear interpolation.

### Required tests

- Strict threshold: distance equal to tolerance is removed; just above tolerance is retained.
- Endpoints always remain; collinear points are removed.
- Equal-distance tie selects first by ascending x.
- Narrow spike below tolerance is removed; spike above tolerance remains.
- Uneven x spacing; negative-valued descriptor; descriptor ranges that make `range*0.005` below/above cap 5.
- Full-curve behavior: redundant points outside edited interval may be removed, but unrelated curve IDs/entities remain unchanged.
- Large gesture: 500+ pointer samples cause one committed command/undo/revision; pre-send compression is optional and gated on matching base revision.
- Cancel/stale/validation/payload failure leaves Rust document, public revision, undo history and render input unchanged.
- Undo exact pre-stroke restoration; redo exact post-simplification restoration.
- Native and WASM produce structurally identical canonical curves/change payloads for shared vectors.
- Save/render before commit use committed state only; save/render after commit use only Rust-canonical points.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f` (checkpoint recorded from current checkout).
- Relevant working-tree changes: checkout contains extensive unrelated/unowned changes. This session changed `native/crates/commands/src/{curve_commands.rs,lib.rs}`, `native/crates/project-session/src/lib.rs`, `native/crates/project-session/tests/session_curves.rs`, `native/tools/synth-server/tests/api_v2_sessions.rs`, `docs/CONTRACT-v1.md`, this record, and `docs/work/BOARD.md`. Existing unrelated dirty files remain unassigned.
- Durable artifacts: specification above; current Rust baseline is `native/crates/project-session/src/v2.rs` (`curve.apply_stroke`) and `native/crates/commands/src/curve_commands.rs` (canonicalize/interval edit), with OpenUtau reference in `ref-openutau-clean/core/OpenUtau.Core/Ustx/UCurve.cs` and `Commands/ExpCommands.cs`.

## Progress

- 2026-09-30: Compared OpenUtau source. It applies `SetCurveCommand` on edit updates, merges compatible curve edit commands, then invokes `UCurve.Simplify()` in `SetCurveCommand.Merge()` before constructing the merged undo command. The simplifier applies to the curve list, not only the pointer's replacement interval.
- 2026-09-30: Drafted §6.7.2 with OpenUtau-compatible whole-curve commit-time simplification, tolerance/threshold/tie/endpoints, determinism and conformance requirements. Updated the high-frequency data-flow note and freeze proposal.
- 2026-09-30: Product clarified that curve values may be high-detail and pre-send point reduction is desirable. §6.5 now permits optional frontend payload-only reduction only from a Rust curve base at the exact expected revision and only with the same parity-tested simplifier; Rust still validates and simplifies at commit. If parity/base revision is absent, send raw within limits or resync—never silently truncate.
- 2026-09-30T14:17:37Z: Added recursive RDP simplification to the Rust curve command and wired descriptor-range tolerance at commit. Added threshold/tie/endpoints, whole-curve simplification, exact undo, 500-sample single-commit, and native HTTP/direct WASM curve parity coverage. Clarified that zero-length endpoint spans cannot occur after strict-x canonicalization.
- 2026-09-30T14:19:21Z: Focused command/session/server tests all pass, including dense input, simplification, undo, and native HTTP/direct WASM canonical-curve parity. T009 acceptance is met; exact-revision frontend payload reduction remains an optional future optimization because no frontend curve gesture implementation exists yet.
- 2026-09-30T17:09:34Z: Product now requires simplified points on every outgoing curve stroke. On a concurrent revision change, rebase the interval replacement over the latest Rust curve and recompute; never send raw samples. Reopened T009 for frontend transport reduction and adapter wiring.
- 2026-10-01T02:54:51Z: Added `web/app/curve-stroke-transport.js` with full-curve base merge, Rust-matching canonicalization/RDP, latest-base rebasing, bounded revision-conflict retries, and no-raw behavior. Added `Session::curve_stroke_context_json` and the WASM `session_curve_stroke_context_json` read so the reducer receives revision, base, clamp/default values and the exact tolerance from Rust; `Driver.applyCurveStroke` now routes one reduced v2 commit. Reducer remains fail-closed behind `CURVE_STROKE_REDUCER_PARITY_VERIFIED = false`. `cargo check --manifest-path native/Cargo.toml -p project-session -p engine-wasm`, `node --check` for the changed JS modules and `git diff --check` pass. Browser/Rust parity vectors have not been run, and the current React Dockview curve mock still has no pointer-up gesture wired to the Driver.
- 2026-10-01T03:08:42Z: The React Dockview mock now has a Prepare pack step that calls the reducer against explicitly mock curve/descriptor data and shows the candidate `curve.apply_stroke` JSON, reduced point count and byte size. Browser check produced 2 points from 9 raw samples (375 bytes). This remains a non-authoritative illustration with mock IDs/revision; it does not call `Driver.applyCurveStroke` and does not bypass the disabled parity gate for real commits. The production send path is still unconnected.
- 2026-10-01T03:41:07Z: Clarified the pack rule: only emit a two-point chord when all samples in that span pass the descriptor tolerance; otherwise recursively preserve violating vertices, allowing multiple adjacent interval pairs in `points[]`. UI labels the fixed tolerance/IDs/revision as mock values and reports the number of accepted adjacent intervals. Refactored the transport seam to expose `prepareSimplifiedCurveStroke()` separately from commit so future UI can inspect a fresh Rust-context pack without committing. The real reducer remains gated off pending native/WASM parity; no Session or Driver is connected to the UI.
- 2026-10-01T05:32:31Z: Updated the mock pack to expose each retained adjacent chord as its own `{start_tick,end_tick,start_value,end_value}` interval pair, alongside the flattened `curve.apply_stroke` preview. Added validation for strictly increasing retained ticks. A hand-built curved 5-point smoke input retained all 5 points / 4 pairs at tolerance 5, and retained 3 points / 2 pairs at tolerance 25; it was not forced to one global endpoint pair. UI still labels the mock descriptor/IDs/revision and sends nothing. `npm run build`, JS syntax check and targeted `git diff --check` pass. Rust/WASM parity remains unverified; this interval list is preview data, not a replacement for the frozen v2 command envelope.

## Verification

- `git diff --check -- docs/CONTRACT-v1.md docs/planning/high-frequency-editing-flow.md docs/planning/contract-v2-freeze-proposal.md docs/work/BOARD.md docs/work/tasks/T009-curve-stroke-streaming-simplification.md` — passed 2026-09-30.
- Source inspection via `read` on the OpenUtau files above — confirms gesture commands are merged and simplified at merge boundary. No source algorithm or Rust simplifier has been implemented/tested in this task.
- `cargo test --manifest-path native/Cargo.toml -p commands curve_simplification -- --nocapture` — passed (2 tests).
- `cargo test --manifest-path native/Cargo.toml -p project-session --test session_curves` — passed (12 tests).
- `cargo test --manifest-path native/Cargo.toml -p synth-server --test api_v2_sessions` — passed (21 tests).
- `cargo test --manifest-path native/Cargo.toml -p project-session --test conformance_vectors` — passed (2 runner/schema tests).
- `git diff --check -- <T009/T010 changed files>` — passed.

## Blockers and dependencies

- The frozen CONTRACT-v1 curve command, Rust commit-time simplification, reducer parity vectors, latest-base retry, and one-revision/undo/redo transport tests are complete; the older draft-contract and missing-parity blockers below are historical.
- Current UI source audit: `expression-lanes-panel.jsx` sends `exp/stroke` through `engine.js`; `curve-stroke-input.js` canonicalizes/snap-merges input but does not call the T009 reducer. `web/app/curve-stroke-transport.js` owns the parity-gated reducer, but the live UI does not route pointer-up through it.
- The UI integration is blocked on a bounded source-owner handoff: `expression-lanes-panel.jsx`/`curve-stroke-input.js` overlap T004, while `clone-state.jsx` has a T033 streaming-only ownership slice and `engine.js` is shared. Do not edit these paths until the handoff names the exact T009 slice and preserves T033's existing stream work.

## Next step

After the exact source handoff, route the live pointer-up gesture through the T009 reducer using the latest Rust curve context, then verify pointer-up commits once, cancellation/error sends nothing, and undo/redo preserves the Rust-canonical curve. Keep the current Rust/session/transport path unchanged until that integration can be tested through its actual shared UI adapter.

## Handoff notes

Do not simplify or persist on each pointer move. The desired boundary is one completed gesture → one commit → one simplification → one undo/revision. Frontend filtering is optional transport compression, gated on the correct committed Rust base and parity-tested identical algorithm; it never becomes a competing project model. Rust/session/transport tests pass; do not report T009 complete until the real UI pointer-up path uses that transport and its single-commit/cancel behavior is verified.

## Continuation audit and bounded implementation (2026-10-15T03:53:11Z)

- Confirmed the owned transport module is still fail-closed: `CURVE_STROKE_REDUCER_PARITY_VERIFIED` remains false, so it does not send a reduced stroke before Rust/JS parity is established.
- Fixed the transport adapter's missing first-stroke context path. A `create: true` request now reads the Rust descriptor/empty-curve context through `session_curve_stroke_context_for_abbr_json`; existing curves continue through `session_curve_stroke_context_json`. The adapter validates the create envelope (`curve_id: null` plus a non-empty `abbr`), rejects an already-existing descriptor before retrying, and permits the contract's optional `abbr` on existing-curve commands while still checking it when supplied.
- This changes only the explicitly T009-owned `web/app/curve-stroke-transport.js`. It does not turn on the reducer gate, mutate project state during preview, connect the UI pointer-up handler, or touch Android.
- Current gaps: no shared JS/WASM/native boundary-vector evidence has been recorded; the production React gesture still has no confirmed T009 handoff into the driver; T004 owns the overlapping UI surface. Do not claim high-frequency curve commits complete until parity and a bounded UI ownership/integration decision are resolved.
- Verification: no tests or build were run in this continuation. Source review confirms the Rust/WASM read methods exist and return revision, part/curve identity, descriptor context, and points for existing or not-yet-created curves.

### Next step

The same seven vectors now pass in a real Chromium browser against a Web-target Session, in Node against direct-WASM and native Sessions, and the transport commit/rebase tests pass. The reducer gate is enabled. The pointer-up UI/session-adapter handoff remains outstanding; keep the shared UI paths untouched until it is confirmed.

## Direct-WASM parity verification (2026-10-15T03:58:24Z)

- Added the T009-focused test harness `web/app/curve-stroke-transport.parity.test.mjs`. It compares the committed curve after sending raw points with the committed curve after the JS reducer output, using a `wasm32-unknown-unknown` `engine-wasm` build executed through Node's WebAssembly runtime.
- Seven vectors pass: below and above the 1.8 descriptor tolerance; exactly at and above the capped tolerance 5; equal-distance tie with uneven spacing; negative values with multiple retained intervals; and a replacement against an existing curve with outside controls. The test also uses `readCurveStrokeContextInWasm` for both new and existing curve context reads.
- Commands and results:
  - `cargo build --manifest-path native/Cargo.toml --target wasm32-unknown-unknown -p engine-wasm` — passed.
  - `wasm-bindgen --target nodejs --out-dir /tmp/t009-curve-parity/pkg native/target/wasm32-unknown-unknown/debug/engine_wasm.wasm` — passed.
  - `T009_WASM_BINDING=/tmp/t009-curve-parity/pkg/engine_wasm.js node --test web/app/curve-stroke-transport.parity.test.mjs` — 1 test passed, 0 failed.
  - `cargo test --manifest-path native/Cargo.toml -p commands curve_simplification -- --nocapture` — 2 passed.
  - `cargo test --manifest-path native/Cargo.toml -p project-session --test conformance_vectors curve -- --nocapture` — 1 passed.
  - `cargo test --manifest-path native/Cargo.toml -p engine-wasm --test shared_adapter_vectors curve -- --nocapture` — 2 passed.
  - `node --check web/app/curve-stroke-transport.js` and targeted `git diff --check` — passed.
- Scope limit: the seven reducer vectors were compared against a real WASM Session in Node/V8. The native Rust tests exercised the existing curve simplification and shared adapter vectors, but did not execute those same seven new cases; the React/browser gesture and pointer-up integration were not run. Keep `CURVE_STROKE_REDUCER_PARITY_VERIFIED` false until those gaps are closed.
- No Android, shared UI, Session contract, or Rust runtime source was changed. T009 remains active; production commits still require exact cross-target parity and bounded UI ownership/integration.

## Cross-runtime and commit-path verification (2026-10-15T04:05:01Z)

- Added the T009-focused harness `web/app/curve-stroke-transport.parity.test.mjs` and native Session oracle `native/crates/engine-wasm/examples/curve_stroke_parity_oracle.rs`. The harness compares final committed curve points after raw and JS-reduced strokes against both a `wasm32-unknown-unknown` Session and a native Session.
- Seven vectors pass: below/above the 1.8 descriptor tolerance; exactly at/above the capped tolerance 5; equal-distance tie with uneven spacing; negative values with multiple retained intervals; and replacement against an existing curve with outside controls.
- `cargo build --manifest-path native/Cargo.toml --target wasm32-unknown-unknown -p engine-wasm` — passed.
- `wasm-bindgen --target nodejs --out-dir /tmp/t009-curve-parity/pkg native/target/wasm32-unknown-unknown/debug/engine_wasm.wasm` and `cargo build --manifest-path native/Cargo.toml -p engine-wasm --example curve_stroke_parity_oracle` — passed.
- `T009_WASM_BINDING=/tmp/t009-curve-parity/pkg/engine_wasm.js T009_NATIVE_ORACLE=/media/DATA/project/android-voice-synth/native/target/debug/examples/curve_stroke_parity_oracle node --test web/app/curve-stroke-transport.parity.test.mjs` — 3 passed, 0 failed. This covers cross-target reducer results, new/existing curve context + one-revision commit + undo/redo, and stale revision retry against the latest base with no raw fallback.
- `cargo test --manifest-path native/Cargo.toml -p commands curve_simplification -- --nocapture` — 2 passed; `cargo test --manifest-path native/Cargo.toml -p project-session --test conformance_vectors curve -- --nocapture` — 1 passed; `cargo test --manifest-path native/Cargo.toml -p engine-wasm --test shared_adapter_vectors curve -- --nocapture` — 2 passed.
- Browser check: built `engine-wasm` with `wasm-bindgen --target web`, served a temporary local parity page, and ran it in headless Chromium 154 via Playwright. Result `PASS 7 browser JS/WASM vectors`; console had 0 errors and 0 warnings. Temporary page, server and browser session were closed.
- `node --check` for transport and parity harness plus targeted `git diff --check` — passed.
- Updated `CURVE_STROKE_REDUCER_PARITY_VERIFIED` to `true` after the parity gate passed. The production UI still bypasses T009: `expression-lanes-panel.jsx` uses `curve-stroke-input.js` to canonicalize only, then sends `exp/stroke` through `clone-state.jsx` and `engine.js`. A bounded UI/session-adapter handoff is pending; do not modify those shared paths before it is confirmed. Android remains out of scope.

## Current blocker audit (2026-10-15T05:19:15Z)

- The T009-owned Rust simplifier, transport reducer, and cross-runtime parity/commit tests are complete. The remaining acceptance gap is live React pointer-up integration: `expression-lanes-panel.jsx` currently submits through `clone-state.jsx`/`engine.js` without the reducer.
- This UI path overlaps T004's active React editor ownership; `clone-state.jsx` also has a narrow T033 stream-player handoff. No T009 source-owner transfer for the curve gesture/adapter path is recorded. Keep those files untouched until T004/T033 provide an exact bounded handoff, preserving T033's streaming-only ownership.
- After handoff: route the committed Rust-context read and one reduced `curve.apply_stroke` through pointer-up; verify cancel/error behavior and exactly one Rust revision/undo using both supported host adapters. No product source changed in this audit.

## Handoff cleanup (2026-10-15T05:21:36Z)

- Replaced the stale top-level blocker text that still described CONTRACT-v2 as unfrozen and Rust/WASM reducer parity as outstanding. Those gates have since been completed; the remaining blocker is solely ownership of the live React UI/adapter path.
- The task remains blocked, not implementation-active, until T004/T033 identify the bounded path transfer. No source or tests changed.
