# Contract v2 extension proposal — structural tracks, per-note properties, bounded reads

> **Historical proposal:** T014 is now proceeding as the additive logical
> **CONTRACT v3.1 `project.controls.v1` extension** defined in
> [`contract-v3.1-project-session-controls.md`](contract-v3.1-project-session-controls.md),
> under CONTRACT-MK3 §2.1. This file remains the original design/audit record;
> its “unapproved v2” wording below is historical and does not describe the
> current versioning target.

**Status:** proposal for review — **NOT approved, NOT frozen, NOT implemented as contract.**
CONTRACT-v1 §6 stays frozen. This document is the versioned contract-update input
required by [T014](../../work/tasks/T014-project-session-openutau-controls-extension.md).
Date: 2026-10-04.

> Everything below is labelled *proposed*. Where this document says a command
> "exists", read "is proposed to exist". Two things are **already implemented** and
> are called out as such, on the same footing as `note.set_tone`: `note.set_pitch`
> and `note.set_tone` are deliberate unapproved extensions that landed in T004. No
> `note.set_vibrato` is now implemented as an unratified v2-draft3 extension:
> it takes the full `UVibrato` value (length 0 disables it) and exposes it in
> note reads. The other commands here are not implemented. The v2 router answers
> `unsupported_operation` for every `track.add*` / `track.remove*` / `track.reorder*`.

## Summary

Three families of addition to §6, plus the read-model gaps that block them:

| Family | Operations | Status |
|---|---|---|
| Structural tracks | `track.create`, `track.remove`, `track.reorder` | proposed |
| Track configuration | `track.set_singer`, `track.set_phonemizer`, `track.set_color` | proposed |
| Per-note properties | `note.set_vibrato`, `note.set_tuning`, `note.set_phoneme_expression`, `note.set_pitch` | `note.set_pitch` and `note.set_vibrato` **implemented as unratified draft extensions**; rest proposed |
| Bounded reads | `expression.descriptor`, `part.stroke_context`, `project.import_diagnostics` | proposed |
| Project load | BOM-tolerant `.ustx` load | **implemented** (see §7) |

## 1. Why a real multitrack project is the acceptance case

Everything in this proposal is specified against `test-ustx-example/king.ustx`,
a genuine **5-track / 5-part** OpenUtau project, rather than the clone's
single-track fixture. That is deliberate:

- Until 2026-10-04 no real OpenUtau `.ustx` could be opened at all — the UTF-8 BOM
  made `serde_yaml` see two documents. The error message read like a multi-document
  problem; the files contain zero `---` separators. See §7.
- Single-track fixtures cannot distinguish "the session tracks part ownership
  correctly" from "the session always resolves against track 0". A part bound to
  the wrong track still returns a valid-looking part.

`native/crates/project-session/tests/multitrack.rs` now pins: 5 tracks open,
5 distinct stable IDs, each track owns exactly its own part, a track edit by
stable ID changes exactly one track, and save→reopen preserves the shape and the
deterministic IDs.

## 2. Structural tracks

### 2.1 `track.create`

Proposed request:

```json
{"expected_revision": 12,
 "command": {"type": "track.create",
             "name": "Track2",
             "index": 1,
             "with_part": true}}
```

| Field | Rule |
|---|---|
| `name` | required, 1–64 chars after trim. Must be non-empty; uniqueness is **not** required (OpenUtau allows duplicate track names, and identity is by ID, not name). |
| `index` | optional. Absent ⇒ append at the end. Present ⇒ insert at that position, shifting later tracks right. Out of range ⇒ `validation_error` with `field: "index"`, no mutation. |
| `with_part` | optional, default `true`. Creates one empty voice part on the new track. `false` creates a bare track (valid; the Web editor always wants `true`). |

**Semantics.** The Rust side mints the track ID and, when `with_part`, the part ID.
Both come from the Rust UUIDv4 generator (§6.2), never from the host. Defaults
match `UProject`'s constructor: mute/solo `false`, volume `0` dB, pan `0`,
empty singer, empty phonemizer.

**Atomicity.** `track.create` is one command and therefore one undo step. It is
**not** a compound `track.create` + `part.create` group: the compound form would
let a host observe a track with no part, and would produce two changes and two
revisions for one user gesture. If a host needs the bare track, it passes
`with_part: false`.

**Change payload.** `track.added` carrying the new `TrackMetaView` and, when
created, the new `PartView`. `authority: patch` — the payload carries the data, so
a host does not need a follow-up read.

**Render invalidation.** None. An empty track renders silence; no existing audio
changes. `render_invalidated` is omitted rather than sent empty.

### 2.2 `track.remove`

```json
{"expected_revision": 13,
 "command": {"type": "track.remove", "track_id": "…"}}
```

**Semantics.** Removes the track **and every part owned by it**, in one command and
one undo step. A host cannot orphan a part: `part.track_no` is a dense index, so
leaving parts behind would silently re-home them to whichever track took the
index.

Removing the **last** track is rejected (`validation_error`, `field: "track_id"`):
`UProject` is defined with at least one track, and the time axis plus mixer assume
it. To get to a single track, remove the extras.

**Identity.** IDs for the removed track, its parts, notes, curves, expressions and
any committed pronunciations are dropped from the identity table. They are **not**
reusable and **not** resurrected if a track is later created — a fresh ID is
minted.

**Change payload.** `track.removed` with the removed `track_id` and the list of
removed part IDs, so a host holding cached part views can drop them. It does **not**
carry full removed bodies; that would make one gesture expensive on a large
project.

### 2.3 `track.reorder`

§6.4.4 explicitly places track reordering outside frozen v2. This proposal admits
it, with the narrowest semantics that satisfy the Web editor:

```json
{"expected_revision": 14,
 "command": {"type": "track.reorder", "track_id": "…", "to_index": 0}}
```

Moves one track to `to_index` in the resulting order. Only `to_index`; there is no
range or shuffle form. A no-op move (already at `to_index`) returns
`applied: false` with no changes and **does not** bump the revision, matching the
existing no-op rule (§6.9).

**Critical constraint — part ownership must survive.** `part.track_no` is a dense
index into `tracks`, so reordering tracks must renumber the `track_no` of every
part, or every part would attach to the wrong track. That renumbering is internal
and **must not** mint, drop, or reorder any stable ID. A part keeps its part ID, its
note IDs, and its note order across a reorder; only its `track_no` changes.

This is the single most important invariant in this section, and it is what
`multitrack.rs` guards from the other direction (a track edit must change exactly
one track).

## 3. Track configuration

`TrackView`/`TrackMetaView` currently expose id, name, singer reference,
mute/solo, volume, pan. The domain `UTrack` also persists phonemizer, renderer
settings, color, mix FX, and track expressions.

| Field | Proposed operation | Rationale |
|---|---|---|
| singer | `track.set_singer` | Already **implemented** — routed in `project-session/src/v2.rs:1664`, covered by `tests/track_singer.rs` (T013). Listed for completeness. |
| phonemizer | `track.set_phonemizer` | Now backed by real semantics: `phonemizer::PhonemizerKind::from_track` (moved out of the CLI crate on 2026-10-04). The value stored is the **OpenUtau registered key** verbatim, not a Rusted UTAU enum name, so `.ustx` round-trips. |
| color | `track.set_color` | Pure presentation, but persisted in the domain and written to `.ustx`. Presentation-only would mean the file cannot round-trip. |
| renderer settings | **deferred** | Engine-specific; needs its own per-renderer contract. |
| mix FX | **deferred** | T014 section E; sequenced after the track commands. |
| track expressions | **deferred** | Distinct from part-level expression curves, which already work. |

**`track.set_phonemizer` validation** is the interesting part, and it is now
enforceable rather than advisory. The value must be a string the engine can
resolve: either an exact OpenUtau registered key, or a name matching the
fallback rules in `phonemizer::selection`. An unrecognised name is **accepted**
and resolves to `Universal` — the same lenient behaviour OpenUtau has, because a
project must stay openable on a build that lacks a given phonemizer. The response
therefore reports the **resolved** kind:

```json
{"resolved": "japanese", "exact_match": true}
```

so a host can show "this track will use the Japanese VCV path" without
reimplementing the mapping. A host must never resolve the name itself.

## 4. Per-note properties

`UNote` already persists `UPitch`, `UVibrato`, tuning (cents), phoneme expressions,
and phoneme overrides. `NoteView` exposes only id, position, duration, tone, lyric.

### 4.1 `note.set_pitch` — already implemented (unapproved extension)

Replaces the note's whole `UPitch` with a validated curve: sorted, finite `x`,
strictly increasing after `f32` narrowing, first `x >= 0`. Landed in T004 on the
same footing as `note.set_tone`; listed here so the review has the full picture.
`domain::PitchPoint::x` is an **f32** — a client that sends `f64` precision can
produce two points that collapse to one `f32`, and the engine correctly rejects the
whole curve.

### 4.2 `note.set_vibrato`, `note.set_tuning`

Full-value setters, not property patches. A partial patch would need a
merge-into-existing semantics and a "clear" spelling; a whole value has neither
problem and matches `note.set_pitch`.

- `note.set_vibrato` — full `{vibrato: {length, period, depth, in, out, shift, drift, vol_link}}` replacement. `length: 0` disables vibrato while retaining the other values. Bounds follow `UVibrato` (`length 0..100`, `period 5..500`, `depth 5..200`, fades each 0..100 with sum ≤100, `shift 0..100`, `drift` and `vol_link -100..100`). This operation is implemented but remains an unratified v2 draft extension.
- `note.set_tuning` — `{cents: f64}`, bounded to ±1200 (one octave). Out of range ⇒
  `validation_error`, no mutation.

### 4.3 `note.set_phoneme_expression`

Phoneme-indexed, so this depends on T013's committed pronunciation.

Open question for review: **what happens to per-phoneme data when the phoneme
count changes.** Three candidate rules:

1. **Refuse** — committing a different phoneme count while per-phoneme data exists
   is a `validation_error` until the user clears it.
2. **Truncate/pad** — silently keep the overlap.
3. **Clear** — drop the stale data and report it in the change payload.

Recommendation: **(1) refuse**, then (3) as an explicit user action. Silent
truncation (2) is the one that loses a user's edit without telling them, which is
the failure mode AGENTS.md is written against. Review must decide.

### 4.4 `note.split`

The clone's split gesture currently composes two note commands. Proposal: **the
existing group contract (§6.8) is sufficient.** A semantic `note.split` would
eventually be right, but it adds a command whose only value is saving two
round-trips inside one already-atomic group. Deferred; not in this proposal.

## 5. Bounded reads

- **`expression.descriptor`** — `GET …/descriptors/{abbr}` returning the
  `UExpressionDescriptor`, plus `session_curve_descriptor_json(part_id, abbr)`.
  No command or schema change; it belongs to the §6.10 bounded-reads family. The
  Web clone currently hard-codes descriptor metadata in the frontend, which is a
  second source of truth for musical state.
- **`part.stroke_context`** — the native stroke-context route. Precedent exists:
  `curve_stroke_context_json` already exists but requires an existing curve. The
  pitch tool needs context for a note that has **no** curve yet.
- **`project.import_diagnostics`** — after load, return what `after_load` repaired
  (`LoadRepair`) as structured data instead of only `identity` warnings. Relevant
  to multitrack because load-time track repair is exactly where a 5-track project
  can silently lose a part.

## 6. Shared semantics required by all of the above

Each proposed command must define, before implementation:

1. input bounds and validation, with machine-readable rejection fields;
2. **rejected-command atomicity** — no partial mutation;
3. `expected_revision` enforcement and `revision_conflict`;
4. canonical change payload with `authority: patch|invalidate`;
5. ID minting (Rust-only) and ID survival across the operation;
6. revision and undo/redo effects, including no-op handling;
7. group failure behaviour where composed;
8. `render_invalidated` range;
9. serialization/import/export round-trip through `.ustx`;
10. **shared adapter vectors** runnable against reference, native HTTP, and direct
    WASM — the existing pattern is
    `native/crates/project-session/tests/conformance_vectors.rs` plus the shared
    JSON under `native/crates/engine-wasm/tests/`.

Frontend previews during a gesture stay ephemeral (§6.1.2). Committed values,
validation, save, and render all use the session.

## 7. Project load: BOM tolerance (implemented, recorded here)

Not a §6 change — a defect fix in the domain — but it belongs in this record
because it is what made multitrack acceptance possible.

Every `.ustx` OpenUtau writes starts with a UTF-8 BOM. `ref-openutau-clean/core/
OpenUtau.Core/Format/USTx.cs:102` saves with `File.WriteAllText(..., Encoding.UTF8)`,
and .NET's `Encoding.UTF8` emits a BOM preamble. `serde_yaml` does not skip it, so
the first key becomes unreadable and the document parses as **two**:

```text
deserializing from YAML containing more than one document is not supported
```

That message misdirects: the fixtures contain **zero** `---` separators. Verified —
`king.ustx` has 0 document markers and still failed exactly this way. Stripping the
BOM fixes it, and the project loads with all **5 tracks / 5 parts**.

`UProject::parse_yaml` is now the canonical BOM-tolerant loader;
`UProject::from_yaml_text` is the same parse without the `after_load` repair step,
for callers that need the `LoadRepair` warnings and must run the repair exactly
once. `pipeline::parse_project`, `ProjectDoc::from_yaml`, and
`Session::open_with_pronunciation_sidecar` all route through it.

**Remaining gap:** other call sites still call `serde_yaml::from_str` on project
text directly (notably `ustx-codec`'s `UstxDecode` FFI entry). Those are not
covered by this fix.

## 8. What this proposal does NOT change

- CONTRACT-v1 §6 remains frozen and unmodified.
- No endpoint, schema, or capability claim is presented as live.
- §6.1 ownership is unchanged: Rust owns all of this state.
- The composition draft in `phoneme-compose-contract-proposal.md` is a **separate**
  proposal. Review both together; neither depends on the other's approval.

## 9. Review checklist

- [ ] Approve/reject each family independently (§2, §3, §4, §5).
- [ ] Decide the §4.3 phoneme-count-change rule — recommendation is refuse, then
      explicit clear.
- [ ] Confirm `track.remove` deleting its parts is the intended semantics.
- [ ] Confirm track reorder renumbers `part.track_no` without touching stable IDs.
- [ ] Confirm `track.set_phonemizer` stores the OpenUtau key verbatim.
- [ ] Decide whether `note.set_pitch` / `note.set_tone` are ratified into §6 or
      remain labelled unapproved extensions.
