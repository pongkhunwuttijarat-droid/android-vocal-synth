# T005 — Finish DiffSinger timing and device acceptance

**Status:** queued  
**Owner:** unassigned  
**Last updated (UTC):** 2026-09-30T05:25:19Z

## Goal

Define and pass the remaining DiffSinger timing and target-device acceptance
checks for the supported native product path.

## Completion criteria

- [ ] Identify the current timing mismatches and the agreed reference/acceptance tolerance.
- [ ] Verify representative projects and voicebanks on target Android hardware.
- [ ] Record runtime, memory, and output evidence for the supported configuration.
- [ ] Update the milestone status with exact evidence and remaining limits.

## Source planning

- [Roadmap MS3.3](../../planning/roadmap/ROADMAP.md)
- [DiffSinger milestone](../../planning/milestones/ms3/DIFFSINGER.md)

## Repository checkpoint

- Branch/base commit: capture when claimed.
- Relevant working-tree changes: inspect and record when claimed; ownership unknown until checked.
- Durable artifacts: repository tests, reports, and fixtures.

## Progress

- 2026-09-30: Added from roadmap snapshot describing tensor-to-WAV as working while timing/device acceptance remained. Verify current implementation and reports.

## Verification

- Not run for this queued task.

## Blockers and dependencies

- Requires agreed target devices, representative model/project fixtures, and timing tolerance.

## Next step

Review existing DiffSinger parity reports and test inventory; turn unresolved cases into explicit acceptance checks.

## Handoff notes

Treat native rendering as the current product direction for neural engines unless a newer architecture decision says otherwise.
