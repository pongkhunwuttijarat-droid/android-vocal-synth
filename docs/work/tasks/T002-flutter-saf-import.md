# T002 — Complete Android SAF project and voicebank import flow

**Status:** queued  
**Owner:** unassigned  
**Last updated (UTC):** 2026-09-30T05:25:19Z

## Goal

Let users import and manage project and voicebank files through Android's
Storage Access Framework, with the engine consuming the selected project and
bank through the existing host adapters.

## Completion criteria

- [ ] Confirm which project and voicebank import paths are missing in the current app.
- [ ] Import and reopen supported projects through SAF without creating a second authoritative project model in Flutter.
- [ ] Import/select voicebanks through the documented Android storage flow.
- [ ] Add host/UI coverage for selection, errors, and resulting engine inputs.

## Source planning

- [Roadmap MS4.3](../../planning/roadmap/ROADMAP.md)
- [MS4 productization](../../planning/milestones/ms4/MS.md)

## Repository checkpoint

- Branch/base commit: capture when claimed.
- Relevant working-tree changes: inspect and record when claimed; ownership unknown until checked.
- Durable artifacts: repository source and tests.

## Progress

- 2026-09-30: Added from roadmap notes that project save/open/autosave and WAV save exist while SAF import was listed as remaining. Recheck that snapshot before implementation.

## Verification

- Not run for this queued task.

## Blockers and dependencies

- Depends on the current project-session migration decisions for project import and save ownership.

## Next step

Inspect current Android SAF, project storage, and voicebank selection code; revise completion criteria to match the remaining gaps.

## Handoff notes

Use SAF as a host storage adapter. Project content and validation remain Rust-owned under the engine-state policy.
