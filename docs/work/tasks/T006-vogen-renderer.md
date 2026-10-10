# T006 — Implement and validate Vogen renderer

**Status:** queued  
**Owner:** unassigned  
**Last updated (UTC):** 2026-09-30T05:25:19Z

## Goal

Integrate the Vogen `f0_man` and VSVocoder path into the engine and establish
repeatable correctness and product acceptance.

## Completion criteria

- [ ] Confirm model format, licensing/provenance, runtime, and target hardware requirements.
- [ ] Implement engine integration and the required input/output adaptation.
- [ ] Add representative tests or golden comparisons for timing and audio output.
- [ ] Document device performance, memory limits, and supported configuration.

## Source planning

- [Roadmap MS3.4](../../planning/roadmap/ROADMAP.md)
- [MS3 milestone](../../planning/milestones/ms3/MS.md)

## Repository checkpoint

- Branch/base commit: capture when claimed.
- Relevant working-tree changes: inspect and record when claimed; ownership unknown until checked.
- Durable artifacts: repository source, fixtures, and reports.

## Progress

- 2026-09-30: Added from roadmap snapshot marking Vogen not started. Check for newer implementation work before treating that as current.

## Verification

- Not run for this queued task.

## Blockers and dependencies

- Model/runtime availability, redistribution terms, and target hardware support need confirmation.

## Next step

Inventory existing Vogen-related code, model artifacts, and upstream requirements; record a feasibility decision before implementation.

## Handoff notes

Keep renderer/model execution behind the engine renderer interface and host adapters.
