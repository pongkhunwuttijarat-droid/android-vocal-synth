# T040 — E1 native ORT versus Burn/WASM parity gate

**Status:** blocked — needs separate model-experiment/build/test authorization  
**Owner:** unassigned — proposed Engine verification lane  
**Last updated (UTC):** 2026-10-08T12:07:41Z

## Scope and deliverables

Establish model-specific numerical and audio parity between the existing native ONNX Runtime reference and the E1 Burn/WASM known-family importer, plus cache miss/hit/restart correctness and measured performance. Do not infer parity from model parsing, shape checks, or generic Burn examples.

## Existing task relationship

- Depends on E0 [T037](T037-burn-onnx-model-inventory.md), runtime importer [T038](T038-burn-vocoder-runtime-import.md), and cache [T039](T039-burn-opfs-runtime-cache.md).
- Reuse real native fixtures/reports from T005/T007 only after verifying model identity; no UI integration is in scope.

## Dependencies and blockers

- Blocked on explicit permission to convert/load/run models and build/test, plus approved model license and parity methodology.
- T015 ONNX-WASM limitation remains in force until this model's full E4 gate and separate architecture/capability review.

## Likely files / ownership

Isolated Engine tests, deterministic input/output fixtures, parity report and cache integration tests. Avoid committing proprietary model weights or generated artifacts unless licensing/distribution is approved.

## Acceptance and evidence

- [ ] Run same real model, inputs, preprocessing and output contract through native ORT and Burn WASM; pin seeds/versions and compare tensor and audio metrics.
- [ ] Require output shape/rate correctness and report numerical error, perceptual/audio metrics, and any approved tolerances from a measured baseline.
- [ ] Prove cache miss/hit/restart yields equivalent output and does not silently load stale model/mapper versions.
- [ ] Measure import/inference wall time, cold/warm time, peak memory, and target-device/browser limits.
- [ ] Record a pass/fail decision for this model only; no universal ONNX claim.

## Concurrency and approvals

After T038/T039; parity runs may be independent from UI work but require model execution and target-specific verification approval. Keep test data/artifacts in the isolated Engine worktree.

## Repository checkpoint and next step

- Branch: future isolated Engine worktree only; candidate branch not created.
- Plan reference: [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Next: wait for importer/cache acceptance and experiment authorization before executing any parity run.