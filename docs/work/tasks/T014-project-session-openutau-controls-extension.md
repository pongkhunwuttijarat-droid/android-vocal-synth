# T014 — Project Session extension for OpenUtau note and track controls

**Status:** blocked — implementation and adapter tests pass; waiting for separate user acceptance of the v3.1 extension under MK3 §2.1  
**Owner:** Codex (implementation, user-authorized 2026-10-15)  
**Last updated (UTC):** 2026-10-15T05:18:49Z

**Historical status reconciliation:** The central Taskboard recorded `active`
with Codebuff ownership at `2026-10-04T06:55:00Z`, later than this record’s
`queued` snapshot (`2026-10-03T09:38:54Z`). Current ownership and authorization
are recorded in the progress updates below.

## Progress update — 2026-10-15T02:30:06Z

The user confirmed the T014 remainder may proceed and suggested v3.1; the current
contract work uses that as an additive extension to MK3 3.0. Added
`project.preserve_timing` as one atomic document command. It scales the tempo map
and every tick-based part/note/curve position inversely to the tempo change,
while keeping note pitch points in milliseconds unchanged. Overflow and
rounding collisions reject the cloned plan before it replaces the live project.

Static source review found the following T014 work already implemented in the
dirty shared Rust tree before this increment: track create/remove/reorder,
`project.import_tracks`, ordinary tempo/time-signature edits, note pitch and
vibrato, curve strokes, and wave-part mixdown. T015 owns wave-part behavior and
T045/T018 own the mixer FX contract. No ownership is inferred for the remaining
pre-existing changes beyond the T014 review the user assigned here.

Implementation added in this increment: revision-tagged expression descriptor
and first-stroke context reads in `project-session`, direct-WASM wrappers, and
native `/api/v2` routes; note tuning mutation and authoritative note-view data
for tuning, phoneme expressions and overrides; and `project.preserve_timing`.
`git diff --check` passed. No Rust test or build was run; static review is not
host-parity evidence.

At this checkpoint (02:30Z), the next work was capability/adapter conformance,
legacy proposal reconciliation, and a resolved-phoneme API design. The later
02:41Z/02:45Z updates below close the first two items and hand the stable-target
resolved-phoneme API to T028.

## Progress update — 2026-10-15T02:41:23Z

Continued the user-authorized v3.1 increment. Corrected a planner-routing regression
that had intercepted all `track.*`/`part.*`/`project.*` commands and returned
`unsupported_operation` for already-supported track mixer and part operations;
the document planner now intercepts only its explicit command set. Document note
patches now include their affected tick range.

Corrected Preserve Timing: tick positions scale by `new_bpm / old_bpm` (the same
ratio as tempo markers), which keeps elapsed time constant; the previous inverse
ratio was wrong. Added note tuning bounds/read/undo coverage, Preserve Timing
scaling and undo/redo coverage, descriptor and first-stroke context coverage, and
phoneme-data retarget protection. `note.clear_phoneme_data` is the explicit
clear path; lyric/pronunciation changes reject while dependent arrays remain.
Both native HTTP and Direct-WASM now advertise `project.controls.v1` and expose
the same descriptor/stroke-context reads.

Verification: `cargo test -p project-session` passes; the Direct-WASM test
`cargo test -p engine-wasm --test project_controls_v31` passes; the native HTTP
read test `cargo test -p synth-server --test api_v2_sessions
project_controls_v31_reads_are_available_through_native_http` passes. The
remaining T014 follow-up is formal v3.1 acceptance/review and the stable-target
resolved-phoneme API, assigned to T028; UI wiring remains in T004.

## Progress update — 2026-10-15T02:45:05Z

Final focused verification after the Preserve Timing correction and phoneme-data
guard: `cargo test -p project-session` passes (103 tests across integration
targets); `cargo test -p engine-wasm --test project_controls_v31` passes; native
HTTP descriptor/stroke-context and capability tests both pass. `git diff
--check` passes. `cargo fmt --all -- --check` is not clean across this heavily
dirty checkout and reports formatting differences in many unrelated files; no
workspace-wide formatting was applied. No T004/T027 UI implementation was
started. T014 remains active for formal extension review/acceptance; stable
resolved-phoneme reads/writes remain a T028 follow-up.

## Current handoff — 2026-10-15T02:24:39Z

The user authorized continuing the remaining T014 work and selected **logical
CONTRACT v3.1** as an additive `project.controls.v1` extension to the accepted
MK3 3.0 baseline. Do not edit frozen CONTRACT-v1 §6 or rewrite the MK3 3.0
baseline. The extension is documented in
[`contract-v3.1-project-session-controls.md`](../../planning/contract-v3.1-project-session-controls.md);
the earlier v2 proposal below remains historical design/audit input.

Current source inspection found that much of the old “Still owed” list is
already present in the shared Session working tree: stable-ID track create,
remove and reorder; USTX track import; tempo/time-signature edits; note pitch
and vibrato; and whole-project wave mixing are implemented in existing Rust
paths. T045 owns the separate mixer FX contract. This T014 increment added the
revision-tagged expression-descriptor and first-stroke context reads to the
shared Session, their Direct-WASM wrappers, and native HTTP routes, plus the
bounded note-tuning command and authoritative tuning/phoneme-data fields in
`NoteView`.

The workspace already had extensive dirty/untracked Rust and Web changes before
this increment. Do not attribute those existing changes to T014 without an
ownership audit. Files intentionally edited in this increment: this task
record, the T014 v2 proposal history, the new v3.1 extension, `project-session`
session/command/view paths, `engine-wasm/src/lib.rs`, and the clean native
`synth-server` handler/router paths. Branch is `wasm-native-rust`, HEAD at
start was `e6af244abbf04d4a490fca8483295ed6418e6cad`.

Not complete: stable-target phoneme/alias/timing writes and their
resolved-phoneme read; UI integration; host conformance/verification; review of
the phoneme-count-change refusal rule; and reconciliation of the old task's
stale completion list. `project.preserve_timing` was added after the initial
handoff note above; its semantics are recorded in the v3.1 extension and it has
not been tested. No tests/builds were run in this increment.

## Goal

Prepare a reviewable, versioned Project Session contract extension for the OpenUtau-style
Web editor controls that are still mock/local-only. Identify the smallest Rust-owned command
and read-model additions needed for structural track editing and per-note properties, define
their cross-host semantics, and hand the proposal to contract review before implementation.

Do not silently change frozen CONTRACT-v1 §6 or represent mock controls as live functionality.

## Completion criteria

- [ ] Write a versioned follow-on contract proposal through the §6 versioning/review process;
      preserve the frozen v2 baseline and label all proposals as unapproved until reviewed.
- [ ] Specify stable-ID track create/remove/reorder semantics, defaults, ordering, identity,
      errors, changes, revision/history, and atomicity. Decide whether duplicate/settings
      operations are in scope or composed from primitive commands.
- [ ] Specify authoritative note reads and commands for the OpenUtau note controls selected
      for the first release: tuning, custom pitch/portamento data, vibrato, and phoneme-indexed
      expressions/overrides. Define units, validation, defaults, selection/bulk behavior,
      and how phoneme indexing interacts with T013.
- [ ] Separate persistent track configuration from presentation-only controls; record whether
      singer/phonemizer/renderer selection, track color, and FX are in scope or deferred.
- [ ] Define canonical change payloads, bounded reads, grouped undo behavior, render
      invalidation ranges, save/export round-trip, and shared command vectors for native HTTP
      and direct WASM adapters.
- [ ] Update the OpenUtau control inventory and T004 handoff with the reviewed contract scope
      and dependency before enabling any currently disabled live-session control.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`
- Relevant working-tree changes: the checkout contains extensive modified and untracked work
  from concurrent tasks. Ownership is unknown except for the T004 UI work already described
  in [`T004`](T004-web-editor-product-flow.md), plus the T013 dependency/IME requirements
  drafted in this record and the record/board edits owned here. Do not claim the existing
  contract, Rust, or UI edits as T014 implementation.
- Durable artifacts: `docs/CONTRACT-v1.md` §6 (frozen baseline),
  `docs/planning/openutau-control-inventory.md`, this record, and T004.

## Progress

- 2026-10-03T09:38:54Z: T013 backend/session acceptance is complete. Retain the deferred UI IME
  requirements below as an unapproved draft dependency in this T014 record: composition text and
  popup state stay ephemeral; candidates/provenance and validation come from Rust; keyboard
  selection commits a canonical choice by stable note ID; lyric + pronunciation group as one undo
  step; stale revisions are discarded; voicebank changes refresh aliases; save/render use session
  state. No editor UI work has started. Merge this draft with the structural/per-note gaps during
  T014's contract review while leaving CONTRACT-v1 §6 frozen.

- 2026-10-03T09:22:20Z: T013 replaced its earlier temporary-hint render bridge with native and
  WASM paths that pass stable-ID-aligned committed choices to the shared render pipeline. The
  pipeline recomposes the selected strategy/style against the active voicebank; Japanese VCV
  retains preceding-note context. Build checks pass, but render behavior is not yet runtime-tested
  and other styles still need neighbor-boundary coverage. Keep UI IME deferred and retain these
  render/read requirements in the T014 versioned extension proposal.

- 2026-10-03T09:07:10Z: T013 now has a working-tree session sidecar, stable-ID pronunciation
  commands/views, native/WASM compose entry points, and a session-derived render snapshot. The
  render bridge currently passes committed language phones as a temporary phonetic hint; exact
  strategy/style and neighboring-note alias semantics remain open. Invalid pronunciation
  sidecars now emit `pronunciation_sidecar_ignored` from the session warning channel. Expanded
  the merge draft to formalize warning details, style-aware render rederivation, cross-adapter
  vectors, and per-phoneme override stability. UI IME remains draft only and was not started.

- 2026-10-03T08:44:00Z: Added a T013 dependency draft covering stable-ID composition reads,
  pronunciation commit/clear semantics, sidecar lifecycle, render-time alias derivation, and
  per-phoneme-expression interaction. Added a deferred UI IME requirements checklist per user
  direction; no frontend implementation began. Merge this into the versioned T014 proposal
  before contract review.

- 2026-10-03T04:26:50Z: Audited the frozen §6 command catalogue, session read DTOs, domain
  note/track models, and the OpenUtau clone controls. Confirmed that part expression curves
  already use `curve.apply_stroke`; the gaps below concern different state and should not
  duplicate that API. No implementation or tests were run.
- 2026-10-03T05:33:29Z: Re-verified the inventory against the current source: the v2 router
  returns `unsupported_operation` for `track.add/remove/reorder`; no descriptor read or
  descriptor command exists; HTTP has no stroke-context route; `curve_stroke_context_json`
  still requires an existing curve. Added the verified gap inventory, the new read-model gaps,
  and minimal read-only proposals. No contract text changed; no implementation or tests for the
  proposal.

## Contract gap inventory

### T013 dependency: note pronunciation and phoneme authoring

T013 owns the Rust semantics for composing and committing a note's pronunciation.
T014 must include the session/model requirements below in its versioned extension proposal
before any connected editor controls or per-phoneme properties are enabled. CONTRACT-v1 §6
remains frozen. The concrete DTO draft stays in
[`phoneme-compose-contract-proposal.md`](../../planning/phoneme-compose-contract-proposal.md).

- A revision-tagged `phoneme.compose` read uses stable project/part/note IDs, the current
  lyric or explicit input, tone, selected voicebank, language/style/mode, and a bounded result.
  It returns candidate provenance, language phones, universal phones, bank aliases, and
  unresolved diagnostics; a transient candidate ID is never stored.
- A `note.set_pronunciation` command commits the canonical universal phone sequence plus
  language, mode, and resolved style. Clearing the choice restores ordinary lyric/hint
  phonemization. Both operations are validated in Rust and participate in revision/history
  semantics. Changing voicebank recomputes bank aliases from the committed universal sequence.
- The stored choice is keyed by Rust-minted stable note ID in a versioned session sidecar, not
  by note index and not in frontend project state. Reopen and Save As retain the choices while
  keeping `.ustx` interoperability; undo/redo, note deletion, import replacement, and ID repair
  need explicit lifecycle semantics in the proposal. Missing sidecars are normal; malformed,
  wrong-project, or stale choices currently emit `pronunciation_sidecar_ignored` while leaving
  USTX openable. Specify warning counts and whether the warning should distinguish the reason.
- Render planning consumes the committed choice from the Rust session and re-derives the chosen
  strategy/style against the selected voicebank and neighboring-note context. The adapter reports
  unresolved phones and never falls back to a stale frontend phoneme list; saved USTX stays
  unchanged because the render snapshot is disposable.
- Per-phoneme expressions/overrides in T014 depend on this committed sequence. Define whether
  an expression index follows the current rendered alias sequence and how a pronunciation
  change invalidates or remaps existing per-phoneme data before exposing those controls.

### Deferred UI IME requirements (draft only; no frontend implementation started)

- Keep only raw composition text, popup visibility, highlighted row, and keyboard focus in the
  UI. Request candidate meanings from the shared engine/session API; do not reproduce language,
  phoneme, style, alias, or validation logic in Flutter/Web.
- Support Up/Down candidate navigation, Enter to confirm, Escape to dismiss, and a visible way
  to switch universal vs bank-native display. Confirmation sends stable IDs and the selected
  canonical pronunciation; never send a candidate list index as persisted meaning.
- Commit a lyric edit and its selected pronunciation atomically as one grouped undo step. A
  canceled popup leaves the committed project unchanged. Revision-stale candidate responses
  are discarded, and the UI refreshes the authoritative result after commit/undo/redo.
- Present strategy/provenance and unresolved aliases to the user, refresh alias previews when
  the voicebank changes, and keep selection/viewport/layout ephemeral.
- Save and render always use the Rust session state, including when the user switches hosts or
  reopens a project. This UI slice is explicitly deferred until T013 session/native/WASM
  semantics pass their shared conformance gate.

### Structural tracks

- The frozen v2 stable-ID command catalogue has `track.rename`, `track.set_mute`,
  `track.set_solo`, `track.set_volume_db`, and `track.set_pan`; it has no `track.create`,
  `track.remove`, or `track.reorder`. §6.4.3 already names `track.added`/`track.removed`
  changes, but that vocabulary alone does not define commands or atomic semantics.
- `TrackView`/`TrackMetaView` are limited to id, name, singer reference, mute/solo, volume,
  and pan. The domain track also has persistent configuration such as phonemizer,
  renderer settings, track color, mix FX, and track expressions; decide field-by-field whether
  those belong in this extension or later work.
- The clone's Add Track path synthesizes a track and voice part in frontend mock state. Specify
  whether this is a single compound operation or one group containing `track.create` and
  `part.create`, including rollback, defaults, minted IDs, and one-undo behavior. Define what
  removing a track does to its parts and identity sidecar, plus canonical ordering/reorder
  semantics. §6.4.4 explicitly leaves track reordering outside frozen v2.

### Per-note properties

- Existing stable-ID note commands cover add/remove, lyric/tone, move, and resize. Current
  `NoteView` exposes only id, position/duration ticks, tone, and lyric. `note.add` does not
  accept the additional domain properties.
- The Rust domain `UNote` already persists `UPitch`, `UVibrato`, tuning (cents), phoneme
  expressions, and phoneme overrides. The clone currently puts vibrato patches in local
  reducer state; tuning is a disabled placeholder; Portamento and Expressions panels use
  local mock values. Design typed operations and complete authoritative reads rather than
  frontend patch bags. At minimum decide whether to expose full-value setters
  (`note.set_pitch`, `note.set_vibrato`, `note.set_tuning`, and a typed phoneme-expression
  operation) or a narrowly specified property command.
- OpenUtau portamento is represented by custom pitch data, not necessarily an independent
  stored preset/length/start/shape primitive. Define pitch-point units and interpolation,
  note-relative timing, endpoint behavior, and what resizing/moving a note does to that data;
  do not canonize the mock panel's fields without mapping them to domain semantics.
- `UExpression` entries are phoneme-indexed and depend on phonemization. Coordinate index
  stability, phoneme-count changes, override behavior, and selection with T013. Keep these
  distinct from the already-supported part-level expression curves/descriptors.
- The clone's split gesture currently composes two note commands. Decide whether the existing
  group contract is sufficient or whether a semantic `note.split` belongs in this proposal.

### Shared semantics required for every addition

Specify input bounds/validation, rejected-command atomicity, expected revision, canonical
change/read shape, ID minting, revision and undo/redo effects, group failure behavior,
render-invalidated range, serialization/import/export round-trip, and shared adapter vectors.
The UI may preview values during a gesture, but committed values, validation, save, and render
must all use the Rust session. Do not add duplicate APIs for existing part curve editing.

## Verified gap inventory (2026-10-03T05:33:29Z, Codebuff)

Method: read the frozen §6.3.1/§6.4.3/§6.4.4/§6.10/§6.20 contract, the `project-session` command
router (`src/v2.rs`), the session reads (`src/session.rs`), the native routes
(`synth-server/src/handlers_v2.rs`), and the clone's live-session controls; then ran the clone
against a live `synth-server` on `127.0.0.1:8765`. This refines and confirms the inventory above.

### A. Command gaps — the router answers `unsupported_operation`

- **Structural tracks.** `v2.rs` explicitly matches `track.add*`, `track.remove*` and
  `track.reorder*` and returns `unsupported_operation` (v2.rs:1581-1586). §6.4.4 also declares
  track reordering out of v2 scope. §6.4.3 reserves the `track.added`/`track.removed` change
  kinds but no command emits them. Blocks the clone's Add Track / delete / reorder / duplicate.
- **Track configuration.** No command and no view field for singer/phonemizer/renderer
  selection, track color, mix FX, or track-level expressions; `TrackView` is id/name/singer
  reference/mute/solo/volume/pan only. Voicebank-reference edits are explicitly outside v2.
- **Per-note authoring beyond position/duration/tone/lyric.** No `note.set_tuning`,
  `note.set_vibrato`, `note.set_pitch` (UPitch / portamento), and no per-note
  phoneme-expression/override command. The domain persists all of these; §6.20's `note.add`
  accepts none and `NoteView` exposes none, so the clone's tuning placeholder and its local
  vibrato/portamento fields have no authoritative home.
- **Expression-descriptor editing.** The domain has `SetExpDescriptorCommand`, but no v2
  descriptor command is routed, so descriptor add/edit/remove cannot be committed. The Project
  Expressions dialog is a disabled draft for exactly this reason.
- **Clipboard.** No copy/cut/paste command, so paste identity rules (fresh IDs vs preserved) are
  undefined; the clone has no paste.
- **MIDI import.** `Source` offers only `new`, `stored_project` and `imported_ustx`; there is no
  MIDI source or parser. `imported_ustx` covers USTX import at the logical level (mints IDs),
  but the adapter wiring is T004.
- **`note.split`** — not required for the first release: the clone's split is already one group
  of `note.resize` + `note.add` and undoes as one step. Record the decision explicitly rather
  than adding a command.

### B. Read-model gaps — the clone hits these today

- **No expression-descriptor read.** Neither the Snapshot nor `ProjectMetaView` (§6.3.1) exposes
  the `expressions` registry, and §6.10's bounded-read table has no descriptor/expressions read.
  The engine's `project.expressions` (name, type, min, max, default, options, colour) is therefore
  unreachable over the logical API, so the clone hardcodes the seven §6.6.1 curve descriptors in
  `mock-data.js`. A project whose descriptor is custom or retuned cannot be displayed correctly.
- **No stroke-context read for a curve that does not exist yet.**
  `Session::curve_stroke_context_json(part_id, curve_id)` (session.rs:701) requires an existing
  `curve_id`, so a client cannot obtain min/max/default/tolerance to create the *first* curve from
  a stroke even though `curve.apply_stroke` with `curve_id: null` creates one. WASM exposes it
  (`session_curve_stroke_context_json`); **HTTP does not** — `handlers_v2.rs` has no
  stroke-context route — so HTTP-backend curve editing falls back to the frontend's static table.
- **Minimal proposed additions (no command or schema change):** a §6.10 bounded read for the
  project's descriptors (`GET …/project/descriptors`, envelope `{revision, expressions[]}`) plus a
  WASM binding; and either allow `curve_stroke_context_json(part_id, curve_id: null, abbr)` or add
  `GET …/parts/{part_id}/curves/{curve_id}/stroke-context` so both adapters can serve existing
  curves too. These are read-only extensions and keep frozen §6 semantics intact.

### C. Already covered — do not duplicate

The `part.*` and `project.*` commands, `curve.apply_stroke`, and the stable-ID note/track commands
all exist and are routed; part-level expression curves are the supported editing path and the
clone already uses them. The gaps above concern different state (structural tracks, per-note
properties, descriptors) and must not add a second API for what already works.

## Verification

- Read-only audit of `docs/CONTRACT-v1.md`, `native/crates/project-session/src/views.rs`,
  `native/crates/project-session/src/v2.rs`, `native/crates/session.rs`, `synth-server/src/handlers_v2.rs`,
  `native/crates/domain/src/note.rs`, and `ui-mock/react-dock/src/openutau/` — completed 2026-10-03.
- Live confirmation of the descriptor gap: the clone's expression lanes draw from the static
  `mock-data.js` table while the engine serves canonical curves (observed in the live session).
- Tests/builds — not run for the contract findings; this record is analysis plus a handoff.
- Future verification after review/implementation: shared command vectors through the
  project-session reference path, native HTTP, and direct WASM; grouped undo/redo and failure
  atomicity; canonical save/reopen/export round-trip; and render invalidation for changed note
  properties. Exact commands should be recorded by the implementing task.

### Import gaps added by the OpenUtau File-menu alignment — 2026-10-03T07:41:02Z

Decision taken with the owner: **no MIDI import**. OpenUtau has none (`desktop-ref` has
`Format/MidiWriter.cs`, export-only, and its File menu imports *tracks* and *audio*), and §6.11
(frozen) exposes only `new` / `stored_project` / `imported_ustx`. Writing a MIDI parser in the UI
would be a second project model, so the File menu now carries OpenUtau's own import entries and
each names the engine work it needs. Two new gaps, both read-only descriptions of what is missing:

- **`Import Tracks…` (`menu.file.importtracks`)** — adopting another project's parts into the open
document. v2 can `part.create` on an existing track but has no command that adopts a whole track's
contents (parts, notes, curves) as one operation, so a merge cannot be expressed without inventing
frontend merge rules. Needs a command plus a defined collision/identity policy for the adopted
tracks.
- **`Import Audio…` (`menu.file.importaudio`)** — `part.create` already accepts `kind: "wave"` with
a validated project-relative `media_ref` (`v2.rs` ~1075), so the *part* is creatable today. It is
held back because the phrase/render pipeline never mixes wave parts (no `UPart::Wave` reference in
`phrase`/`feed`/`runtime`/`render_service.rs`), so an imported wave part would be silent. Needs
wave-part mixing in the render path (and a host-side home for the bytes; the reference is already
project-relative by design).

Frozen §6 was not changed; these are proposals for the same review process as the rest of this
record.

## Blockers and dependencies

- CONTRACT-v1 §6 says the Project Session logical standard is frozen as of 2026-09-30. The
  proposal must go through the documented versioning/review process; do not edit §6 in place
  or imply v2 support is live.
- T013 is active and owns phoneme input/mapping. Per-phoneme expression semantics depend on
  its stable phoneme/candidate/index model; coordinate before freezing that subsection.
- T004 may continue UI work only for controls already backed by commands. Keep the affected
  mock controls disabled in connected sessions until the new contract is reviewed and the
  corresponding Rust/adapters are implemented.

### D. Musical operations moved here from T004 (2026-10-03, owner direction)

The clone's **Project → Adjust Tempo…** and **Preserve Timing** dialogs were shipped as
presentation drafts with Apply disabled. They are musical operations over the whole document, so
they belong to this contract extension rather than to UI work. Recording the requirement here
instead of implementing it in T004:

- **Adjust tempo** must be one atomic Rust command over the frozen §6 command set. The domain
  already persists `bpm`; §6.4.4 exposes `project.set_tempo`, but the OpenUtau control is more
  than a number field — the owner chose whether a tempo change retimes notes, phrases and part
  positions, or leaves ticks alone and only changes playback rate. That decision changes what the
  command *means*, so it is a contract question, not a UI question.
- **Preserve Timing** (OpenUtau: change the tempo while every note keeps its real-world duration,
  which MOVES tick positions) is a distinct operation from Adjust Tempo and needs its own command
  with its own atomicity, revision and render-invalidation rules. It cannot be expressed as a
  sequence of `note.move` commands assembled in a host without making the host the authority for a
  document-wide musical transformation.
- Until both are specified and implemented, the two dialogs stay UI drafts with Apply disabled.
  T004 must not compute either transformation in JavaScript under any circumstances.

### E. Mixer / FX add-on effects (planned, not started — 2026-10-03, owner direction)

Recorded as a plan item because it is not built and not yet specified. No UI, command or schema
change has been made for it.

- OpenUtau's track card carries an FX button and the mixer carries effect slots with bypass,
  wet/dry and parameter automation. Nothing equivalent exists here: `TrackView` is
  id/name/singer/mute/solo/volume/pan, and the only mixer fields are those two faders. `crates/mixer-fx`
  exists in the workspace but is not reachable through the project session, so FX state would
  otherwise become frontend-owned the moment a UI appears for it.
- Enabling this needs, at minimum: a persistent track-level effect chain in Rust (identity,
  ordering, parameters, bypass, and which parameters are automatable), stable IDs for chain
  entries, save/export round-trip through USTX, render invalidation when the chain changes, and a
  decision on whether effect parameters may be expression-driven (which would make them curves
  under §6.6 rather than scalars).
- Sequencing recommendation: after the structural-track commands, not before. FX on tracks that
  cannot be created, removed or reordered is not reachable, and the mixer surface should follow
  the track commands rather than race them.

## Next step

Merge the T013 dependency and deferred IME requirements above with the versioned command and
read-model proposal, including the tempo operations (D) and the FX/mixer add-on plan (E). Review
the combined draft before changing the frozen baseline or enabling connected UI controls.

Immediate sequencing chosen by the owner on 2026-10-03: **curve editing first.** The expression
stroke path was simplified and made faithful before any other T014 item — see T004 for what that
changed and what is still owed here.

## Handoff notes

The OpenUtau clone is the user's preferred Web editor UI. This record focuses on its controls
that are still mock or unavailable in a connected Rust session; it is not a request to replace
the UI or to implement these commands immediately. Existing supported lyric/tone/move/resize,
track mixer/rename, and part curve-stroke paths should be preserved and reused.

## 2026-10-03T09:20Z — two reported gaps that are NOT contract gaps

The multitrack-import report ("notes disappear, audio track disappears") was
investigated against the user's own `golden-the-end-of-hutsune-miku.ustx` and is a
**viewport** defect, not a missing command: the engine imported all 5 tracks / 1709
notes through the real dialog, and the piano roll simply sat at bar 1 while the part
began at tick 205200. Fixed in the UI (`useFollowPart` + `clampScrollX`); no contract
change needed. Recorded here so the next contract pass does not go looking for a
`track.*` command that is not the problem.

Singer install now accepts `.zip` (`web/zip.js`, `DecompressionStream('deflate-raw')`).
That is host-side storage only — no session command, and no read-model gap.

Still open from the earlier inventory, unchanged by this pass:

- `Import Tracks…` — no command adopts another project's parts (parts + notes + curves)
  into the open document as one undoable transaction.
- `Import Audio…` — `part.create` already accepts `kind: "wave"` with a validated
  project-relative `media_ref`, but no `UPart::Wave` reference exists anywhere in
  `phrase`/`feed`/`runtime`/`render_service.rs`, so a wave part is silent. The user's
  Hatsune file has exactly one (an OFFVocal mp3 on track 5): it imports and draws in
  the arrangement, but contributes nothing to the render.
- Per-track singer selection: `TrackView.singer_id` is already in the frozen §6
  snapshot and imports correctly, but there is no command to change it, so the editor
  offers no per-track singer picker.

## 2026-10-03T09:35Z — design for the two reported engine gaps (recorded, not built)

Per the user's decision these two are **design only** for now; no Rust was changed
for either. Both need a new frozen-contract surface, so they go through the §6
versioning/review process rather than being slipped into the baseline.

### A. Per-track singer selection

**Verified gap.** `grep singer native/crates/project-session/src/v2.rs` returns
nothing: there is no command that writes `singer_id`. `v2.rs` has
`track.rename` (1629), `track.set_mute` / `track.set_solo` (1581),
`track.set_volume_db` / `track.set_pan` (1605) and `track.add` / `track.remove` /
`track.reorder` (1655–1657, all `unsupported_operation`). The *read* side is already
frozen and works — `TrackView.singer_id: Option<String>` is in the §6 snapshot, and
the user's Hatsune file imports four tracks with
`重音テト OU用日本語統合ライブラリー` intact — so this is a write-side gap only, and the
editor simply has nothing to bind a picker to.

Proposed command, mirroring the existing `track.set_*` shape:

```
{ "type": "track.set_singer", "track_id": "<stable id>",
  "singer_id": "<string>" | null }
```

- `null` clears the singer (a track with no bank, which is what the engine already
  reports for an audio track); a non-empty string sets it.
- Unknown `track_id` → `ERR_NOT_FOUND`; unknown command → `ERR_UNSUPPORTED`, so an
  older host degrades to "not available" rather than silently dropping the edit.
- One undo transaction, like every other `track.set_*`.
- Validation question for review: is an arbitrary string acceptable as a singer id,
  or must it resolve against an installed bank? OpenUtau stores a free-form singer
  name on the track and validates at render time, which argues for free-form here
  and validation in the render planner — but that is a contract decision, not a UI one.
- Host side: `DirectWasmBackend.apply` already forwards arbitrary commands, so the
  only host work is the dropdown, fed by the bank roots the host already lists
  (`OpfsBank.installedRoots`, which `Tools → Singers…` uses) plus an explicit
  "no singer" entry. No frontend-owned project state is involved.

### B. Audio (wave) parts never reach the render

**Verified gap.** `UPart::Wave` appears only in `commands/` (part/track/curve
commands and their tests). There is **no** reference in `phrase/`, `feed`, `runtime`
or `render_service.rs`, so a wave part round-trips through the session, draws in the
arrangement, and contributes nothing to the render. `part.create` already accepts
`kind: "wave"` with a validated project-relative `media_ref` (v2 ≈ 1075), so the
document half is done and only the render half is missing. The user's Hatsune file
has exactly one wave part (an OFFVocal mp3 on track 5) and is silent because of it.

This is engine work, not a contract addition: the command already exists, so the
change is a render-path one. Shape of it:

1. `phrase` must emit something for a `UPart::Wave` — today it presumably iterates
   voice parts only. A wave part needs position/duration/trim/fadein/fadeout from
   the file (all present in the decoded `UWavePart`) and no phonemisation at all, so
   it should bypass the phrase/synthesis stage entirely rather than be forced
   through it.
2. Decoding the media (mp3/wav/ogg/opus) into PCM belongs in `feed`/`runtime`; the
   render service then mixes it into the track buffer. `media_ref` is already
   validated as project-relative, which is what makes this safe to load.
3. Sampling rate and channel count must be reconciled with the synth output before
   mixing, or the audio track will play at the wrong speed/pitch.
4. Mute/solo/volume for the owning track must apply to the wave part like any other
   part — the mixer already handles per-track fader/pan (`pool.setTrackMix`), so this
   should fall out if the wave part lands on the same track buffer.
5. A missing or unreadable `media_ref` (the user's file points at a path that does
   not exist on this machine) must be a reported, non-fatal condition — the rest of
   the song still renders — rather than a panic or a silent full-song failure.

Items 3 and 5 are the ones most likely to need contract-visible reporting (what the
host is told about a part that could not be mixed), so they belong in the same review
as A rather than being settled in the render code alone.

## 2026-10-03T09:45Z — `track.set_singer` implemented (was section A above)

Per the user's "can you just do it first", section A is now **built**, not just
designed. The read side was already frozen, so this is the write half only.

**Rust**
- `commands/src/track_commands.rs`: `SetTrackSingerCommand` (`new(project, index,
  Option<String>)`), mirroring `RenameTrackCommand`/`SetTrackSoloCommand` —
  snapshot the old track, replace, and locate by value equality on undo.
  Exported from `commands/src/lib.rs`.
- `project-session/src/lib.rs`: legacy dispatch `"set_track_singer"` (string **or
  null; null clears**) and the undo label `"Track singer"`.
- `project-session/src/v2.rs`: the `"track.set_singer"` plan, stable-ID addressed
  via `locate_track`, emitting `track.changed` + `render.invalidated`. A blank or
  whitespace-only name normalises to `None` so the document never carries a
  singer that renders as an empty-but-named row.
- **§6 itself was not edited.** The frozen baseline is untouched; this adds a
  command the review still has to ratify.

**Tests** — `project-session/tests/track_singer.rs`, 7 cases: set + change names
the track; undo/redo as one transaction; `null` clears; blank normalises; two
tracks stay independent (needs its own 2-track fixture — `DEMO` has one track);
a rejected change (wrong type → `validation_error`, unknown id → `not_found`)
leaves yaml, ids and revision untouched; and the singer survives a yaml round
trip. All 7 pass.

**Host**
- `engine.js`: `track/singer` → `{type:"track.set_singer", track_id, singer_id}`.
- `clone-state.jsx`: mock fallback mirroring the engine's blank→null rule.
- `arrangement-panel.jsx`: `SingerPicker` on each track card. It is a **combobox**
  (`input` + `datalist`), not a select: the suggestions are the banks installed in
  OPFS, but a typed name is accepted too. A select-only control would strand a
  track whose project names a singer this browser has not installed — exactly the
  case when importing the Hatsune file, which references 重音テト. Commits on blur
  or Enter; a typed draft is not overwritten by the engine's value mid-keystroke.

**Verified**
- `cargo test -p project-session -p commands`: all suites pass except one
  **pre-existing** failure, `session_v2::a_note_edit_by_id_reports_a_patch_and_
  invalidates_render` (`left: None, right: Some(1440)`). Confirmed pre-existing by
  removing the new `track.set_singer` arm and re-running: it fails identically.
  Not caused by this change and not fixed here.
- `cargo build --release --target wasm32-unknown-unknown -p engine-wasm` +
  `wasm-bindgen` → `web/pkg`; `cargo build --release -p synth-server` (the HTTP
  host had to be rebuilt too — the probe caught it still answering
  `unknown command type 'track.set_singer'`, which is the host-parity check
  working).
- `node --test tests/` → 21/21.
- `npm run build` → pass.
- probe `?backend=http` → **87/87, all pass**, including the three new checks
  (set through the session, undo as one transaction, clearing stores `null`).
- probe Direct WASM → **85 ok, 2 fail**, both the `resultOf` pronunciation-sidecar
  bug in `web/app/session-driver.js` recorded in T004, not touched here.
- In-browser: picking "Dev Fixture" moved rev 5 → 6 with `track.changed`; typing a
  name moved 5 → 6; undo returned it to `null`.

Still to build from this record: the expression-descriptor read (#2), the
wave-part render path (no contract work), adopt-a-parts (`Import Tracks…`), and
the wave mix-failure read.

## 2026-10-03T17:20Z — sequencing update (owner): curve editing first

Owner direction: tempo work moves here (section D above, recorded not built), **curve editing is
the next implementation priority**, and mixer/FX add-on effects are a plan item only (section E).
Nothing in this record was implemented in that pass and frozen §6 was not touched.

The curve work itself was done in T004 against commands that already exist, so it needed no
contract change: `curve.apply_stroke` was already routed, and Rust already simplifies the committed
curve at commit time (`commands::simplify_curve`, tolerance `min(5, (max-min)*0.005)`). What was
broken was the Web host, which was throwing the gesture away before the engine saw it. T004 has
the details.

Still owed here, unchanged and still all read-only or command-level:

1. expression-**descriptor read** — §6.10 family, no command/schema change
2. native **stroke-context route** (HTTP has no equivalent of `session_curve_stroke_context_json`)
3. **wave-part render path** — engine-only, no contract work
4. **adopt-a-parts** (`Import Tracks…`) — needs a command
5. `track.add/remove/reorder` — currently `unsupported_operation`
6. **tempo operations** (section D) — Adjust Tempo and Preserve Timing, both document-wide
7. **mixer / FX add-on effects** (section E) — plan only; sequenced after the track commands

## Contract proposal drafted + multitrack acceptance + the BOM defect (2026-10-04T06:05:00Z)

### The proposal

`docs/planning/contract-v2-extension-t014.md` — the versioned contract-update input this
record asked for. CONTRACT-v1 §6 is **unchanged and still frozen**; every operation in the
proposal is labelled *proposed*, and the router still answers `unsupported_operation` for
`track.add*` / `track.remove*` / `track.reorder*` (verified: `v2.rs:1706-1711`).

It covers all six original completion criteria: structural track create/remove/reorder with
minted IDs, defaults, ordering, atomicity and rollback; per-note properties (vibrato, tuning,
phoneme expressions, the already-implemented `note.set_pitch`) with units, validation and
defaults; a field-by-field split of persistent track configuration vs presentation, with
renderer settings / mix FX / track expressions explicitly **deferred**; bounded reads
(expression descriptor, stroke context, import diagnostics); and the shared-semantics checklist
every addition must satisfy before implementation.

Three positions worth surfacing at review:

- **`track.remove` deletes the track's parts.** `part.track_no` is a dense index, so leaving
  parts behind would silently re-home them to whichever track took the index. Removing the
  last track is rejected.
- **`track.reorder` must renumber `part.track_no` without minting, dropping or reordering any
  stable ID.** This is the invariant that single-track fixtures cannot test.
- **Phoneme-count change (§4.3) is an open question with a recommendation**: refuse the commit
  while per-phoneme data exists, then let the user clear it explicitly. Silent truncation
  loses an edit without saying so, which is the failure mode AGENTS.md is written against.

### The defect that made multitrack untestable

**No real OpenUtau `.ustx` could be opened at all.** Every project OpenUtau writes starts with
a UTF-8 BOM — `core/OpenUtau.Core/Format/USTx.cs:102` saves with
`File.WriteAllText(..., Encoding.UTF8)`, and .NET's `Encoding.UTF8` emits a BOM preamble.
`serde_yaml` does not skip it, so the first key becomes unreadable and the document parses as
two:

```text
deserializing from YAML containing more than one document is not supported
```

That message misdirects badly. `king.ustx` contains **zero** `---` separators — verified, not
assumed — and still failed exactly that way. Stripping the BOM loads it with **5 tracks /
5 parts**. Before this, the multitrack case could not be tested at all, which is why it was
never noticed.

Fixed in the domain, not at the call sites: `UProject::parse_yaml` is the canonical
BOM-tolerant loader, and `UProject::from_yaml_text` is the same parse without the `after_load`
repair step, for callers that need the `LoadRepair` warnings and must run the repair exactly
once. Routed through it: `pipeline::parse_project`, `ProjectDoc::from_yaml`, and
`Session::open_with_pronunciation_sidecar`.

I initially wired `open_with_pronunciation_sidecar` through `parse_yaml`, which would have run
`after_load` twice; it now uses `from_yaml_text` so the repair runs exactly once.

### Multitrack acceptance, against a real 5-track project

`native/crates/project-session/tests/multitrack.rs` (new, 5 tests) and
`native/crates/domain/tests/ustx_load.rs` (new, 5 tests) run against
`test-ustx-example/king.ustx` — a genuine 5-track / 5-part OpenUtau project, chosen because
single-track fixtures cannot distinguish "the session tracks part ownership correctly" from
"it always resolves against track 0": a part bound to the wrong track still returns a
valid-looking part.

Pinned: 5 tracks open; 5 distinct stable IDs; each track owns exactly its own part and the
snapshot's part ID matches the identity table; a `track.set_mute` by stable ID on track 2
changes **exactly** track 2 and no other; save→reopen preserves shape and the deterministic
IDs. Verified the mutation test is not vacuous by deliberately breaking the expectation — it
reported `left: [2]`, then was restored and re-run green.

Two test-authoring mistakes worth recording, because both would have shipped false confidence:

- The fixture YAML was hand-written and drifted from the model (`mute: 0` where the field is a
  bool). It is now generated from `serde_yaml::to_string(&UProject::create())`, so it cannot
  disagree with the domain.
- `repo_root()` was one directory short, so every fixture-dependent test **passed vacuously**
  by silently skipping. Fixed, and the tests now assert on the checked-in fixtures plus
  `checked > 0` where a skip is still legitimate (the fixtures are gitignored).

Also corrected while drafting: I first wrote that `track.set_singer` lives in `commands` — it
is routed in `project-session/src/v2.rs:1664`. And I cited a `USTx.cs` path I had not checked;
the correct one is `ref-openutau-clean/core/OpenUtau.Core/Format/USTx.cs:102`, which does
confirm the `Encoding.UTF8` BOM write.

### Verified

- `cargo test -p domain` → all green, including 5 new `ustx_load` cases.
- `cargo test -p project-session` → all green except the pre-existing
  `a_note_edit_by_id_reports_a_patch_and_invalidates_render` (`left: None, right: Some(1440)`,
  in `session_v2.rs` which this work does not touch). 5 new `multitrack` cases pass with zero
  skips.
- `cargo test -p phonemizer -p synth-cli -p ustx-codec` → all green.
- `cargo build --workspace` and `cargo build -p engine-wasm --release` → clean.
- `./web/run-editor.sh` → **OK, exit 0**; probe `?backend=http` and `?backend=wasm` →
  **all checks passed, exit 0** on both; `node --test tests/` → **71 pass / 0 fail**, 3 skipped.
- `git diff --check` → clean.

### Still owed

- Review has not happened, so nothing here may be represented as live. No endpoint or schema
  claim in the proposal is implemented.
- Other project-text parse sites (`domain` round-trip tests, `phrase`, `train-prep`) still
  bypass the canonical loader. `ustx-codec`'s `UstxDecode` was the important one and is now
  fixed (see below); nothing enforces the "always use the canonical loader" rule, so a new
  call site can still reintroduce the bug.
- `track.add/remove/reorder`, `note.set_vibrato/tuning/phoneme_expression`, and the bounded
  reads are all still `unsupported_operation`.
- Pre-existing: clippy fails in `worldline-sys` (untouched crate); voicebank resolution is
  still not in Rust so projects are not portable across hosts.

### Closed after drafting: the `UstxDecode` gap

I had listed `ustx-codec`'s `UstxDecode` FFI entry as still bypassing the BOM-tolerant loader.
It was a one-line fix in a crate that already depends on `domain`, so leaving a known hole open
was the wrong call — it is now routed through `UProject::parse_yaml`, with a test
(`decode_accepts_a_utf8_bom_like_every_real_openutau_file`) that feeds the FFI entry a
BOM-prefixed `.ustx` and asserts it decodes the track and the note lyric.

Verified the test is not vacuous by temporarily reverting the fix to `serde_yaml::from_str`:
the test fails with `UstxDecode must accept a BOM-prefixed .ustx`. Restored, and
`cargo test -p ustx-codec` → **4 pass / 0 fail**.

The FFI path matters more than it looks: `synth-server/src/handlers.rs:335` calls
`UstxDecode` for the codec decode route, so the host-facing surface had the same defect.

### The loader rule is now enforced, not just documented (2026-10-04T06:40:00Z)

The "always use the canonical loader" rule existed only as doc comments, so the next call
site could reintroduce the BOM bug silently. `native/crates/domain/tests/ustx_loader_discipline.rs`
(now 2 tests) scans every `.rs` under `crates/` and `tools/` and **fails** if a production call
site deserializes `UProject` with `serde_yaml` directly.

It allow-lists deliberately: integration tests and `examples/` (a test parsing a hand-written
fixture without a BOM is fine), doc comments (this file names the anti-pattern on purpose), and
`domain/src/project.rs` itself (the loader must deserialize *somewhere*). Voicebank and
DiffSinger configs are excluded by path — they are different formats, not project text.

A second test pins the loader itself, because the allow-list means the scan would still pass if
someone "simplified" `parse_yaml` back into a bare `serde_yaml` call while the real defect
returned: it asserts `parse_yaml` accepts a BOM **and** raw `serde_yaml` rejects one. If serde
ever learns to skip BOMs, that test tells the next person the whole guard can be simplified.

**Verified the guard is not decorative.** Injecting a real bypass into `engine-wasm/src/lib.rs`
— replacing `parse_project(ustx_yaml)` with `serde_yaml::from_str::<UProject>(ustx_yaml)` —
made it fail with the file and line:

```text
Offending lines:
  crates/engine-wasm/src/lib.rs:260: let project: domain::UProject = serde_yaml::from_str(ustx_yaml)…
```

Restored, and both call sites confirmed back in place.

Audit result: **no production bypass remains.** Every remaining `serde_yaml::from_str` on a
project-shaped type is inside a `#[cfg(test)]` module. All real entry points route through the
canonical loader — `engine-wasm` (3 sites via `pipeline::parse_project`), `project-session`
(`from_yaml`, `open_with_pronunciation_sidecar`), `ustx-codec` (`UstxDecode`), `synth-server`
(`render_service`, `sessions`).

### Verified (this increment)

- `cargo test -p domain` → green, including 5 `ustx_load` + 2 `ustx_loader_discipline` cases.
- `cargo test -p project-session -p phonemizer -p synth-cli -p ustx-codec` → green except the
  pre-existing `a_note_edit_by_id_reports_a_patch_and_invalidates_render` (untouched file).
- `cargo build --workspace` → clean.
- `./web/run-editor.sh` → **OK, exit 0**; probe `?backend=http` and `?backend=wasm` →
  **all checks passed, exit 0** on both; `node --test tests/` → **71 pass / 0 fail**, 3 skipped.
- `git diff --check` → clean. No scratch files left behind.

### A doctest I broke and caught only by running the full suite

`parse_yaml`'s doc comment quotes the serde error message in an indented block:

    deserializing from YAML containing more than one document is not supported

Rust treats an indented block in a doc comment as a **doctest** and tries to compile it, so
`cargo test -p domain` failed with `error: expected one of '!' or '::', found 'from'`. Fixed by
fencing the block as ```text.

Worth recording because of *how* it was caught. I had been running the new integration tests
individually (`--test ustx_load`, `--test ustx_loader_discipline`), all of which passed. Running
`cargo test -p domain` without a filter — which also runs doctests — is what surfaced it. Had I
kept filtering by test target, I would have shipped a red `cargo test -p domain`.

The lesson is narrow and concrete: after editing a **doc comment** in this repo, run the
package's full test command, not just the new test target.

Also corrected in the same comment: it cited `ref-openutau-clean/desktop-ref/OpenUtau/Format/
USTx.cs`, which is not the file. The real one is
`ref-openutau-clean/core/OpenUtau.Core/Format/USTx.cs`, which does confirm the
`File.WriteAllText(..., Encoding.UTF8)` BOM write.

### Multitrack verified through the real UI, not just in Rust (2026-10-04T06:55:00Z)

The Rust side had 5-track acceptance, but nothing had ever put a real multitrack project in
front of the clone. `tools/drag-probe.mjs` now imports `test-ustx-example/king.ustx` — a real
OpenUtau file with a BOM, 397 KB, 5 tracks / 5 parts / 1124 notes — through the actual File →
Import dialog, and asserts five things about the result: the engine reports 5 tracks, their
stable IDs are distinct, each track owns exactly one part, the notes span **5 distinct part
ids** rather than piling onto the first track, and no error toast appears.

Result: **passes on both backends** — native HTTP and direct WASM — so the same 5-track file
survives the whole host path, not just `UProject::parse_yaml`. No application code changed; the
defect was already fixed, and this proves it end to end.

The "notes span 5 part ids" check is the one that matters. A part attached to the wrong track
still renders and still looks plausible, so track ownership is only observable through identity.

**Three mistakes in my own check, each caught rather than assumed:**

1. I read `n.partId` on note objects and got 0 distinct ids. The field is `part_id` — snake_case,
   like the rest of the session DTOs. Guessing the field name produced a *red* result, not a
   false green, so it was caught immediately; had I asserted loosely it would have shipped a
   meaningless check.
2. Fixed to `part_id` and still 0 — because `state.notes` is **keyed by part id** (`engine.js`
   `readNotes`), so the part identity is the map key and notes do not repeat it. The check now
   reads the real shape: `Object.entries(s.notes)`.
3. My explanatory `//` comment went *inside* a template literal, whose trailing backslash made
   the line a continuation — `SyntaxError: missing ) after argument list`. Moved the comment to
   JS level. This broke the whole probe, not just the new check, and is worth remembering when
   editing evaluated snippets.

**Verified the check is not decorative:** slicing the part-id list to 1 element made it fail
with `1124 notes across 1 distinct part ids`, then it was restored and re-run green.

Fixture-dependent, so it prints an explicit `skip …` line when `test-ustx-example/` is absent
rather than passing silently.

### Verified (this increment)

- probe `?backend=http` → **all drag checks passed, exit 0**; `?backend=wasm` → **exit 0**; both
  include the 5 new multitrack checks.
- `./web/run-editor.sh` → **OK, exit 0**. `node --test tests/` → **71 pass / 0 fail**, 3 skipped.
- `git diff --check` → clean; no scratch files left behind.
- **No application code changed in this increment** — probe coverage only.

### Correction: the `engine-wasm` → `synth-cli` dependency is NOT a policy violation

I described this repeatedly in conversation as "the clearest policy violation that remains".
**That was wrong**, and the correction is worth recording because I checked rather than
defending the claim:

- `synth-cli` **has a `src/lib.rs`**. Its `[[bin]]` target carries `required-features = ["cli"]`,
  so it is a legitimate library crate that additionally ships a binary. `engine-wasm` consumes
  `synth_cli::pipeline`, which is real code that runs on wasm.
- **`AGENTS.md` says nothing about crate layering.** It is a *state ownership* policy: no second
  project model / musical rule set / serializer / render authority outside Rust. This dependency
  does not violate any of that — everything it provides is still in Rust.

The accurate description is a **structural smell, not a violation**: a library crate living under
`tools/` is depended on by `crates/engine-wasm`, which muddles the path and the feature story.
The tidy fix is to move `pipeline` into `crates/pipeline` and leave the CLI as a thin wrapper.
Worth doing for clarity; nothing is misbehaving, and no user-visible defect traces to it.

### Why the wasm host has no ONNX runtime (answers a standing question)

`engine-wasm` has no `ort`/`onnx`/`r2` dependency at all, and it pulls `synth-cli` with
`default-features = false`, which turns **off** the `r2` feature — the one that carries `ort`,
`diffsinger`, `moe-train` and `burn`.

So the wasm host has **no ONNX runtime by design**, not because one went missing. Consequence:
phonemizers and engines that need ONNX (the two Vogen classes, Enunu, DiffSinger) cannot run in
the browser at all. This matches the T013 plugin-matrix record, which saw both Vogen classes
fail in Browser WASM purely because the native `onnxruntime` library is unavailable there.

This is a deliberate trade — a large ONNX runtime in a wasm bundle is not worth it for the
browser host — but it means **host capabilities must be reported from what is actually
available**, not from what would be desirable. Enabling it later is a fresh decision with a
measurable cost, not a flag flip.

### Audio track (wave part): half-built, and the blocker is portability

Current state, verified rather than assumed:

- **Present:** `domain::UWavePart`; `.ustx` parse/serialize; `wave_part_view()` produces a
  `PartView` with `kind: "wave"`, `media_ref`, and a duration computed from `file_duration_ms`.
- **Missing:** `grep` finds **no `UPart::Wave` match anywhere in `phrase/` or `pipeline.rs`**.
  Wave parts load and display but are **never mixed into the render**. Audio output is
  voice-parts-only.

So "do the audio track" is not a matter of adding a mixer call. `relative_path` points at a file
on disk, and **each host stores its audio differently** — the browser keeps things in OPFS, the
server reads from `--voicebanks`/project directories. That is the *same* unresolved problem as
voicebank resolution (still not in Rust, so projects are not portable across hosts), and audio
tracks would inherit it wholesale.

Sequencing consequence: audio-track rendering is blocked behind, or should land together with,
engine-owned media resolution. Doing only the mixer half would produce a feature that renders
silence on the web host while working on native — the exact class of host divergence the policy
forbids.

## Fresh v3.1 acceptance audit (2026-10-15T04:42:06Z)

- Re-ran `cargo test --manifest-path native/Cargo.toml -p project-session`: all 116 tests across its integration targets passed. One existing dead-code warning remains in `conformance_vectors.rs` (`Expect.label`).
- Re-ran `cargo test --manifest-path native/Cargo.toml -p engine-wasm --test project_controls_v31`: 1/1 passed; the Direct-WASM adapter advertises `project.controls.v1` and reads curve context.
- Re-ran `cargo test --manifest-path native/Cargo.toml -p synth-server --test api_v2_sessions project_controls_v31_reads_are_available_through_native_http`: 1/1 passed; native HTTP exposes the shared Rust curve-context read. Build emitted existing warnings in DiffSinger, moe-train, synth-cli and synth-server.
- This verifies current Rust/adapter evidence for the shipped T014 slice. It does not verify browser UI wiring, stable-target phoneme operations (T028), T018's transient phonemizer-expression precedence, or user acceptance of the v3.1 extension; task remains active until its separate acceptance gate is resolved.

## Current consistency and host recheck (2026-10-15T05:18:07Z)

- Compared the v3.1 proposal's `project.controls.v1`, descriptor/stroke-context reads, and note/project commands with the current shared Session capability list, command planner, Direct-WASM adapter, and native HTTP routes. The implemented capability/read seams match the draft; this source inspection did not establish browser UI integration or acceptance of the extension.
- Fresh verification: `cargo test --manifest-path native/Cargo.toml -p project-session` — all test targets passed (the existing `conformance_vectors.rs` dead-code warning remains); `cargo test --manifest-path native/Cargo.toml -p engine-wasm --test project_controls_v31` — 1/1 passed; `cargo test --manifest-path native/Cargo.toml -p synth-server --test api_v2_sessions project_controls_v31_reads_are_available_through_native_http` — 1/1 passed. These cover shared Session behavior and the two host adapters, not complete browser/product acceptance.
- No source or contract files were edited in this recheck. Separate v3.1 acceptance/review remained outstanding; T028 stable-target phoneme operations, T018 transient phonemizer-expression precedence, and T004 UI wiring remain separate dependencies.

## Acceptance handoff (2026-10-15T05:18:49Z)

- Reclassified T014 as blocked at the remaining external gate: MK3 §2.1 makes acceptance of each later extension separate from implementation authorization. The v3.1 extension implementation and current native/Direct-WASM host checks pass, but the user has not accepted this extension as a product contract. Do not interpret the earlier implementation authorization as extension acceptance or change frozen CONTRACT-v1.
- To resume/close this gate, the user must review and accept or request changes to `docs/planning/contract-v3.1-project-session-controls.md`. T028 stable-target phoneme operations, T018 expression precedence, and T004 UI wiring remain out-of-scope follow-ups, not evidence that v3.1's implemented slice failed.
