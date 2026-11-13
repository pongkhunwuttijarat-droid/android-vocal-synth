# T011 — Wire RenderPool into the production Web flow

**Status:** active
**Owner:** Codex
**Last updated (UTC):** 2026-10-02T13:45:00Z

## Goal

Build the Session-backed browser RenderPool adapter independently of T004, then wire it into the selected React/Dockview/PixiJS 8 production UI after the mock is reviewed. Rendering consumes committed Rust Session state and returns audio/progress/cancellation results without making the frontend authoritative for project or render-planning state. The current HTML editor's legacy YAML path is retained during migration.

## Completion criteria

- [x] Define the browser render adapter boundary from frozen CONTRACT-v1 §6.13, so the scheduler and Session snapshot capture can be implemented independently of React mock review.
- [x] Add committed snapshot capture, revision-tagged RenderPool jobs, per-chunk progress, FIFO worker scheduling, and per-job cancellation at worker chunk boundaries.
- [x] Inspect current production Web Driver, RenderPool, worker protocol, cache and browser audio path; record what is already wired and what remains.
- [x] Connect the adapter to committed engine/WASM Session snapshots and worker plans; do not render from local edited models or gesture previews.
- [x] Preserve worker reuse, incremental sample loading/cache, revision-tagged progress, per-job cancellation, and stale-result publication checks.
- [x] Verify output parity, edit → invalidate → rerender, active cancellation, and worker replacement after a simulated failure.
- [ ] Wire the stable-ID production Driver and selected React UI through the adapter; verify playback and export integration.
- [x] Document browser requirements and exact adapter results; update the WASM migration and MS3.5.4 status.

## Source planning

- [WASM migration status](../../planning/wasm-migration-status.md)
- [MS3](../../planning/milestones/ms3/MS.md) §3.5.4
- [Web UI design](../../planning/web-ui-design.md) §Worker pool + per-chunk sample loading
- [T004 Web editor product flow](T004-web-editor-product-flow.md)
- [T010 Project Session contract conformance](T010-contract-adapter-conformance.md)

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`
- Relevant working-tree changes: repository contains extensive pre-existing/concurrent edits with unknown ownership; inspect before claiming or changing implementation files.
- Durable references: `web/app/`, `docs/planning/wasm-migration-status.md`, `docs/planning/milestones/ms3/MS.md`.

## Progress

- 2026-10-02T10:05:00Z (Buffy): **Streaming playback is now possible — and it pays more than the
  native benchmark suggested.**
  `runtime::mixer::mix_from(inputs, start_index)` returns the audio from a chunk boundary onward, and
  `Mixer::finish_from` exposes it to WASM. A host that has rendered chunks `0..=i` can play that
  instead of waiting for the rest, then append the next range; the result is **bit-identical** to one
  full mix, so the seams are inaudible.

  *The subtlety that made the first attempt wrong:* `mix` is a sequential crossfade-overwrite, so a
  chunk does not crossfade against nothing — it blends into the tail of the previous chunk over its
  leading zone. Starting at `start_index` directly cuts where the fade should be, and starting at
  `start_index - 1` is not always enough either: a chunk is long relative to its leading, so chunk
  `start_index - 2` can still overlap the first milliseconds of `start_index`. `mix_from` therefore
  walks back until no earlier chunk can still contribute. `mix_from_is_the_tail_of_a_full_mix` caught
  this (`mix_from(2)` failed bit-comparison at sample 0 on a 45 ms / 20 ms-leading plan) — it asserts
  `to_bits()` equality, not closeness, on chunks that genuinely overlap.

  In `web/pool.js`: `mixFrom(i)` (returns `null` when the cache has not reached `i`, so a caller can
  tell "not ready" from "ready and empty") and `readyThrough()`. The latter had an off-by-one I wrote
  and then fixed: chunk 0 missing is **not** "chunk 0 ready", it must report `-1`, otherwise a player
  believes it can start playing silence from the top.

  Measured in the browser harness on a cold cache (4 chunks, 14.5 s of audio):

  | | ms |
  |---|---|
  | first playable audio | **1361–1376** |
  | full plan ready | 3431–3456 |
  | **saved** | **~2080** |

  Two runs, stable. This is larger than the native CLI benchmark predicted (1.255 s cold for the
  whole plan) because the WASM pool under headless Chromium is slower than the native binary, which
  makes waiting for the whole plan *more* expensive — streaming helps proportionally more there.

  Verification: `bash web/run-session-render.sh` PASS — `render parity: 639685 frames, sampled max
  difference 0`, `4 mixFrom() ranges match the full-mix tail`, streaming-latency line above,
  `5 chunk(s) kept in cache`, `post-cancel rerender reused 4 cached chunk(s)`, worker recovery and
  the stale-revision publication guard intact. `bash web/run-editor.sh` PASS.
  `cargo test -p runtime -p project-session -p synth-cli` — 180 passed / 0 failed.
  `cargo test -p synth-cli --test plan_convention --test render_real -- --ignored` — 9 passed,
  including `planned_chunks_mix_bit_identically_to_the_sequential_render` and the four
  `rust_*_matches_so_*` comparisons against `libworldline.so`, so the mixer change is golden-clean.
  The latency measurement polls the pool rather than listening for a progress event, because a
  progress event can fire before the cache write lands.

  **Still not wired into playback.** `pool.mixFrom`/`readyThrough` exist and are proven; the Web
  Audio transport in `web/app/editor.js` still awaits a full render before the first sample, and the
  React clone page has no transport at all. That is the next step, and it is where the remaining
  risk lives: a player must schedule ranges on the audio clock and handle the case where the next
  range is not ready when the current one ends.

- 2026-10-02T09:10:00Z (Buffy): **Streaming step 1 — drain chunks into the cache on delivery.**
  The user asked for OpenUtau-style streaming. Measured the budget first, because it decides whether
  streaming pays: `scale-demo-app` is 14.5s of audio in 4 chunks, and `synth-cli render` takes
  **1.255s cold / 0.019s warm** (release binary, `libworldline.so`). So streaming buys roughly 1s
  of Play latency in the cold case and nothing in the warm case — it does not improve throughput.
  (Measurement note: the cache lives at `<voicebank>/../cache`, i.e. `teto-english/cache`; clearing
  `test/golden/cache` measures nothing. The cache was moved aside and restored, 4 files intact.)
  **What was found:** the streaming plumbing already exists. `worker.js` posts one `type:"chunk"`
  message per chunk with a `yieldToMessages()` between them, and `pool.js` already calls
  `p.onChunk?.(chunk)` for each arrival — but `pool.render()` never passed that callback, so every
  chunk was buffered in `out[]` and only written to the cache after a worker's whole batch settled.
  `render()` now writes each chunk to `this.cache` the moment its worker delivers it. Safe because
  the cache is keyed by the engine hash and `_cancelled()` already returns `samples: null`, so a
  cancelled job still publishes no audio; it simply stops throwing away finished work.
  A/B, same harness, cancel fired after chunks land:

  | | chunks kept after cancel | reused by the rerender |
  |---|---|---|
  | before (buffer until complete) | 4 | 3 |
  | after (drain on delivery) | **5** | **4** |

  Verification: `bash web/run-session-render.sh` PASS — `render parity: 639685 frames, sampled max
  difference 0`, `5 chunk(s) kept in cache`, `post-cancel rerender reused 4 cached chunk(s)`, worker
  recovery and the stale-revision publication guard intact. `bash web/run-editor.sh` PASS. Two new
  assertions added to `web/session-render-test.html`: a cancelled job must publish no audio, and the
  post-cancel rerender must not re-render everything (it fails loudly if cancelled audio was
  discarded).
  **Honest limit:** this is one chunk of work saved per cancel, and it does not make playback start
  sooner. Full streaming playback (scheduling rendered chunks on the audio clock, as
  `MasterAdapter` does with its stalled clock) is still unimplemented, and on this budget it would
  save ~1s cold and 0s warm. The mixer constraint found while scoping it: `runtime::mixer::mix` is
  a sequential crossfade-overwrite — chunk N blends against chunk N-1's tail with cos²/sin² over
  the leading zone — so chunks can only be played from an already-computed boundary forward, not
  jumped into at an arbitrary point.

- 2026-10-01T05:34:45Z: Added `tests/vectors/project_part_edge_shared.json` with 13 representative project/part no-op and rejection cases, asserting canonical error code, unchanged committed revision/document, and unchanged undo/redo flags. The same shared fixture passes the reference `project-session`, direct WASM `Engine`, and native HTTP route tests. Updated CONTRACT-v1 §6.18 to describe this coverage accurately (representative cases, not exhaustive malformed-input enumeration). T011's remaining non-wiring checks for these project/part commands are closed; selected React production wiring remains sequenced after T004 mock review.
- 2026-10-01T05:21:46Z: Resumed T011 at user request. Rechecked the current branch/HEAD and dirty tree; no implementation ownership was inferred from the pre-existing edits. Verified the current representative project/part vectors in all three layers (`project-session`, direct WASM, native HTTP): `cargo test -p project-session --test session_document_commands`, `cargo test -p engine-wasm --test shared_adapter_vectors`, plus native `shared_document_vector_matches_native_http_routes` and `shared_part_vector_matches_native_http_routes` all pass. These are representative positive vectors only; the required exhaustive project/part invalid/no-op parity set is still open. Production Driver/React wiring remains gated by T004 mock review and the agreed contract-check sequence.

- 2026-09-30T17:05:21Z: Implemented the Session render boundary in `web/app/session-render-contract.js`, `web/pool.js`, and `web/worker.js`: expected-revision snapshot capture; committed snapshot handoff; revision-tagged job state/events; per-chunk streamed progress; FIFO jobs over shared worker engines; per-job cancel at chunk boundaries; and worker-call rejection on failure/close. Added a browser regression page/runner that checks sampled PCM parity with the sequential WASM renderer, active cancellation, one-chunk invalidation/rerender, and stale-result guard. `node --check` passes for the modified JS. First legacy regression run exposed `null` vs `undefined` worker job IDs; normalized that identity and reran successfully. `bash web/run-editor.sh` passes (639343 frames, cache reuse, one-chunk invalidation, cancellation, OPFS lifecycle). `bash web/run-session-render.sh` passes (639685 frames, sampled max PCM diff 0, active per-job cancellation, one changed chunk rerender). Failure/recovery and React production UI wiring remain unverified.
- 2026-09-30T17:12:47Z: Final syntax check passed for `web/pool.js`, `web/worker.js`, and `web/app/session-render-contract.js`; the Session render harness passed again after listener/termination handling was added. The adapter sends immutable job metadata with each worker plan and validates render/chunk identity. Production React wiring and automatic worker recovery remain open.
- 2026-09-30T17:17:05Z: Added automated recovery of failed worker links before the next Session job, plus a simulated worker-crash recovery case in the browser harness. Final `node --check` passed and `bash web/run-session-render.sh` passed: 639685 frames, sampled max PCM difference 0, per-chunk tagged progress, active cancellation, one changed chunk rerender, stale-result guard, and worker replacement. Updated browser requirements and WASM/MS3.5.4 status. The adapter is prepared independently of T004; production React Driver and export/playback wiring remain downstream.

- 2026-10-01T02:52:48Z: Added the reusable WASM `SessionDriver` and paired OPFS identity-bundle storage required by the next production integration step. Session commands and snapshots are now available to a Web host without legacy index-based APIs; stored reopen preserves sidecar IDs, portable import omits them, and Save As persists the prepared pair before identity adoption. WASM release build/bindings and staged Save As identity check pass. React RenderPool/playback/export wiring remains outstanding.

- 2026-10-01T04:52:40Z: User clarified this is the active T011 task and directed that remaining non-wiring gaps be completed before production Web wiring. Supporting Rust work was mistakenly tracked as standalone T012. That prerequisite now implements §6.4.4 project/part commands (including wave move), schema-v2 project/part views, bounded reads, native HTTP routes, and direct WASM methods. Reference/native/WASM tests cover document metadata, grouped part metadata, split curve boundaries and undo. See [T012 supporting handoff](T012-rust-part-project-commands.md); its work is part of T011's readiness, not a replacement for T011.

- 2026-09-30T16:54:46Z: Resumed at user request and rechecked the current implementation without modifying production code. Stable-ID session APIs are present in `engine-wasm`, including `session_snapshot_json` and v2 command methods, but `web/app/driver.js` still uses the compatibility facade (`project_load_yaml`, `apply_command`, legacy notes/tracks views). `RenderPool.create/setProject` accepts YAML and plans through legacy `Engine.plan_project`; workers also initialize/replan from YAML. The session capability contract still reports async render jobs unsupported, and there is no session render-plan API. T004 remains active and explicitly reserves UI/session/lifecycle/RenderPool for one consolidated integration pass after mock review. Marked T011 blocked on that dependency; no implementation files were changed.

- 2026-09-30T16:32:16Z: Handoff resumed together with T004. T004 remains at mock review: Dockview initial panel composition was corrected, but the Piano Roll Pixi canvas renders black in browser preview. No T011 production wiring is started; preserve the agreed sequence and wait for mock signoff plus the stable-ID Session render snapshot contract. Current inspection shows `RenderPool.create/setProject` and workers initialize/plan from a YAML string, while Rust `Engine` exposes session open/apply/snapshot/sidecar and independent legacy YAML planning/render APIs. The Rust v2 parser still explicitly leaves `part.*`/`project.*` unimplemented, so a full Session-backed UI/render flow cannot be claimed yet.

- 2026-09-30T15:03:25Z: Created at user direction to track RenderPool product wiring separately from Project Session contract decisions and lifecycle persistence.
- 2026-09-30T15:57:00Z: Source inspection found the existing HTML editor already creates/reuses RenderPool workers, incrementally loads samples, tracks chunk-cache progress, cancels by epoch and preserves audio on stale results (`web/app/editor.js`, `web/pool.js`). That path plans from legacy YAML, so it does not satisfy the committed-Session boundary. User directed finishing the React/Dockview/PixiJS 8 mock first, then integrating WASM Session/lifecycle and RenderPool together once. T011 remains queued behind T004. The earlier note about a sandbox socket-bind failure was superseded by the later passing regression run.
- 2026-09-30T16:09:10Z: `bash web/run-editor.sh` now passes end to end after replacing WASM `SystemTime::now()` with browser-safe session seed generation. The legacy YAML RenderPool baseline is verified (4 workers, 6 chunks, cache reuse, invalidation, cancellation); this does not fulfill the queued Session-backed React wiring.
- 2026-09-30T16:26:06Z: T004 is still at mock-review stage. The in-app browser showed legacy `web/editor-test.html`, which is a different page from the React Dockview mock. Its URL policy rejected a `file://` attempt to open `ui-mock/react-dock/dist/index.html` and prohibited local-server/alternate-surface access to that React mock afterward. User explicitly authorized local server + Chrome preview; no pool integration started.

### 2026-10-02T13:10:00Z (Buffy) — volume/pan moved into the MIX stage; the browser was losing it

**A real bug, found while wiring the clone's waveform.** `Chunk::hash`
(`chunking.rs`) deliberately excludes volume, pan and FX — that exclusion is the
whole point of the chunk cache, and it is why a fader move should re-mix cached
PCM instead of re-rendering it. But the WASM `Mixer` wrapper pushed
`TrackSpec::default()` for every chunk, so the browser mixdown silently dropped
the entire mixer that `runtime::mixer::mix` has always applied per input. The
fader looked alive in the UI and did nothing to the audio.

- `engine-wasm`: new `Mixer::add_chunk_track(samples, position_ms, leading_ms, volume_db, pan)`.
  `add_chunk` delegates to it with `0.0, 0.0`, so nothing already using it changes.
  The doc comments say *why* the two values belong here (mix stage, therefore
  absent from the hash) and that the stateful post-mix FX chain is deliberately
  still not applied — a range mixed from mid-song cannot reproduce a full
  render's biquad/compressor state.
- `web/pool.js`: `RenderPool.create({trackMix})` plus `setTrackMix({volumeDb, pan})`;
  `mix()`, `mixFrom()` and `toWav()` all go through `_addChunk`, which calls
  `add_chunk_track`. `setTrackMix` touches neither the plan nor the cache, so the
  next mix re-uses the same PCM.
- Regression guard: `native/crates/engine-wasm/tests/mixer_track.rs` — 0 dB/centre
  stays unity, −6.0206 dB is 0.5 linear, hard pan is cos(π/4), and
  `finish_from(i)` is **bit-identical** to the tail of `finish()` with
  non-default volume/pan on every chunk.
- `web/pkg` rebuilt (simd128 + wasm-bindgen); `add_chunk_track` is in the
  generated `.d.ts` and `.js`.

**Measured in the browser, not just asserted.** drag-probe moved the arrangement
fader to −24 dB and measured the strip: tallest peak column 45px → 4px, with the
pool reporting `0 chunks rendered / 1 reused`. The fader is heard, and nothing
was re-rendered. FX stays deferred by user decision.

### 2026-10-02T13:45:00Z (Buffy) — the clone's transport plays the cache

`mixFrom`/`readyThrough` had no caller on the product UI: ▶ moved a clock and
nothing else. It plays audio now.

- `ui-mock/react-dock/src/openutau/audio-playback.js` (new): a Web Audio player
  that asks the pool for the range at the playhead — `chunkIndexForMs` picks the
  last chunk starting at or before it, `pool.mixFrom(index)` returns the tail
  from there, and playback starts `wantMs - originMs` into it. The pool is shared
  with the waveform preview through `getRenderPool()`.
- **The playhead is read back from the `AudioContext` clock, not from a timer.**
  That is the whole point: the sample you hear and the tick under the cursor are
  the same position. The silent delta clock remains only for the case where
  there is no rendered take to play, and the status line says so.
- Seeks are handled where the intent is (`dispatch`), using `transportTarget()` —
  the same helper the reducer uses, so the cursor and the audio can never
  disagree about where "next measure" landed.
- A project can be longer than its audio, so a play/seek past the end clamps to
  the end of the take and `onEnded` stops the transport.
- `AudioContext` creation/resume happens inside the transport click (a browser
  refuses it from a later effect), and a context whose clock never runs is
  detected after 600 ms and falls back to the silent clock rather than freezing
  the playhead.

**Two real bugs found on the way.**

1. **A render loop** (`Maximum update depth exceeded`) that only appeared once the
   playhead started updating every frame. `clone-state.jsx` built
   `const vibratoPatches = ui.vibratoPatches || {}` — a fresh `{}` on every
   render, which gave `activeNotes` a new identity on every render, which
   re-ran `useFollowPart`, which called `setScroll` with a fresh object, which
   re-rendered. Fixed at both ends: no throwaway object, and `useFollowPart`
   returns `current` unchanged when the clamp is a no-op.
2. **Chasing a fake bug.** The probe reported the playhead advancing 3.1 s per
   1.2 s of wall time. An in-page measurement of `AudioContext.currentTime`
   against `performance.now()` says 0.998–1.000×: the clock is real time and the
   rest was CDP round-trip latency on a software-rasterised page (a transport
   click costs a few hundred ms there). The probe now measures inside the page
   and asserts jumps rather than absolute positions.

## Verification

- `node --check web/pool.js && node --check web/worker.js && node --check web/app/session-render-contract.js`: passed.
- `bash web/run-editor.sh`: passed after the legacy worker identity compatibility fix. Confirms existing editor render/playback, cache reuse, invalidation, cancellation, and OPFS lifecycle remain working.
- `bash web/run-session-render.sh`: passed. Confirms committed Session snapshot render, revision-tagged chunk progress, sequential WASM PCM parity (sampled max difference 0), active per-job cancellation, current/stale publication guard, one-chunk invalidation/rerender, and worker replacement after a simulated crash.
- `cargo test -p runtime -p engine-wasm -p synth-cli`: all green (new `mixer_track.rs` 2 passed; `memory_render` 3 passed against the real Teto bank; the `--ignored` suites still need `libworldline.so`).
- `bash web/run-session-render.sh`: **PASS** after the `add_chunk_track` change — render parity 639685 frames with sampled max difference 0, 4 `mixFrom()` ranges match the full-mix tail, cancel keeps 5 chunks, post-cancel rerender reuses 4, edit/invalidate re-renders 0 changed chunks. This is the harness that proves `pool.mix()` still equals the sequential WASM mix after the volume/pan wiring.
- Browser (clone page, both backends): a fader move re-mixes with `0 chunks rendered / 1 reused` and the drawn peaks scale with the fader.
- drag-probe (57 checks, green on `?backend=http` and `?backend=wasm`): the audio clock tracks real time (1.000×), play starts real audio from the cache, the playhead advances in step with the audio at the engine tempo (975 ticks in 1015 ms, expected 975), restart rewinds the audio, skip-to-note moves the audio, pause stops it and keeps the position.
- The synth-server fallback path is verified only up to the request envelope: `POST /api/v2/sessions/{id}/render` with `{expected_revision, voicebank, track}` is accepted (a stale revision answers 409 with the frozen error shape), but the dev server on :8765 runs without `--so`, so it answers 500 `renderer not loaded` and the strip says so instead of drawing anything.
- Production React UI and export/playback integration remain unverified. The browser engine capability descriptor remains unchanged and still reports async jobs/per-job cancellation as unsupported.

## Blockers and dependencies

- T004 mock review is not a dependency for the Session snapshot and RenderPool adapter implementation. It remains a dependency only for wiring the finished adapter into the selected React UI.
- The frozen logical revision/cancellation/stale-result semantics are in T010/CONTRACT-v1.md §6; worker transport remains host-specific.

## Next step

The representative project/part invalid/no-op parity vectors now pass through reference, native HTTP, and direct WASM. Next: complete T004 mock review, then connect the stable-ID production Driver and selected React UI to the adapter and verify playback/export integration. Keep async render capability claims host-specific: native server jobs are available; direct WASM jobs remain unavailable.

## Handoff notes

RenderPool is a rendering execution adapter. It does not own project identity, musical state, serialization, validation, edit history, or render planning. Rust remains authoritative; browser state may track transient playback and UI status only.
