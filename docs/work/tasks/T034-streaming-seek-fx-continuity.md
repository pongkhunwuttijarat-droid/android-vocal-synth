# T034 — P2B streaming seek and stateful FX continuity

**Status:** blocked — source-bounded legacy FX, versioned seek snapshots, past-source-end behavior, and cache policy are confirmed; implementation awaits T033 base-streaming gates and bounded T034 handoff for shared paths
**Owner:** unassigned — proposed Streaming/audio lane
**Last updated (UTC):** 2026-10-15T08:30:39Z

## Scope and deliverables

Add seeking and stateful effect continuity to the approved incremental playback path: define state snapshots/reset/seek rules, cross-chunk FX history, replay behavior, and correctness around discontinuities. This is not a second FX implementation.

## Existing task relationship

- Depends on [T033](T033-session-streaming-product-integration.md) and active [T018](T018-openutau-flags-and-track-fx.md) FX semantics/evidence.
- Use [T011](T011-render-pool-product-wiring.md) chunk evidence only within its tested mixer scope; done [T020](T020-arrangement-worker-isolation.md) is completed-take playback evidence, not streaming seek proof.

## Dependencies and blockers

- T033 is active against the user-approved MK3 logical 3.0 baseline, but its assigned stream pipeline/server files are not automatically T034-owned. Wait for base-streaming host gates and obtain a bounded T034 path handoff before editing shared stream/session integration.
- The user-approved behavior and a candidate internal checkpoint contract are documented in MK3 §6.1. The proposal adds no public snapshot DTO/bytes or seek-only response fields; its reuse of `range.start_frame`, `unsupported_range` gating, and cache limits still require review/ratification before implementation or capability advertisement. Do not change frozen CONTRACT-v1 or claim MK3 routes/capabilities are live.

## Product decisions confirmed (2026-10-15)

- Legacy USTX track FX and its streaming/offline output keep the existing source-bounded end. Do not drain a reverb tail past the source end. A future SoundCraft-derived insert chain may declare a separate bounded tail policy.
- Seek should use versioned FX-state snapshots for faster reconstruction rather than replaying every time from frame zero.
- Android wrapper work remains deferred; this task is native/Web only until the user resumes Android.

## Proposed snapshot invariants for review

- Snapshot is opaque to the UI and versioned independently from CONTRACT-v1; it contains the FX processor state only, not project authority.
- Bind each checkpoint to the immutable render identity: session/project revision, render configuration/build identity, sample rate/channel layout, FX chain configuration digest, and exact frame boundary. Reject restore on any mismatch; never apply state from another render generation.
- Save only at completed block boundaries after all frames through `frame_exclusive` have been processed. A seek starts from the latest compatible checkpoint at or before the target, restores it, processes/discards to the target, and starts output at that exact frame. If no compatible checkpoint exists, start at frame zero and discard.
- Bound checkpoint count/bytes and evict old checkpoints deterministically. Cancel/restart invalidates in-flight work by generation; checkpoints for an old revision/config remain unusable.
- Keep legacy USTX source-end truncation identical in offline and streamed paths. New mixer inserts require their own explicit tail contract and are out of this T034 change.

These are proposed implementation invariants, not a claim that a public snapshot schema is already accepted. The exact DTO/error surface needs review before implementation. The more detailed candidate behavior below is a design artifact for that review.

## Likely files / ownership

Potential T034 paths are `native/crates/mixer-fx/src/openutau.rs` (T018-owned state snapshot primitives), `native/tools/synth-cli/src/pipeline/streaming.rs` and `native/tools/synth-server/src/stream_ws.rs` (T033-owned stream integration), plus the T033 Web stream adapter/worklet and focused tests. The source owners must explicitly hand off any overlapping paths; no Android files are in scope.

## Candidate `stream.seek.v1` behavior (proposal, not live schema)

`session_render_stream_begin` keeps the half-open requested output range. For `start_frame > 0`, the engine checks its bounded, session-local checkpoint cache for the nearest compatible checkpoint with `frame_exclusive <= start_frame`. It restores that state, processes/discards through `start_frame`, then emits the first data block with `frame_start == start_frame`. If no compatible checkpoint exists, it constructs state at frame zero and replays/discards. The old playback generation is cancelled and its queued PCM flushed before the new generation is accepted; the UI cannot append blocks from the prior generation.

A checkpoint is an internal Rust cache entry, never project data and never supplied by the UI. It remains an in-memory typed value; this candidate does not define a JSON snapshot or persist DSP state in `.rustx`. Candidate fields:

```text
checkpoint_schema_version
render_identity = { session_id, revision, plan_digest, renderer_build_id,
                    sample_rate, layout, fx_chain_digest }
frame_exclusive                 # emitted master timeline frontier
track_fx_states[] = { track_id, processor_id, processed_through_frame,
                      processor_state_version, opaque_rust_snapshot }
```

`plan_digest` includes all source/render inputs that affect the track bus, including immutable voicebank/sample and project-media byte identities; `fx_chain_digest` includes the complete ordered legacy USTX FX configuration and enabled/bypass state. `opaque_rust_snapshot` is an in-memory `OpenUtauTrackFxSnapshot`, not serialized state bytes. Its processor-state version changes whenever its representation or restore semantics change. Charge its actual cloned DSP buffers against the cache byte limit. Restore each candidate into temporary per-track processors; validate the outer render identity, per-track processor identity/configuration/version, and each track's `processed_through_frame` before publishing any restored state. Any mismatch or invalid checkpoint is discarded and falls back to the next earlier compatible checkpoint, or frame zero; never partially restore the active stream.

Capture only at completed master block boundaries. The checkpoint's master `frame_exclusive` is the emitted timeline frontier; each track separately records the last frame its FX processor actually consumed, because a track may end before the master stream. Keep checkpoints session-local and in memory; closing the owning session clears them. Enforce local count/byte limits and TTL, evict deterministically, and keep restore/replay diagnostics internal unless a reviewed wire DTO needs them. This does not promise that a first cold forward seek is fast: FX snapshots only avoid rebuilding prior FX history. The source pipeline must still provide or regenerate the voice/wave bus for the requested interval; measure that source preparation cost separately. No checkpoint crosses a project revision, output format, render plan/build, immutable asset generation, FX configuration, per-track cursor or processor-state version.

This proposal reuses the existing `range.start_frame` rather than adding a second seek command or exposing checkpoint IDs. The additive response fields and errors, capability negotiation, cache limits and exact schema version still require review before implementation. The default legacy FX output end remains the project/source end; seeking does not extend it to drain reverb.

## Acceptance and evidence

- [ ] Seek forward/backward/restart yields deterministic output with no stale chunks or incorrect FX state.
- [ ] Stateful FX remain continuous across sequential chunks and reset/reconstruct correctly at a seek boundary.
- [ ] Compare streamed windows with the approved sequential render on voice + wave projects; record exact/approved numerical parity.
- [ ] Test cold/warm cache, boundary seeks, rapid repeated seeks, cancellation, and bounded memory.

## Concurrency and approvals

Runs after T033; FX-specific preparation may be source-research only after separate execution authorization. No concurrent changes to T018-owned FX code without owner coordination.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e6af244abbf04d4a490fca8483295ed6418e6cad`; relevant streaming/FX source files are dirty or untracked and task ownership must be respected. T033 owns its stream route/pipeline and assigned Web audio paths; T018 owns `OpenUtauTrackFx`; T034 has no source ownership for shared Session/API or T011 RenderPool paths.
- Plan references: [Worker-first UI plan](../../planning/openutau-ui-parity-plan.md), [Taskboard](../BOARD.md).
- Next: resolve T033's non-Android Web credential/trusted-origin host gate, then obtain a bounded T034 handoff for `stream_ws.rs`/streaming pipeline/Web seek paths and coordinate the T018-owned FX snapshot primitives. The past-source-end and 32 MiB/session cache policies are confirmed. Until base streaming and seek implementation pass their gates, keep nonzero ranges rejected and `render.stream` unadvertised.

## Read-only continuation audit (2026-10-14T15:09:08Z)

- Current native Session stream validation in `native/tools/synth-server/src/stream_ws.rs` rejects any range except `[0, end)` (`start_frame != 0` or bounded `end_frame` yields `unsupported_range`). There is no FX checkpoint/state-restore operation. A new connection starts a new render from frame zero.
- `native/tools/synth-cli/src/pipeline/streaming.rs` keeps one `OpenUtauTrackFx` instance per enabled track and calls it on successive final-track-bus blocks in `flush_until`; the stateful Rust processor now has a block-continuity parity test in T018. The UI stream adapter remains intentionally limited to starting at frame zero; it now reports nonzero-playhead streaming seek as unavailable pending this task.
- Neither `OpenUtauTrackFx` nor the Rust stream report exposes an effect-tail length/drain operation. The streaming mixer ends at `TrackStream::final_input_end()` after the last voice/wave source, then emits no extra tail frames. Existing offline mixdown also processes the finite source bus without an explicit reverb tail flush. Therefore streamed/offline source-range equality does not yet prove retained effect tails.

No files in `native/tools/synth-cli/src/pipeline/streaming.rs`, `native/tools/synth-server/src/stream_ws.rs`, or T018's FX implementation were changed in this audit; only the T033 player was constrained to avoid implementing seek ahead of T034. T034 remains blocked pending T033 host activation and an API/product decision for tail duration and seek reconstruction. A viable first design to review is deterministic replay from zero with bounded discard for seek, plus a declared finite tail policy that is applied identically to offline and stream renders; processor-state snapshots would need explicit versioning and invalidation rules. No public API/schema was changed or presumed approved.

## Continuation update (2026-10-15T01:09:17Z)

- T033's assigned Web player now routes nonzero playhead requests to a completed mixdown/RenderPool when available; it only attempts the current native stream from frame zero. This preserves existing seek behavior but does not provide incremental-stream seek when no completed take exists, so the `[0,end)` server limitation and seek reconstruction decision remain.
- T018 now has tested 4,096-frame block-state continuity and sample-rate-scaled Freeverb delay lines; no state snapshot/restore or explicit tail-drain API exists. The current offline pipeline still has no same-policy tail flush, so T034 still needs a cross-path finite-tail rule and owned offline/stream integration paths.
- Android wrapper activation is deferred by the user; T033 host activation remains unavailable for that target.

## Read-only continuation audit (2026-10-15T01:14:11Z)

- Revalidated T033's current state: `stream_ws::tests` pass 12/12, while server range validation still accepts only `[0, end)` and hello advertises no stream feature. This is native protocol evidence, not browser/device playback acceptance. Android remains deferred.
- OpenUtau's `MixFxSource.Mix` clears a full requested scratch block, mixes the source into it, applies EQ/compression/reverb across the requested count, then returns the source's position (`SignalChain/MixFxSource.cs:38-60`). `MasterAdapter` and `ExportAdapter` advance and report only `max(0, source_position - position)` (`MasterAdapter.cs:18-32`, `ExportAdapter.cs:17-28`); neither adds a post-source drain. Thus the reference render length is source-bounded even though stateful FX process the final requested block. This is static source evidence; no C# runtime comparison was performed.
- Rust `flush_until` calls the per-track FX processor on each contiguous source segment and stops at `TrackStream::final_input_end()` (`native/tools/synth-cli/src/pipeline/streaming.rs:574-579, 637-645`). It likewise emits no extra post-source frames. For legacy USTX FX, truncating stream and offline output at the existing source end is the parity-preserving default. A future SoundCraft-derived insert chain can use its own declared bounded tail contract; do not fold that decision into legacy USTX FX implicitly.
- No seek or snapshot implementation/API was changed. Starting from frame zero and discarding to a seek target is deterministic but may make distant seeks slow; versioned FX-state snapshots can reduce latency but need an approved identity/invalidation model. T034 now needs a product choice on replay versus snapshots and confirmation of the legacy tail default before it can own code changes.


## Decision update (2026-10-15T01:21:03Z)

- User selected source-end truncation for legacy USTX FX and versioned FX-state snapshots for seek. This resolves the two product questions recorded in the prior audit.
- Replaced the stale blocker wording with the remaining engineering gates: checkpoint identity/version/invalidation contract review and explicit ownership for T033 stream integration paths. Proposed invariants are recorded above for review; no source, public DTO, or capability was changed.
- T033 remains native/Web only; Android paths stay excluded by the user's latest instruction.


## MK3 seek-design continuation (2026-10-15T01:23:02Z)

- Compared MK3 §§6–8 and §9.3 with the selected behavior. MK3 already has half-open `range.start_frame`, separate `stream.seek.v1` capability, pinned render revisions, generation fencing and legacy `UMixFx` identity, but it does not yet define a checkpoint DTO/cache lifecycle or seek restore/fallback details.
- Added a T034 candidate design: internal session-local versioned checkpoints, full render/FX identity and digest checks, completed-block capture, nearest-compatible restore plus discard to the requested frame, deterministic fallback replay, generation invalidation, bounded count/bytes/TTL, and no tail extension for legacy USTX FX. This remains a proposal for review, not an accepted public DTO or live implementation.
- Added the first T018-owned implementation primitive in `native/crates/mixer-fx/src/openutau.rs`: process-local version-1 snapshot/restore with configuration and sample-rate checks. Focused suite passes 8 unit + 4 integration tests.
- Stream/server/Web integration, cache identity/lifecycle and public DTO remain unimplemented. T034 still needs a bounded handoff for overlapping T033 native/Web paths before those files can change. Android remains excluded.

## User decision confirmation (2026-10-15T01:51:25Z)

- User reconfirmed the T034 behavior: legacy USTX track FX keeps the source-bounded end in both streaming and offline output, and seeking uses versioned FX-state snapshots. Android work is still deferred.
- Made the MK3 `stream.seek.v1` task boundary explicit: restore the nearest compatible session-local snapshot, process/discard to the requested frame, and fall back to replay from frame zero when no compatible snapshot exists. Snapshot identity mismatches invalidate reuse. Exact DTO/cache limits and implementation ownership remain review gates; this does not advertise or implement the capability.
- Next: obtain bounded ownership for T033 native and Web seek integration paths, then review the response/error DTO and cache limits before implementation. No Android, runtime source, or public live schema was changed.

## MK3 seek DTO proposal (2026-10-15T02:01:11Z)

- Added a protocol-level proposal that reuses the existing `session_render_stream_begin` half-open range and `range.start_frame`; no new seek method, checkpoint ID, DSP state bytes, or seek-only response fields are added. Without `stream.seek.v1`, nonzero start returns `unsupported_range`; with it, the new stream has a fresh generation and sequence zero, and its first PCM is labelled at the requested frame.
- The playback client is responsible for cancelling/flushing its prior generation before accepting the new one. The engine restores the nearest compatible session-local FX snapshot and processes/discards to the target; absent or invalid snapshots fall back to deterministic replay from frame zero. Checkpoints bind revision/render/build/asset/FX/sample-layout identity and per-track FX frontier, are captured only at completed block boundaries, and use bounded internal cache limits. They cannot skip voice-source generation.
- This is a concrete proposal for review, not a public contract ratification. Existing stream capabilities remain unadvertised; no runtime/session/server path changed. T034 still needs T033 path ownership and implementation/acceptance after T033 base streaming gates.

## Dependency recheck (2026-10-15T02:04:02Z)

- T033 added regression coverage for cancellation while session credentials, worklet loading, or stream begin are pending; its focused Web suite now passes 8/8 and the production Web build passes. This closes player-startup cancellation races, not actual host playback, credential bridge, memory/request-budget or capability gates. T034 remains sequenced after those T033 base-streaming gates.
- No T034 runtime path was edited. The candidate protocol section and internal snapshot invariants are ready for review; only the source handoff, host gates, and implementation verification remain.


## FX checkpoint and parity update (2026-10-15T01:31:26Z)

- T018's process-local version-1 snapshot/restore primitive remains configuration- and sample-rate-bound. Its FX implementation now matches a deterministic OpenUtau C# reference across EQ-only and reverb-only 320,000-sample runs exactly and the full chain within max error `1.1920929e-7` / RMSE `1.42180755e-8`; the focused suite passes 8 unit + 4 integration tests.
- This improves the sequential FX state that T034 will checkpoint, but does not implement cache ownership, render identity, seek range, or server/Web integration. T034 remains blocked on the T033 stream-path handoff and review of the public seek DTO/capability.

## Snapshot byte accounting (2026-10-15T02:15:08Z)

- T018 added `OpenUtauTrackFxSnapshot::heap_payload_bytes()` in its owned mixer-FX path. It counts the snapshot and boxed processor footprints plus actual owned string/vector capacities, including each Freeverb comb/allpass/predelay buffer. Allocator metadata and external cache/container overhead are not included and must be budgeted separately.
- Exact fixture charges: 139,018 B at 44.1 kHz, 151,090 B at 48 kHz, and 300,298 B at 96 kHz for enabled `small_room` FX. The independent capacity-sum unit test and full `mixer-fx` suite passed (9 unit + 7 integration).
- This is an FX memory-charge primitive only. It does not add a checkpoint cache, render identity, seek handling, or stream/server/Web behavior; those remain gated by the T033 path handoff and stream acceptance.

## Decision reconfirmation (2026-10-15T02:21:28Z)

- User reconfirmed that legacy USTX track FX remains source-bounded in both stream and offline output, and that seek reconstruction should use versioned FX-state snapshots. These match the existing MK3 proposal and do not require a contract change.
- T034 remains blocked on T033 base-streaming host gates and explicit ownership handoff for overlapping native/Web stream paths. Android remains deferred. No implementation or capability advertisement is authorized by this confirmation alone.

## User confirmation (2026-10-15T02:45:05Z)

- User again selected source-end output for legacy USTX track FX in both streaming and offline rendering.
- User again selected versioned FX-state snapshots for streaming seek reconstruction. Keep snapshots session-local and identity/version-bound as proposed; cache/seek integration still awaits the T033 host gates, API review, and explicit source-path handoff.
- Android work remains deferred. No route, capability, DTO, or runtime behavior was changed by this decision.


## Preset parity continuation (2026-10-15T01:34:19Z)

- T018's FX processor was swept against OpenUtau C# `MixFxSource` for all named EQ, compressor and reverb presets on a deterministic 160,000-frame stereo fixture. EQ profiles were bit-exact; every other profile was bit-exact or differed by at most one f32 ULP. A Rust regression test now pins three reference samples for each profile; the mixer-fx suite passes 8 unit + 4 integration tests.
- The improved FX implementation strengthens checkpoint correctness, but seek integration/cache behavior is still absent; no stream/server/Web paths or capability advertisement changed.




## Snapshot contract audit (2026-10-15T01:38:26Z)

- The current `OpenUtauTrackFxSnapshot` is a private, process-local clone (`Box<OpenUtauTrackFx>`) with version 1, exact `UMixFx` configuration, and sample-rate identity. It is not a serialized `state_bytes` payload and has no portable digest. Keep the checkpoint opaque/in-memory; do not expose its DSP state to Web clients or persist it in `.rustx`.
- The proposed outer `track_fx_states[]` should therefore refer to typed Rust snapshots, with a processor-state version and a measured byte charge. Only introduce serialization/digests if a future cache crosses process boundaries. The bounded cache must account for cloned Freeverb delay buffers, not only metadata.
- A timeline-wide `frame_exclusive` is insufficient by itself: the streaming mixer tracks `processed_frame` per track and may stop processing an ended track before the master cursor. Record each track's FX `processed_through_frame` (or capture only a proven common frontier) so restored DSP state cannot be paired with the wrong source-bus cursor.
- The render identity must include immutable voicebank/sample and project-media bytes, renderer/configuration identity, and the ordered FX configuration. Project revision alone is insufficient because bank/media bytes may change without a Session edit. Invalidate the entire key on any identity/version mismatch; restore into temporary processors and commit only after every track passes validation.
- FX checkpoints accelerate restoration of the stateful post-source track FX only. They do not prove that voice synthesis can begin at an arbitrary frame without re-rendering earlier source material. T034 acceptance must separately measure source preparation/replay and FX restore/discard cost; cold seeks may replay from frame zero. No latency claim should be inferred from snapshot hits alone.
- The current implementation still rejects nonzero `range.start_frame`, returns `[0,end)`, and exposes no seek capability. The MK3 draft's seek capability/response diagnostics remain a proposal; frozen CONTRACT-v1 is unchanged. This audit is design-only and did not edit T033/T018 runtime paths.

## Contract tail-policy clarification (2026-10-15T01:35:17Z)

- Updated the MK3 draft at §6, §9.2/§9.3, §13 and C17: note releases and explicitly declared processor tails may drain; legacy USTX `UMixFx` never extends past source end; a new SoundCraft-derived insert can declare its own bounded tail. This removes the blanket tail-drain wording that contradicted the user's choice.
- This changes planning text only. No stream range/API/capability implementation changed; T034 still awaits ownership for the T033 stream/server/Web paths and review of the seek DTO.

## User decision confirmation (2026-10-15T03:01:42Z)

- User confirmed that legacy USTX track FX keeps OpenUtau's source-bounded duration in both streaming and offline output. Tail draining remains reserved for effects that explicitly declare a bounded tail under the newer insert model.
- User selected versioned FX-state snapshots for streaming seek, matching the existing proposal for session-local, identity/version-bound checkpoints. This does not authorize snapshot-cache or seek implementation by itself.
- T034 remains blocked on T033 host gates, seek API review, and explicit native/Web stream-path ownership. Android work remains deferred; no runtime source or capability advertisement changed.

## Checkpoint cache sizing audit (2026-10-15T03:05:36Z)

- Converted T018's measured `small_room` snapshot payload into a sizing example for four FX-enabled tracks. One checkpoint set charges 556,072 B at 44.1 kHz, 604,360 B at 48 kHz, or 1,201,192 B at 96 kHz, before allocator/cache-container overhead. A hypothetical 32 MiB cap would retain at most 60, 55, or 27 such sets respectively.
- If those sets were distributed uniformly over a 288-second project, nominal checkpoint spacing would be about 4.8 s, 5.2 s, or 10.7 s. This illustrates the tradeoff between memory and post-restore FX replay; it is not a selected product budget, a measured seek latency, or a bound for other FX configurations/track counts.
- Before implementation, T034 still needs a reviewed per-session byte cap, deterministic placement/eviction policy, and test matrix for variable FX payloads/track counts. Any claim that snapshots improve total seek latency must separately measure source regeneration, which FX snapshots do not skip. No runtime/API/source changes were made.

## Seek wire-surface review (2026-10-15T03:06:37Z)

- The proposed public surface can reuse `session_render_stream_begin` with its existing half-open `range.start_frame`; it needs no seek-only operation, checkpoint ID, or DSP state in JSON. `stream.seek.v1` capability negotiation gates nonzero starts, and the existing stream generation/sequence/frame labels plus client cancel/flush rule fence old PCM. A pre-capability nonzero request should keep returning `unsupported_range`.
- This shape aligns with the user's versioned-snapshot decision and keeps checkpoint representation internal. It does not change frozen CONTRACT-v1 or ratify MK3. T033 currently rejects both nonzero starts and bounded ends; this must remain so until the T034 capability is reviewed and implemented.
- A request with equal explicit bounds is already defined as empty; the unresolved range case is an open-ended start strictly beyond computed source end. Seek error detail and cache byte/placement/lifetime limits also need review. Do not infer a clamp from the source-bounded FX tail policy. No runtime/API source changed.

## User decision reconfirmation (2026-10-15T03:15:39Z)

- User reconfirmed that legacy USTX FX output stays source-bounded in both streaming and offline renders, and that streaming seek uses versioned FX-state snapshots. These choices match the existing MK3 proposal; no public contract change is required.
- The source-end FX tail question is resolved. Remaining design review items are seek behavior at/past computed source end, error detail, and cache byte/placement/lifetime limits. Implementation remains sequenced after T033 base-streaming gates and an explicit bounded T034 handoff for overlapping stream paths. Android stays deferred; no runtime path or capability was changed.

## Range-semantics source audit (2026-10-15T03:17:33Z)

- Checked the approved MK3 §6.1 range rules against the current T034 note and server validator. MK3 defines a half-open range, accepts equal bounds as an empty request, and requires an empty project/range to terminate with `end` and no PCM. Therefore, once `stream.seek.v1` is implemented, a request whose explicit `start_frame == end_frame` has an established empty-range result; this does not authorize a nonzero start while the capability is absent.
- The contract does not define what to do when `start_frame` is strictly greater than the computed source end while `end_frame` is open. Keep that as a concrete review item; do not infer a clamp from the USTX FX source-bounded tail rule. Current server still rejects every nonzero start before it can evaluate this case. No runtime or contract source changed.


## Source-path recheck (2026-10-15T03:37:57Z)

- Current `stream_ws::validate_stream_options` rejects every nonzero `start_frame` and every bounded `end_frame` as `unsupported_range`; it cannot yet evaluate a request against computed source end. This keeps seek unavailable until the T034 capability and implementation are reviewed.
- `pipeline::streaming::TrackStream` owns a separate FX processor and `processed_frame` per track. `flush_until` caps each processed segment at `final_input_end()` once future sources are exhausted, applies `fx.process` only to that source segment, then advances the cursor; it does not drain a legacy USTX FX tail beyond source end. The user's source-bounded choice matches current sequential streaming behavior.
- `OpenUtauTrackFxSnapshot` already provides an internal typed versioned clone, exact FX-config/sample-rate matching, atomic restore, and payload byte accounting. T034 still lacks the outer render identity/per-track frontiers, a session-local cache, range seek/replay, and end-to-end stream wiring.
- Review recommendation for remaining policy: when seek is enabled, an open-ended request starting strictly after computed source end should terminate immediately with `end` and no PCM at the actual source end (empty intersection of the requested half-open range with project audio); do not clamp to the source end. A 32 MiB per-session cache with deterministic oldest-checkpoint eviction and close-time clearing is a reasonable initial cap: the existing four-track `small_room` example charges 556,072 B per set at 44.1 kHz (~60 sets before container overhead). These two recommendations remain unconfirmed.
- No runtime, contract, capability, or Android path changed. T034 remains blocked on those policy decisions, T033 base-stream host gates, and a bounded path handoff for T033/T018 overlap.

## Product decision reconfirmation (2026-10-15T03:52:17Z)

- User reconfirmed that legacy USTX track FX remains source-bounded for both streaming and offline output, and selected versioned FX-state snapshots for seek. Both choices match the accepted MK3 design; this does not authorize Android work or advertise seek capability.
- The remaining T034 product decisions are the open-ended seek case strictly past source end and the per-session checkpoint cache budget/eviction/lifetime. Current recommendations remain `end` with no PCM and 32 MiB/session with oldest-checkpoint eviction and clear-on-close.
- No source, API, contract, or capability changed. Implementation still follows T033 base-streaming gates and requires bounded ownership for overlapping T033/T018 paths.

## Confirmed seek/cache policy (2026-10-15T08:25:58Z)

- User confirmed that an open-ended seek strictly past source end returns terminal `end` with no PCM.
- User approved a 32 MiB per-session checkpoint cache, deterministic eviction of the oldest checkpoint when full, and clearing on session close. Checkpoint payloads must be charged by actual bytes and remain identity/version bound as already proposed.
- These decisions resolve T034's open range/cache policy questions; T033 host/capability gates and bounded source ownership for overlapping stream/FX paths remain. No runtime, contract, or capability changed.

## MK3 base updated with confirmed policy (2026-10-15T08:30:39Z)

- Added the confirmed `stream.seek.v1` past-source-end result and 32 MiB/session checkpoint eviction/close policy to `docs/planning/contract-mk3-proposal.md` §1 and §6.1, and removed the obsolete “exact cache limits require review” blocker from its capability table.
- The contract remains a logical draft baseline and does not claim seek is implemented or advertised. T034 still needs the T033 host gate and bounded source ownership for shared stream/FX files before runtime work.
