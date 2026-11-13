# T001 — Unify Rust project data holder across harnesses

**Status:** done  
**Owner:** Codex  
**Last updated (UTC):** 2026-09-30T08:35:00Z

## Goal

Use `project-session::Session` as the shared authoritative project data holder.
Native/server is the primary integration target; WASM is a second host that must
use the same command, identity, revision, group and persistence semantics.
Flutter migration is outside this task because it is not the current primary
target.

## Completion criteria

- [x] WASM `Engine` stores `project-session::Session` as its sole project holder; the legacy JS facade adapts through it and preserves CONTRACT-v1 §6.8.3 group behavior.
- [x] Native/server session routes use the shared session and match the reviewed frozen contract rules for stable-ID edits, groups, bounded reads, required revisions, save/reopen and render snapshot capture.
- [x] WASM exposes the shared stable-ID session API, including create/open with sidecar, commands, groups, history, bounded reads and sidecar save.
- [x] Native/server and WASM pass a common integration scenario for stable-ID edits, group commit, undo, sidecar save/reopen and revision outcomes.
- [x] Save and render inputs in Native/server consume the authoritative session document.

## Scope boundary

The browser `Driver` and OPFS persistence still use the compatibility facade and
YAML path. Migrating those UI/storage adapters is follow-up work; the WASM
session API and shared Rust data holder are now available. Flutter remains a
separate host migration and was not analyzer/build verified in this task.

## Source contract

- [Engine-owned state migration](../../architecture/engine-owned-state.md)
- [Shared Project Session contract](../../CONTRACT-v1.md#6-project-session-api-draft-v1--implementation-ready-partially-implemented)
- [Roadmap migration checkpoint](../../planning/roadmap/ROADMAP.md)

## Repository checkpoint

- Branch/base commit: `wasm-native-rust` at `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f` when claimed.
- The checkout already had extensive modified and untracked work before T001; ownership of unrelated files is not established. Do not attribute the broader working tree to this task.

## Progress

- 2026-09-30: `project-session` core supplied stable IDs, revisioned snapshots, stable-ID commands, curve strokes, isolated groups and conformance vectors.
- 2026-09-30: `engine-wasm::Engine` was moved to hold `project_session::Session`. Its legacy API remains a compatibility adapter, preserving immediate-apply group behavior from §6.8.3.
- 2026-09-30: Added a direct WASM session surface for new/open, snapshots, capabilities, stable-ID commands, groups, guarded undo/redo, bounded reads and sidecar save.
- 2026-09-30: Native/server audit corrected the part-scoped curve route, forwarded new-session name/BPM, enforced required `expected_revision` on save/group commit/undo/redo/render start, checked optional group-cancel revision, and aligned typed error envelopes/revision precedence.
- 2026-09-30: A planning failure in a group child now fails closed as `group_failed`, dropping the tentative group as required by §6.8.2.
- 2026-09-30: Added a Native/server ↔ WASM integration scenario covering stable-ID edits, grouped mutation/commit, undo, stale revision rejection, save/reopen and ID continuity.
- 2026-09-30: Removed an undocumented phonemize endpoint during contract review. Adding it requires an explicit contract update.

## Verification

- `cargo test -p synth-server --test api_v2_sessions` — 20 passed, 0 failed.
- `cargo test -p project-session` — all tests passed, including conformance vectors, identity, groups and v2 session behavior.
- `cargo check -p engine-wasm --target wasm32-unknown-unknown` — passed.
- `cargo check -p synth-server` — passed; existing unrelated workspace/server warnings remain.
- `git diff --check` — passed.
- Flutter analysis/build was not run; Flutter is outside this task's target scope.

## Freeze readiness follow-up

- 2026-09-30: Added [CONTRACT-v2 freeze proposal](../../planning/contract-v2-freeze-proposal.md), based on current §6 and adapter source. It separates decisions (wire schema review, `group_failed`, polling baseline, render optionality, source-to-WASM mapping and first command scope) from implementation gates and explicitly non-blocking follow-ups. The existing `CONTRACT-v1.md` is unchanged by this proposal; no freeze is claimed.
- Next evidence required before CONTRACT-v2 freeze is tracked separately in [T010 Contract adapter conformance](T010-contract-adapter-conformance.md): shared vectors via direct WASM + native HTTP; project lifecycle vectors (create/open/import/save/export/reopen/Save As); client stale-read handling; golden DTO fixtures; native render-job vectors. WASM currently advertises render jobs unsupported. Legacy Web Driver/OPFS still uses the compatibility facade and is not migrated to stable-ID Session calls.
- User clarified 2026-09-30: Native uses API routes; WASM is called directly/in-process (do not impose HTTP shape on WASM); open/create/import/save/export are required lifecycle operations for a usable editor; adapter parity tests must run before freeze; WASM async render is expected to be extended later. The freeze proposal records this; async-render initial requirement and the first command scope still need an explicit choice.
- Current §6.18 schema/result/error review and native progress transport choice remain. Recommended `group_failed` retention and polling baseline await explicit confirmation. No v2 freeze is claimed.
- 2026-09-30: Product accepted OpenUtau-compatible curve simplification for v2 strokes. §6.7.2 now specifies commit-time whole-curve RDP-like simplification (tolerance `min(5, descriptor_range*0.005)`, perpendicular distance, strict `>`, endpoints retained, first maximum wins) and conformance requirements; current Rust does not yet implement it. Redundant points outside the edited interval may be removed within tolerance to match the OpenUtau whole-curve call. `high-frequency-editing-flow.md` and the freeze proposal are aligned. Before merge/freeze, verify the source `Simplify()` recursion's exact indexing/degenerate-line behavior against these drafted rules; then implement integer/deterministic parity and vectors.

## Handoff notes

The shared data-holder task is complete for Native/server and the WASM engine API.
Continue browser `Driver`/OPFS migration as a separately scoped task if Web
becomes a target. Keep the ownership boundary in [AGENTS.md](../../../AGENTS.md)
and follow sections marked frozen in CONTRACT-v1 §6; do not claim the entire
v2 draft is frozen or fully implemented.
