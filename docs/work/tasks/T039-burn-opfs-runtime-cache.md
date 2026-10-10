# T039 — E1 OPFS converted-model cache and reload

**Status:** blocked — needs separate cache-schema/implementation authorization  
**Owner:** unassigned — proposed Burn/WASM cache lane  
**Last updated (UTC):** 2026-10-08T12:07:41Z

## Scope and deliverables

Design and implement the approved two-level model cache: persistent OPFS converted weights plus metadata, and bounded in-memory loaded-model/weight reuse. Cache identity includes model content hash, importer version, architecture ID, converted-format version, and weight-mapper version. Define atomic/recoverable writes, validation, invalidation, quota handling, corruption recovery, duplicate-memory control, and reload behavior.

## Existing task relationship

- Depends on E0 [T037](T037-burn-onnx-model-inventory.md) and runtime importer [T038](T038-burn-vocoder-runtime-import.md).
- Distinct from completed [T003](T003-web-voicebank-opfs.md), which persists voicebank source files; this task caches converted model artifacts in the isolated Engine path.

## Dependencies and blockers

- Blocked on E0/E1 feasibility, separate cache schema and implementation approval, and a clean Engine worktree from T036.
- Do not modify browser OPFS voicebank storage or existing project cache behavior; no dependency changes authorized.

## Likely files / ownership

Engine runtime cache module, WASM storage adapter, cache metadata/version types, isolated tests and documentation in the Engine worktree. Any new OPFS interface or user data lifecycle policy requires separate review.

## Acceptance and evidence

- [ ] Persistent cache miss converts/imports once; hit reloads exact compatible artifact after browser/app restart.
- [ ] Model/importer/architecture/format/weight-mapper version changes invalidate precisely; corrupt/partial entries are rejected and safely regenerated.
- [ ] Quota denial and unsupported OPFS are recoverable with clear fallback/diagnostics; no silent data loss.
- [ ] In-memory cache is bounded, has explicit ownership/eviction, and avoids duplicate large buffers; report cold/warm latency and peak memory.
- [ ] Tests cover cache miss/hit/restart/invalidation/corruption/quota and exact artifact identity.

## Concurrency and approvals

Schema design may be reviewed with T038, but implementation must coordinate one owner for cache metadata and importer artifact format. Requires separate authorization; no current OPFS change is permitted.

## Repository checkpoint and next step

- Branch: future isolated Engine worktree only; candidate branch not created.
- Plan reference: [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Next: after T038 format/version approval and explicit authorization, implement the cache in isolation.