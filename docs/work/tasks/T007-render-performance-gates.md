# T007 — Complete render performance device and stress gates

**Status:** queued  
**Owner:** unassigned  
**Last updated (UTC):** 2026-09-30T05:25:19Z

## Goal

Close the remaining device and stress acceptance checks for render latency,
memory use, cache behavior, and workload scaling.

## Completion criteria

- [ ] Confirm current performance targets and the device matrix.
- [ ] Run representative short and long projects on target devices, including cold/warm cache cases.
- [ ] Measure peak memory and stress behavior under documented concurrency/workload.
- [ ] Save reproducible commands and results in a durable report and update the roadmap.

## Source planning

- [Roadmap MS3.5](../../planning/roadmap/ROADMAP.md)
- [Engine vs OpenUtau performance report](../../planning/engine-vs-openutau-performance.md)

## Repository checkpoint

- Branch/base commit: capture when claimed.
- Relevant working-tree changes: inspect and record when claimed; ownership unknown until checked.
- Durable artifacts: scripts and reports in the repository.

## Progress

- 2026-09-30: Added from roadmap notes that desktop benchmarks/cache checks had evidence while device and stress gates remained. Verify the newer 2026-09-29 performance report and current target criteria.

## Verification

- Not run for this queued task.

## Blockers and dependencies

- Requires access to the target Android device(s) and a stable workload/device matrix.

## Next step

Read the latest performance report, reconcile its findings with MS3.5 targets, and define the exact missing device runs.

## Handoff notes

Keep benchmark inputs, tool versions, device details, commands, and output artifacts together so another harness can reproduce the measurements.
