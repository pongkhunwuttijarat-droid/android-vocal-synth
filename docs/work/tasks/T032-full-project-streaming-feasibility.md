# T032 — P2A full-project incremental PCM feasibility and Session proposal

**Status:** done — Wave 1 feasibility/proposal and Wave 2 API/host review complete; T033 remains blocked  
**Owner:** DSH coordinator (subagent support)  
**Last updated (UTC):** 2026-10-08T13:38:31Z

## Scope and deliverables

Source-audit full-project incremental PCM feasibility and prepare a reviewable, separate Session/API proposal if needed. Define Rust-owned chunk rendering, global timeline ordering/lookahead, transport and bounded buffering, AudioWorklet delivery, underrun behavior, stale revisions/edit invalidation, media and stateful FX continuity, and how playback begins before the full render completes. P0C completed-take jobs are not streaming.

## Existing task relationship

- Reuse [T011](T011-render-pool-product-wiring.md)'s queue, committed snapshot, chunk and mixer evidence only within tested scope; its production RenderPool wiring remains active and is not the full-project Session mixdown path.
- Use done [T017](T017-audio-track-waveform-end-to-end.md), [T020](T020-arrangement-worker-isolation.md), [T015](T015-media-mixdown-and-real-project-render.md), and active [T018](T018-openutau-flags-and-track-fx.md) evidence without reopening or duplicating them.
- Follow the P2A gates in the [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md).

## Dependencies and blockers

- Wave 1 source feasibility and the explicitly unapproved proposal are complete; implementation remains blocked. Frozen [CONTRACT-v1 §6](../../CONTRACT-v1.md) is unchanged.
- Any public Session/API or dependency change needs separate review/approval before implementation. T011 file ownership must be coordinated before touching pool/worker source.

## Likely files / ownership

A new proposal under `docs/planning/`, plus source-backed references in this task record. Feasibility work should not modify `native/crates/project-session/`, `web/pool.js`, `web/worker.js`, or playback source. Those remain owned by existing T011/T004/T019/T018 efforts unless ownership is explicitly coordinated.

## Acceptance and evidence

- [x] Document current Direct WASM versus HTTP capability differences and distinguish harness evidence from product streaming.
- [x] Provide a full-project feasibility matrix for voice and wave parts, FX, chunk boundaries, revisions, seek, buffering, cancellation, memory and device support.
- [x] Propose exact Rust-owned API/contract changes only if existing approved APIs cannot serve the requirements; label the proposal unapproved and leave CONTRACT-v1 unchanged.
- [x] Define measurable acceptance for first-audio latency, underruns, continuity, PCM reference comparison, memory and browser-clock evidence. No tolerance or threshold may be invented without a measured baseline and approval.
- [x] Recommend proceed/defer/fallback, with named dependencies and risks.

## Concurrency and approvals

Wave 1 source-only feasibility is complete. T033 remains blocked pending separate API/host review, required Session/API approval, and measured baselines.

## Wave 1 feasibility checkpoint (2026-10-08)

- Source feasibility and a separately labeled unapproved Session/API proposal are assigned to subagent `b339c051-e616-418e-9dab-4212c8775aa3`, restricted to new report `docs/planning/t032-full-project-streaming-feasibility-wave1.md`. No Rust Session, worker, playback source, or frozen contract edits are authorized.
- Wave 1 source audit/proposal completed and verified in `docs/planning/t032-full-project-streaming-feasibility-wave1.md` (92 lines): Direct-WASM/HTTP capability gap, full-project matrix, explicitly NOT APPROVED Rust API, threshold-free measurement plan, and defer-T033 recommendation. No code, contract, build, test, dependency, experiment, or Git changes.
- T033 remains blocked pending separate API/host review, approval, and measured baselines; CONTRACT-v1 remains unchanged.

## Wave 2 API/host boundary review (2026-10-08)

- Review is complete and read-only. Direct WASM currently exposes synchronous full-vector mixdown over live Session/assets; HTTP jobs are asynchronous to poll/cancel but retain the completed full PCM vector and do not expose progressive blocks. T011 chunk/order/cancel-between-chunks is not full-project streaming.
- Candidate API remains explicitly unapproved and is not a CONTRACT-v1 edit: pub fn session_render_stream_begin(&mut self, expected_revision: u64, options: SessionRenderStreamOptions) -> Result<SessionRenderStreamInfo, String>; pub fn session_render_stream_next(&mut self, stream_id: &str, max_frames: u32) -> Result<SessionRenderBlock, String>; pub fn session_render_stream_cancel(&mut self, stream_id: &str) -> Result<(), String>; pub fn session_render_stream_close(&mut self, stream_id: &str) -> Result<(), String>. Blocks need session/revision/generation identity, format/rate/channels, monotonic sequence and frame range, bounded PCM, and explicit data/terminal/cancelled/stale/error status.
- Full-project semantics still need immutable input/asset identity, global voice/wave ordering, FX-tail continuity, bounded backpressure/underruns, stale invalidation, and measured memory/cancellation/device latency. No code/tests/builds or contract/API changes were made. T033 remains blocked until separate Session/API and host approval plus measured baselines.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`; dirty/untracked ownership is unknown.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Next: conduct the separate API/host review and establish measured baselines only when authorized; do not begin T033 until its Session/API and host gates are approved.