# T038 — E1 known-family runtime vocoder import

**Status:** blocked — needs separate dependency/implementation authorization  
**Owner:** unassigned — proposed Burn/WASM Engine lane  
**Last updated (UTC):** 2026-10-08T12:07:41Z

## Scope and deliverables

Implement a model-family-specific runtime importer for the E0-approved real vocoder target: validate an ONNX graph and weight mapping, import compatible weights into a Burn architecture already compiled in the application, and run WASM inference without rebuilding the app for each compatible voicebank. This is Path A only; dynamic ONNX graph execution (Path B) is separate research and excluded.

## Existing task relationship

- Depends on [T036](T036-engine-worktree-preparation.md), E0 [T037](T037-burn-onnx-model-inventory.md), and the [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Preserve T015's ONNX-on-WASM limitation until model-specific E4 evidence and separate capability review; T020 is a host-worker baseline, not an ONNX proof.

## Dependencies and blockers

- Blocked on E0 selecting a compatible model/architecture and explicit dependency/version, implementation, and experiment approval.
- Official `burn-onnx` and `onnx-ir` remain reuse/reference candidates; do not build a custom general importer absent a demonstrated gap. Local Burn is 0.21 versus upstream 0.22.0; no upgrade is authorized.

## Likely files / ownership

Engine-only future paths under `native/crates/` and `native/Cargo.toml`/lockfile, model-specific importer/weight-map modules, and tests in the isolated Engine worktree. Do not edit UI/session contracts or current dirty checkout.

## Acceptance and evidence

- [ ] Validate graph/operator/shape/dtype/architecture and weight mapping before import; unsupported models fail with explicit diagnostics.
- [ ] Import weights at runtime into the compiled Burn family; demonstrate two compatible voicebank model instances without rebuilding WASM.
- [ ] Run the selected vocoder in WASM on deterministic input and verify output rank/shape/sample-rate contract.
- [ ] Record exact model hash, tool/dependency versions, conversion/import time, peak memory, and unsupported-operator results.
- [ ] No claim of universal ONNX compatibility or arbitrary graph execution.

## Concurrency and approvals

After E0 and T036, importer work may proceed independently of UI and streaming but must coordinate with T039 cache schema and T040 parity fixtures. No dependency/build/test/model inference is authorized until separate approval.

## Repository checkpoint and next step

- Branch: future isolated Engine worktree only; candidate branch not created.
- Plan reference: [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Next: wait for E0 decision and importer/dependency approval; then prototype only the approved family.