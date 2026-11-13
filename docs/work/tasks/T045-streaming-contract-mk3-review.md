# T045 — P2A streaming and virtual-instrument contract MK3 review

**Status:** done — logical 3.0 implementation baseline accepted; DTO/runtime work is handed to downstream tasks  
**Owner:** Codex — contract review and documentation  
**Last updated (UTC):** 2026-10-15T01:35:17Z

## Goal

Produce a reviewable, versioned contract covering Rust-owned full-project incremental PCM streaming and virtual-instrument track backends before T033 implementation. The user selected MK3 as the logical 3.0 base for this feature set; later compatible capabilities are additive extensions to that base, while frozen logical v2 remains unchanged.

Keep `CONTRACT-v1.md` §0–§6 unchanged while reviewing. The repository currently contains the frozen logical v2 standard in §6 and an unapproved full-project streaming API proposal in T032; this task reconciles them rather than claiming the proposed stream API is live.

## Scope

- Compare the candidate stream API in [T032](T032-full-project-streaming-feasibility.md) and its [feasibility report](../../planning/t032-full-project-streaming-feasibility-wave1.md) against the frozen Project Session rules in [CONTRACT-v1 §6](../../CONTRACT-v1.md#6-project-session-api-draft-v1--implementation-ready-partially-implemented), the [v2 freeze proposal](../../planning/contract-v2-freeze-proposal.md), and the Rust `project-session` implementation.
- Define contract versioning and compatibility: MK3 is the proposed logical 3.0 base; later compatible features are additive named extensions, while breaking changes require a new major and migration. Specify negotiation/capability signaling, unknown fields/versions, and behavior for clients that do not support streaming. Do not silently rename frozen CONTRACT-v1 or claim v2 product readiness.
- Specify begin/next/cancel/close lifecycle; expected revision and immutable render input; stream/session identity and generation; block ordering and frame ranges; format/rate/channel metadata; bounded payload size; data/terminal/cancelled/stale/error outcomes; idempotency and resource cleanup.
- Specify host adapters and capabilities separately for Direct WASM/worker calls and native local-server WebSocket PCM transport (Android target). Existing native HTTP control/render-job APIs are not progressive PCM evidence; do not imply unimplemented routes are live.
- Define a Rust-owned virtual-instrument track model selecting SoundCraft built-ins or hosted plugin formats (including VST3 where the host supports it), or RustySynth SoundFont/SF2. Treat RustySynth as a SoundFont synthesizer backend, not a VSTi plugin format.
- Specify stable track/part/event IDs, MIDI note and required event DTOs, backend identity and plugin state references, SoundFont asset references, unsupported/missing backend behavior, and per-host capability negotiation. Keep external plugin/SoundFont payload redistribution separate from crate/source licensing.
- Specify concrete SoundCraft/SoundFont reads and controls: catalogue, parameters/presets, sampler/SF2 binding, 16 independent SoundFont channel presets/controllers, source options, state capture/restore, native editor, audition/panic and declared per-component MIDI/timing capabilities.
- Specify that USTX track FX and new mixer FX are distinct project chains with independent semantics, including the T018 `UMixFx` read/write value, bounds, chain order and render invalidation; this contract may require cross-block FX continuity but does not conflate their serialization.
- Define producer/consumer responsibilities: global voice and wave ordering, bounded buffering, backpressure, underrun policy, transport clock, cancellation, and stale-block suppression.
- Define audio continuity obligations: stable USTX track FX state/tails across blocks, with new SoundCraft-derived mixer FX remaining a separate chain. MK3 defines the logical product model; T044 owns the SoundCraft crate reuse audit and implementation roadmap.
- Deliver canonical DTO examples, error/state tables, lifecycle/state diagrams or equivalent, conformance vectors, and an implementation/readiness gate for T033. Include security/resource limits for stream handles, memory, concurrency, and maximum block sizes.

## Completion criteria

- [x] User selected MK3 as the logical 3.0 base contract, with later compatible work added through named/versioned extensions and breaking changes requiring a new major plus migration. Formal acceptance of the full draft remains separate.
- [x] Stream lifecycle, revisions, block ordering, bounds, cancellation, freshness and terminal/error semantics are specified in the draft.
- [x] Direct WASM/worker and native local-server WebSocket mappings are specified without claiming routes are live; legacy HTTP completed jobs are not progressive PCM evidence.
- [x] Virtual-instrument track/backend model distinguishes SoundCraft built-ins/hosted formats from RustySynth SoundFont and declares host support through capabilities.
- [x] Backend identity, MIDI event semantics, state/asset references, unavailable-backend errors, and effect-chain separation are specified; entity IDs explicitly reuse frozen lowercase UUID shape.
- [x] Initial SoundFont supports 16-channel binding semantics and declared block timing; SoundCraft VSTi channel/program/editor extensions are listed as integration targets and remain unadvertised until proven.
- [x] Existing frozen Project Session semantics and USTX compatibility are preserved in the proposal; new mixer FX remains separate from USTX track `MixFx`.
- [x] Canonical examples and C01–C47 conformance specifications cover stream, backend and additive-contract evolution cases. These are requirements, not test results.
- [x] User explicitly authorized proceeding against MK3 as the logical 3.0 base. This accepts the baseline for implementation; it does not claim that routes, Rust DTOs or runtime capabilities are implemented.
- [x] T033 points to the accepted logical 3.0 baseline and C01–C47; implementation remains separately gated on source ownership, T024/T011 sequencing and evidence.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit at review planning: `e6af244abbf04d4a490fca8483295ed6418e6cad`
- Relevant working-tree changes: `docs/work/BOARD.md`, T044, this T045 record, the narrow T033 planning delta and MK3 draft/review under `docs/planning/` are untracked. These documentation deltas are registered in this task; many other files are modified/untracked with unknown ownership. The frozen contract and product source were not changed. Do not attribute other dirty files or edit shared contract/source files without ownership confirmation.
- Durable references: [T032](T032-full-project-streaming-feasibility.md), [T033](T033-session-streaming-product-integration.md), [T044](T044-mixer-streaming-and-soundcraft-reference.md), frozen [CONTRACT-v1 §6](../../CONTRACT-v1.md#6-project-session-api-draft-v1--implementation-ready-partially-implemented), and [CONTRACT-v2 freeze proposal](../../planning/contract-v2-freeze-proposal.md).

## Progress

- 2026-10-14T06:52:49Z Read-only check found no file named CONTRACT-MK3. `CONTRACT-v1.md` identifies §0–§5 as legacy frozen v1 and §6 as the frozen logical v2 standard; T032 separately contains an unapproved streaming API proposal. Added this task as the contract review prerequisite for T033; no contract or source file was changed.
- 2026-10-14T07:08:12Z At the user's request, drafted [CONTRACT-MK3 proposal](../../planning/contract-mk3-proposal.md) covering streaming PCM plus virtual-instrument tracks with SoundCraft-hosted plugin or RustySynth/SF2 backend selection. The draft is explicitly unapproved and leaves versioning, transport, plugin formats by host, PCM framing, asset packaging, MIDI scope, FX placement, and measurable targets open for review.
- 2026-10-14T07:09:11Z Preserved the user's earlier target decision: Android uses a WebView UI with a local Rust server/WebSocket bridge; the draft now treats this as selected architecture while leaving exact protocol framing and compatibility details for review.
- 2026-10-14T07:17:58Z Completed the user's requested requirement/implementability review in [MK3 review](../../planning/contract-mk3-review-2026-10-14.md). Verdict: direction is covered but not all latest goals, and the protocol is not implementation-ready. F1–F9 cover missing SoundCraft built-ins, portable/exclude assets and USTX audio export, pending stream responses, exact MIDI/backend schema, mixed voice/wave track compatibility, mixer/FX schema, invalidation/cache semantics, runtime backend state capture, and phase-specific T033 gates. The contract draft and product source were not modified by the review.
- 2026-10-14T08:02:09Z Revised MK3 under the user's instruction to add review findings and discuss ambiguity. User answered all three product questions: stop old audio and require Play after audio-affecting edits; keep editable data and relink missing assets without embedding fallback audio; place new track FX before fader/pan with legacy USTX FX afterward. Recorded these decisions directly in the contract.
- 2026-10-14T08:02:09Z Added SoundCraft builtin/hosted variants, precise initial MIDI data/commands, mixed-track compatibility, pending/terminal stream states, bounded pull and binary WS framing, source/FX/mix invalidation, runtime editor/state capture, proposed `.rustx` v2 extension/assets/exclude rules, explicit USTX bake modes and C01–C25 conformance specifications. Narrow T033 planning edits now link T045, distinguish native WS from completed HTTP jobs, and keep instruments/packaging separate from base streaming. Product source and frozen v1/v2 contract remain untouched.
- 2026-10-14T08:43:42Z Completed [round-2 instrument-control audit](../../planning/contract-mk3-audit-round2-2026-10-14.md) and added its findings to MK3 §3/§4/§5/§14. User chose multitimbral SoundFont control through all 16 MIDI channels from the first release and accepted declared block timing. Added catalogue/preset/read APIs, explicit SoundCraft/SF2 persistent controls, editor/audition lifecycle, source settings and exact capability rejection rules, with C26–C38.
- 2026-10-14T08:43:42Z Inspected RustySynth published crate 1.3.6 read-only, SHA-256 `e3ca93af923df5fc03beddbf464242620fd24daa9e10f9ecd56eb9571eb7ba38`, without adding a dependency. Source showed GM/default preset fallback, special percussion mapping, unsupported CC32 and cached block timing. Pinned SoundCraft VST3/CLAP source showed channel-0 note paths and absent VST3 MIDI Program Change handling. The draft now declares/guards these limitations rather than claiming universal support. No product source or Cargo change.
- 2026-10-14T09:21:28Z User clarified that SoundCraft libraries/adapters are expected to be modified. MK3 now treats audited channel/program gaps as integration targets, not permanent contract restrictions, and specifies channel-aware host events, verified program mapping and adapter services. C36/C37 distinguish patched host capability from independent plugin limitations. No library patch or source implementation was performed.
- 2026-10-14T10:31:12Z Expanded §4.8 with a SoundCraft-hosted VSTi feature/evidence matrix and adapter targets for channel plus frame-offset MIDI, program/bank mapping, control metadata, state/editor lifecycle, bus/layout/latency/tail preparation and realtime/offline parity. Added C39–C46; these are acceptance specifications, not passing tests. Created [T046](T046-soundcraft-hosted-vsti-adapter.md) to track SoundCraft host-library/Rust-adapter work separately from T033 streaming and T044 FX reuse. No product source/library patch or dependency change was made.
- 2026-10-14T10:31:12Z Read the full active/blocked board rows after the user's request to include other tasks. Fixed T045's stale “native HTTP” host-adapter wording to Direct WASM/worker plus Android local-server WebSocket, while keeping completed HTTP render jobs out of progressive-audio evidence. Read-only comparison against the visible v2 parser/identity implementation confirmed mutations use the existing `{expected_revision, command, include_snapshot?, group_id?}` envelope, `type`/snake_case command dispatch and strict lowercase UUID entity IDs; no instrument or stream command implementation was found in that dispatcher. Added the UUID/example-placeholder rule to MK3. No unowned source files were edited.
- 2026-10-14T10:37:48Z Tightened §4.2 to preserve the complete frozen mutation envelope, including optional `group_id`, closed `type`-tagged commands, snake_case fields and unknown-field rejection. Continued the user's broader task-board request with a read-only T002 SAF inventory; see T002. Existing dirty product files were not edited.
- 2026-10-14T10:37:48Z Extended the board sweep to the queued, unassigned T005–T007 items. Their task records now hold read-only evidence inventories: real DiffSinger models exist but timing/device acceptance is open; Vogen renderer/model is absent and provenance is unverified; performance evidence is desktop-only with Android/stress gates open. Board ownership is limited to documentation audits; no renders, builds, tests or benchmarks were run.
- 2026-10-14T10:46:15Z At the user's direction, consolidated the separate T018 per-track `UMixFx` contract into this canonical draft's §9.3. It preserves USTX `MixFx` semantics while specifying the separate new mixer-insert chain, their ordering, full `track.set_mix_fx` value/validation and render invalidation. The old T018 proposal remains as a historical redirect; T018 still owns implementation/parity follow-up. CONTRACT-v1 §6 is unchanged; this draft remains unapproved and no product code was edited.
- 2026-10-14T10:49:27Z User directed that MK3 be the stable base for implementation, with later work added rather than replacing/rewriting it for small changes. Recorded the additive-extension/erratum/breaking-major policy in §2.1, selected logical 3.0 as the base direction, and updated the remaining-review section: requirements are covered, but schema validation, ownership, runtime vectors and measured limits remain. Added C47 for extension/client compatibility. Full contract acceptance and implementation authorization are still open.
- 2026-10-14T10:52:35Z User instructed us to start implementation and work through blocked tasks goal by goal. Recorded explicit acceptance of the MK3 logical 3.0 baseline for implementation, marked the T045 review complete and authorized T033 as a separate implementation increment. No public routes or product capabilities are claimed live; T033 remains blocked only by downstream sequencing/ownership/evidence gates.

## Verification

- `rg --files docs | rg -i 'contract|api|mk3|mk2|v[0-9]'` — found CONTRACT-v1, the v2 freeze proposal, and T032 streaming proposal; no MK3 document found.
- Read-only review of `docs/CONTRACT-v1.md`, `docs/planning/contract-v2-freeze-proposal.md`, and T032/T033 records — confirmed the streaming signatures remain unapproved and the frozen contract was not edited.
- Read-only review of the official [RustySynth repository](https://github.com/sinshu/rustysynth) and [SoundCraft repository](https://github.com/storytold/soundcraft) — recorded their distinct instrument/plugin roles and license references; no dependency or product code was changed.
- Drafted `docs/planning/contract-mk3-proposal.md` only. No contract freeze, schema/API implementation, route, dependency, build, or test was performed.
- No build or tests run; this is a planning/review task.
- `nl -ba docs/planning/contract-mk3-proposal.md` plus read-only domain/pipeline, frozen §6.13–§6.15, `.rustx` and pinned SoundCraft registry/interface inspection — review evidence only. Source/host compatibility and audio/performance claims remain unverified.
- A search initially included nonexistent directory `native/crates/project-session/src/v2`; reran successfully against `native/crates/project-session/src` and found `src/v2.rs`. No source was modified.
- 2026-10-14T08:04:08Z Documentation validation below passed for four documents and 16 JSON examples; local file link targets exist and code fences balance. This does not validate Rust wire serialization, Markdown anchor spelling or runtime compatibility. No product builds/tests were run.
- 2026-10-14T08:43:42Z Updated command below passed for five documents, 19 JSON examples and 38 sequential unique vector IDs; required control method names are present, local file links exist and fences balance. This is document QA only; no SoundFont/plugin runtime test or build was run.
- 2026-10-14T10:37:48Z Documentation QA passed for twelve documents, 19 JSON examples, C01–C46 sequential IDs, local file links and balanced fences. Read-only serde/API inspection is not a DTO validation: new stream/instrument shapes still lack concrete Rust types and the current command dispatcher does not implement them. No Rust serialization, plugin runtime, host build or audio measurement is claimed.
- 2026-10-14T10:49:27Z Re-ran document QA after the base-policy update: twelve documents, 20 JSON examples, C01–C47 sequential IDs, local links and code fences passed. This does not validate Rust serialization or runtime behavior; no build, plugin test or audio measurement was run.

Exact repeatable command from the repository root:

```sh
python3 - <<'PY'
from pathlib import Path
import json,re
paths=[Path(p) for p in ['docs/planning/contract-mk3-proposal.md','docs/planning/contract-mk3-review-2026-10-14.md','docs/planning/contract-mk3-audit-round2-2026-10-14.md','docs/work/tasks/T045-streaming-contract-mk3-review.md','docs/work/tasks/T033-session-streaming-product-integration.md','docs/work/tasks/T044-mixer-streaming-and-soundcraft-reference.md','docs/work/tasks/T046-soundcraft-hosted-vsti-adapter.md','docs/work/tasks/T002-flutter-saf-import.md','docs/work/tasks/T005-diffsinger-acceptance.md','docs/work/tasks/T006-vogen-renderer.md','docs/work/tasks/T007-render-performance-gates.md','docs/work/BOARD.md']]
count=0
for p in paths:
    s=p.read_text()
    assert len(re.findall(r'^```',s,re.M))%2==0,p
    for block in re.findall(r'^```json\n(.*?)^```[ \t]*$',s,re.M|re.S):
        json.loads(block)
        count+=1
    for link in re.findall(r'\]\(([^)]+)\)',s):
        if '://' not in link:
            name=link.split('#',1)[0]
            assert not name or (p.parent/name).exists(),(p,link)
p=paths[0].read_text()
ids=re.findall(r'^\| (C\d+) \|',p,re.M)
assert ids==[f'C{i:02d}' for i in range(1,48)],ids
for method in ['instrument.soundfont.set_channels','instrument.track.load_state','instrument.soundcraft.set_sample','instrument.editor.open','instrument.audition.panic']:
    assert method in p,method
for expected in ['group_id?', 'EntityId', 'snake_case', 'VSTi completeness matrix']:
    assert expected in p,expected
print(f'PASS: {len(paths)} documents; {count} JSON examples; 47 unique sequential vector IDs; local link targets exist; code fences balanced.')
PY
```

## Blockers and dependencies

- T045's contract-acceptance gate is complete. T033 implementation is authorized against this baseline but still depends on T024/T011 sequencing, measured baseline evidence and confirmed source ownership. Concrete Rust DTO/schema implementation remains downstream work; the visible v2 dispatcher does not implement the MK3 stream/instrument commands. SoundCraft VSTi implementation belongs to T046; C47 governs future compatible additions. The T018 legacy track-FX proposal is consolidated into §9.3 alongside the distinct new mixer insert model.
- Public Session/API, route, dependency, or schema changes remain unapproved until the review and approval gate is complete.

## Next step

Contract acceptance is complete. T033 should implement only its base voice/wave streaming slice against the accepted 3.0 baseline after T024/T011 sequencing and path ownership are resolved; hosted VSTi, seek, routing and bundle support remain separate capability increments.

## Handoff notes

Do not edit frozen CONTRACT-v1 in place during the review. The current T032 candidate API is not live. T033 must implement the approved artifact only; T044's separate SoundCraft-derived FX work must not expand T033's scope beyond the state/tail continuity required for streamed audio.


## User-selected legacy FX tail clarification (2026-10-15T01:35:17Z)

- Updated the MK3 draft's stream-drain state, end/truncation wording, §9.2/§9.3 FX semantics, `stream.sequential_fx.v1` gate and C17 vector so the legacy USTX `UMixFx` chain remains source-bounded in both streaming and offline output, while a future SoundCraft-derived insert may declare its own bounded tail. This records the user's explicit choice and removes the prior blanket “all FX tails drain” ambiguity.
- CONTRACT-v1 and runtime/schema code were not changed. The draft still does not claim an endpoint, DTO or capability is live. T045 remains done as the review/base task; this is an additive clarification to its unapproved extension draft.
