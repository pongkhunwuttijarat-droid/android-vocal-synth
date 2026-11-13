# T036 — Prepare isolated Engine worktree and branch

**Status:** done — isolated worktree prepared  
**Owner:** DSH coordinator  
**Last updated (UTC):** 2026-10-08T12:52:00.507Z

## Scope and deliverables

Prepare a clean, separate Burn/ONNX Engine worktree and candidate branch `engine/burn-onnx-wasm-investigation` only after explicit authorization. Verify approved base branch/commit, repository worktree conventions, branch naming, cleanliness, and artifact isolation. The current working tree is not the Engine worktree.

## Existing task relationship

- Enables [T037](T037-burn-onnx-model-inventory.md) and all later Engine E0–E4 tasks.
- Follows the isolated-worktree policy in the [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md); UI P0/P1 remains independent.

## Dependencies and blockers

- Branch/worktree creation was completed under explicit Wave 1 authorization on 2026-10-08; no production source was changed.
- Verified base: `wasm-native-rust` at `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`.

## Likely files / ownership

No production files were changed. T036 created only the isolated branch/worktree Git metadata and path; the Taskboard remains central. Keep generated models/import artifacts inside the Engine worktree and avoid shared dirty files.

## Acceptance and evidence

- [x] Record approved base branch and commit, existing worktree convention, and exact Engine worktree path.
- [x] Create candidate branch/worktree only after authorization; verify clean status and no overlap with current UI checkout.
- [x] Confirm Taskboard remains the single central board and record the Engine checkout location without adding machine-local-only assumptions.

## Concurrency and approvals

T036 prerequisite is complete for the authorized T037 read-only inventory. Other Engine tasks remain separately gated; do not use the current UI working tree for Engine edits.

## Wave 1 completion checkpoint (2026-10-08)

- Authorized base: `wasm-native-rust` at `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`. Created branch `engine/burn-onnx-wasm-investigation` at that exact commit in `/media/DATA/project/android-voice-synth-engine-burn-onnx-wasm-investigation`; it was clean at creation. Main checkout was left on its existing branch with 236 dirty/untracked entries of unknown ownership.
- Isolation: main checkout has `native/target`; the new worktree had no `native/target` at creation, so Cargo outputs are separate. No build/test was run. T037 may write only its inventory report in the isolated worktree and may read candidate assets in the main checkout read-only.
- T036 completion criteria are met; no further Git operation is authorized by this record.

## Repository checkpoint and next step

- Current checkout: `wasm-native-rust`, HEAD `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`; Taskboard/planning files are untracked in this checkout, and the wider dirty tree has unknown ownership.
- Candidate branch/worktree: `engine/burn-onnx-wasm-investigation` created at the approved base in `/media/DATA/project/android-voice-synth-engine-burn-onnx-wasm-investigation`.
- Plan reference: [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Next: T037 continues only the authorized read-only inventory; all other Engine tasks remain blocked pending their separate approvals.