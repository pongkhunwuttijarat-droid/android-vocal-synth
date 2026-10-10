# T042 — E3 broader DiffSinger ONNX-stage compatibility

**Status:** blocked — needs E0/E1 results and separate model-experiment authorization  
**Owner:** unassigned — proposed Engine/DiffSinger lane  
**Last updated (UTC):** 2026-10-08T12:07:41Z

## Scope and deliverables

After the vocoder-first E1 gates, inventory and validate broader DiffSinger ONNX stages (acoustic, duration, vocoder and real voicebank-specific dependencies) against compiled Burn families. Keep unsupported stages on the approved native/server path; full DiffSinger is not an E1 prerequisite.

## Existing task relationship

- Depends on E0 [T037](T037-burn-onnx-model-inventory.md), E1 import/parity [T038](T038-burn-vocoder-runtime-import.md)/[T040](T040-burn-onnx-parity.md), and isolated Engine worktree T036.
- Reuse [T005](T005-diffsinger-acceptance.md) for existing native timing/device acceptance only; do not conflate native DiffSinger timing with Burn/WASM model compatibility.

## Dependencies and blockers

- Blocked until E1 model-family gate passes and separate model/license/implementation approvals are granted.
- No general DiffSinger, dynamic-shape, or universal ONNX compatibility claim; do not update capability reporting under E3.

## Likely files / ownership

Engine-only ONNX inventory/importer/model-specific mappings and real model fixtures in the isolated Engine worktree. Do not change native DiffSinger runtime or T005-owned acceptance code unless the owner explicitly agrees.

## Acceptance and evidence

- [ ] Inventory real model/operator/shape/weight dependencies for each selected DiffSinger stage and identify compatible compiled family or explicit blocker.
- [ ] Validate at least one approved real fixture per selected stage against its native ORT reference with recorded numerical/audio parity.
- [ ] Report per-stage unsupported ops, dynamic-shape behavior, memory/time, and fallback; keep E1 vocoder results separate.
- [ ] Promote no stage until it independently meets its model-specific acceptance gate.

## Concurrency and approvals

Can split stage inventory after E0, but model execution waits for E1 and separate experiment approval. Coordinate T005 fixture/device ownership and T043 cross-platform gates.

## Repository checkpoint and next step

- Branch: future isolated Engine worktree only; candidate branch not created.
- Plan reference: [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Next: wait for E1 and choose one stage/fixture at a time; do not require full DiffSinger for E1.