# T041 — E2 conditional neural phonemizer/G2P runtime support

**Status:** blocked — needs E0 confirmation and separate implementation authorization  
**Owner:** unassigned — proposed Engine/phonemizer lane  
**Last updated (UTC):** 2026-10-08T12:07:41Z

## Scope and deliverables

Only if E0 confirms a real supported Rust product loader path and actual voicebank ONNX input, evaluate and add model-family-specific neural phonemizer/G2P support to the approved Burn/WASM path. Otherwise record “no supported product input” and keep this phase deferred; do not invent a loader or treat a reference asset/label as runtime evidence.

## Existing task relationship

- Gate is E0 [T037](T037-burn-onnx-model-inventory.md); depends on E1 known-family importer [T038](T038-burn-vocoder-runtime-import.md) and separate Engine worktree T036.
- [T006](T006-vogen-renderer.md) tracks the Vogen `f0_man`/VSVocoder renderer, not automatically a neural G2P loader. [T013](T013-phoneme-input-and-mapping.md) is done for rule/dictionary composition; it is not ONNX inference evidence.
- Existing Rust phonemizer has no verified ONNX loader path; preserve native/server fallback.

## Dependencies and blockers

- Blocked unless E0 identifies an actual model and active supported loader path. Requires separate model/dependency/implementation approval.
- Do not change phonemizer semantics or add dynamic ONNX execution under this task; any new public capability requires separate architecture review.

## Likely files / ownership

Only after E0: isolated Engine and, if strictly required, `native/crates/phonemizer/` integration with explicit owner agreement. No current main-worktree code or dependency changes.

## Acceptance and evidence

- [ ] Prove model is loaded from a real supported voicebank path by Rust product code, with version/hash/provenance and licensing.
- [ ] Validate operator/shape/weight compatibility and deterministic phoneme/G2P output against the existing approved native path.
- [ ] Unsupported models return explicit diagnostics and retain native/server fallback; no broad compatibility claim.
- [ ] If no actual path exists, close this phase as “not applicable/deferred” with the E0 evidence; no code added.

## Concurrency and approvals

Blocked behind E0 and E1; does not block E1 vocoder or other workstreams. UI G2P editing and Burn runtime are separate dependencies; coordinate any `phonemizer` file ownership before implementation.

## Repository checkpoint and next step

- Branch: future isolated Engine worktree only; candidate branch not created.
- Plan reference: [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Next: wait for E0's explicit loader-path decision; do not activate if evidence is absent.