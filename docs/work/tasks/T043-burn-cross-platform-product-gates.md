# T043 — E4 per-model cross-platform and product capability gates

**Status:** blocked — needs E1/E3 evidence and separate product/architecture approval  
**Owner:** unassigned — proposed Engine platform acceptance lane  
**Last updated (UTC):** 2026-10-08T12:07:41Z

## Scope and deliverables

For each model that passes prior gates, validate native/Android/Direct WASM browser targets, device performance and memory, fallback behavior, and truthful renderer capability reporting. E4 is per model/family, not a universal ONNX approval.

## Existing task relationship

- Depends on E1 parity [T040](T040-burn-onnx-parity.md), and E3 [T042](T042-diffsinger-runtime-compatibility.md) only for selected DiffSinger stages.
- Reuse queued [T007](T007-render-performance-gates.md) device/stress methodology and coordinate existing [T005](T005-diffsinger-acceptance.md) native device evidence; neither task by itself proves Burn/WASM compatibility.
- Preserve [T015](T015-media-mixdown-and-real-project-render.md) ONNX-on-WASM limitation until this gate and separate review pass.

## Dependencies and blockers

- Blocked until per-model E1/E3 acceptance, supported-device matrix, and explicit product/architecture approval.
- Any change to `known-limitations`, public capabilities, native packaging, dependencies, or Web UI integration requires separate authorization.

## Likely files / ownership

Model-specific platform tests/reports in the isolated Engine worktree; capability/limitation documentation only after approval. Native Android, Web/WASM and product capability surfaces have separate owners and require coordination before changes.

## Acceptance and evidence

- [ ] Run each supported model on approved native, Android and browser/WASM targets; record exact device/browser/OS/backend and model hash.
- [ ] Measure cold/warm import and inference, peak memory, throughput/latency, cache behavior, failure recovery, and device limits; reuse T007 methodology without inventing thresholds.
- [ ] Verify unsupported device/model combinations report accurately and route to approved native/server fallback.
- [ ] Update capability reporting and known limitations only after model-specific evidence and separate review; leave Path B excluded unless separately approved.

## Concurrency and approvals

Later than E1 and per-stage E3; can reuse T007 reports only when target/model identity matches. Requires separate platform, capability, packaging, and product integration approvals; no changes authorized now.

## Repository checkpoint and next step

- Branch: future isolated Engine worktree only; candidate branch not created.
- Plan reference: [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Next: after prior gates and approvals, select one model/device combination for E4 qualification.