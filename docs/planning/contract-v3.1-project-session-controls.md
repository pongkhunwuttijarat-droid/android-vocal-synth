# CONTRACT v3.1 — Project Session editing controls (additive extension)

**Status:** implementation authorized; Rust, Direct-WASM and native HTTP coverage pass; extension acceptance remains a separate §2.1 gate  
**Base:** accepted logical CONTRACT-MK3 3.0 under its §2.1 additive-extension policy  
**Capability:** `project.controls.v1`  
**Owner:** T014

## 1. Compatibility and authority

This document defines the T014 editing-control extension to the logical 3.0
baseline. It does not rewrite CONTRACT-MK3 3.0 or frozen CONTRACT-v1 §6.
Existing v2 adapters expose these operations only when their capabilities
include `project.controls.v1`; clients check that capability before sending
extension commands. Unsupported hosts reject unknown commands clearly. Rust
remains authoritative for project data, commands, validation,
history, serialization and render invalidation. UI state remains transient.

Every mutation uses the existing Project Session envelope and stable IDs. A
failed command is atomic. No-op commands do not increment revision or history.
Save and render consume the committed Rust session.

## 2. Track and project editing

The extension covers stable-ID track creation, removal and reorder, plus
importing tracks from USTX. Rust mints IDs. Removal deletes the selected track's
parts and rejects removal of the final track. Reorder changes only track order
and the dense `part.track_no` mapping; it preserves every track, part and note
identity. Track import preserves project semantics, rejects conflicting
expression descriptors atomically, and mints identities for imported entities.

Project tempo and time-signature edits are revisioned document commands. A
normal tempo edit changes the tempo map while leaving project ticks unchanged.
`project.preserve_timing` is a separate atomic document command: it changes the
origin tempo to the requested BPM, scales every tempo marker's BPM by the same
ratio, and scales project tick positions by that ratio as well. Tick positions
must scale with tempo to preserve elapsed time. It remaps part
positions/durations, note positions/durations, part expression-curve ticks and
wave-part positions. Time-signature bar positions and millisecond-based note
pitch points remain unchanged. Tick values round to the nearest integer; the
command rejects overflow, tempo-marker collisions or curve-point collisions,
and clamps positive note/part durations to at least one tick. One accepted
command is one undo step.

## 3. Note and curve data

Authoritative note reads include pitch points, vibrato, tuning cents,
phoneme-indexed expressions, phoneme overrides and any committed pronunciation
sidecar value. `note.set_pitch`, `note.set_vibrato` and `note.set_tuning` replace
the complete corresponding value. Tuning is bounded to ±1200 cents. Pitch
points are finite, sorted and strictly increasing after conversion to the
domain's f32 representation; note-relative pitch units follow the Rust domain.

Curve descriptor reads return the Rust project's registered descriptor and the
session revision. Stroke-context reads accept a part and descriptor abbreviation
with an optional curve ID. If that curve has not been created yet, Rust returns
an empty point list and the descriptor's bounds, default and simplification
tolerance. If a curve ID is supplied, it must belong to that part and descriptor.
The client discards responses older than its current revision and submits edits
through the normal command path.

Phoneme expressions use a zero-based rendered-phoneme index, a registered
expression abbreviation and a value in that descriptor's units. Phoneme
overrides also use a zero-based rendered-phoneme index; `offset` is ticks and
preutter/overlap/attack/release deltas are milliseconds. These legacy arrays
remain readable in the authoritative note view. v3.1 does not provide an
indexed write command or bulk selection command.

`note.clear_phoneme_data` explicitly clears both per-phoneme arrays. Until the
resolved-phoneme read and stable target IDs exist, `note.set_lyric` and
`note.set_pronunciation` reject a change while either array is nonempty, with
`validation_error` / `details.reason = phoneme_data_requires_clear`. This
conservative rule prevents both count changes and same-count recomposition from
reattaching data to a different phoneme. Clients may group the clear and the
subsequent lyric/pronunciation edit into one undo step. The clear command takes
only `note_id`, is a no-op when both arrays are empty, returns the canonical
`note.changed` patch plus a tick-bounded render invalidation when changed, and
participates in normal revision and undo/redo history.

Stable-target per-phoneme editing and resolved-phoneme reads move to T028. The
follow-up must derive targets from the canonical universal sequence so targets
survive bank-alias changes, and must reject stale or changed sequence
fingerprints. No UI control should expose indexed writes before that API and
render consumption are implemented.

Track singer references are persistent project state and have a stable-ID
setter. Track color and editor appearance are presentation state; phonemizer /
renderer configuration needs a separate validated command and remains out of
this increment. Mixer FX follow the accepted MK3 3.0/T018 contract. Track
duplication is deferred; note split remains a grouped `part`/`note` operation
and does not need a second semantic command.

## 4. Native and direct-WASM reads

The logical reads are:

| Read | Logical result |
|---|---|
| `expression.descriptor` | `{revision, descriptor}` for one registered abbreviation |
| `part.stroke_context` | `{revision, track_id, part_id, curve_id, abbr, points, descriptor}` |

The native local-server adapter maps these reads to:

- `GET /api/v2/sessions/{id}/descriptors/{abbr}`
- `GET /api/v2/sessions/{id}/parts/{part_id}/stroke-context?abbr={abbr}[&curve_id={id}]`

The Direct-WASM adapter exposes the equivalent session methods. Both adapters
must use the same Rust read implementation and return the same DTO and errors.
The existing route `/api/v2/sessions/{id}/parts/{part_id}/curves/{curve_id}`
continues to return the stored curve by ID.

Both hosts advertise `project.controls.v1` only after the shared Session
implementation is available. Clients discover support through the v2
capabilities response; routes and methods use the same Session reads.

## 5. Current implementation record

The shared Rust Session contains structural track commands,
`project.import_tracks`, tempo/time-signature edits, pitch and vibrato commands,
and curve stroke. This increment adds descriptor and first-stroke context reads
to Session, Direct-WASM and native HTTP; bounded `note.set_tuning` and
authoritative tuning/per-phoneme note-view fields; `note.clear_phoneme_data`;
and atomic `project.preserve_timing`. The `project.controls.v1` capability is
advertised by the shared host capability DTO.

Verification completed: `cargo test -p project-session`,
`cargo test -p engine-wasm --test project_controls_v31`, and
`cargo test -p synth-server --test api_v2_sessions project_controls_v31_reads_are_available_through_native_http`
pass. Descriptor/stroke-context reads are exercised through the common Session
implementation and both adapters. This does not claim browser UI integration or
acceptance of the extension itself.

Stable-target per-phoneme writes remain T028 follow-up work. Wave-part rendering
belongs to T015; mixer FX belong to accepted MK3 3.0/T018/T044 and are not
duplicated here.
