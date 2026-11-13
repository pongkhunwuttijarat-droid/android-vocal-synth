# Phoneme compose contract proposal (v2 revision draft)

**Status:** proposal draft for review — **not frozen**. Composition core and
first session/native/WASM adapter paths are implemented in the working tree;
cross-adapter acceptance remains pending. CONTRACT-v1 §6 is frozen;
this document is the versioned contract-update input for the composition
workflow defined in [phoneme-input-composition.md](phoneme-input-composition.md).
Date: 2026-10-03.

## Summary

Two logical operations and their DTOs for the layered phoneme input workflow:

1. `phoneme.compose` — a **read**: text in, bounded deterministic candidate
   list out. Never mutates project state.
2. `note.set_pronunciation` — a **command**: commit one candidate as the
   note's pronunciation. One undo step, one revision transition, atomic.

Both take stable project/part/note identity. The committed form is the
universal phone sequence plus language, mode and style — bank aliases are
derived engine output recomputed when the selected voicebank changes.

## DTO definitions (implemented in `phonemizer::composition`)

The Rust source of truth is [`native/crates/phonemizer/src/composition.rs`](../../native/crates/phonemizer/src/composition.rs).
Field spellings below are the actual serde output, pinned by the conformance
test [`tests/composition_conformance.rs`](../../native/crates/phonemizer/tests/composition_conformance.rs)
and reproducible with
`cargo run -p phonemizer --example t013_compose_json`.

### phoneme.compose (read)

Request:

```json
{
  "input": "か",
  "mode": "universal",
  "style": "auto",
  "limit": 8,
  "tone": 60,
  "voicebank_id": "vb-ja-test"
}
```

- `input` — the typed text/sound hint (≤256 chars; longer is truncated with a
  diagnostic). A `[...]` suffix is a phonetic hint and is authoritative.
- `language` — optional explicit language id (`"ja"`, `"en"`, `"th"`); omitted
  = auto-detect from script. A forced language that disagrees with the script
  adds a diagnostic; it never silently switches strategy.
- `mode` — `universal` (default) or `voicebank_native`; which view the user
  edits. Candidates always carry both views.
- `style` — `auto` (default) or `cv` / `vcv` / `cvvc` / `vccv`; a non-auto
  style restricts candidates to strategies built for that style and reports
  when the bank has none.
- `limit` — 1..=16 candidates (default 8, clamped).
- `tone` — tone context for alias lookups (subbank prefix/suffix mapping).
- `voicebank_id` — host-assigned, echoed in the result.

Result:

```json
{
  "composition_id": "comp-4d15616de1f3f8ec",
  "input": "か",
  "language": "ja",
  "mode": "universal",
  "requested_style": "auto",
  "voicebank_id": "vb-ja-test",
  "candidates": [
    {
      "candidate_id": "cand#ja#direct_alias#cv#4d994c1b8369eb86",
      "display_text": "か",
      "language": "ja",
      "style": "cv",
      "strategy": "direct_alias",
      "language_phones": ["か"],
      "universal_phones": ["か"],
      "voicebank_aliases": ["か"],
      "unresolved_phones": []
    },
    {
      "candidate_id": "cand#ja#japanese_vcv#vcv#4d994c1b8369eb86",
      "display_text": "か",
      "language": "ja",
      "style": "vcv",
      "strategy": "japanese_vcv",
      "language_phones": ["か"],
      "universal_phones": ["か"],
      "voicebank_aliases": ["- か"],
      "unresolved_phones": []
    },
    {
      "candidate_id": "cand#ja#mora_cv#cv#4d994c1b8369eb86",
      "display_text": "か",
      "language": "ja",
      "style": "cv",
      "strategy": "mora_cv",
      "language_phones": ["か"],
      "universal_phones": ["か"],
      "voicebank_aliases": ["か", "a"],
      "unresolved_phones": []
    },
    {
      "candidate_id": "cand#ja#universal_ipa#cv#4d994c1b8369eb86",
      "display_text": "か",
      "language": "ja",
      "style": "cv",
      "strategy": "universal_ipa",
      "language_phones": ["か"],
      "universal_phones": ["か"],
      "voicebank_aliases": ["か"],
      "unresolved_phones": []
    }
  ],
  "diagnostics": []
}
```

Style-restriction example (`style: "vccv"` against a bank without VCCV
aliases) — candidates are dropped, not guessed:

```json
{
  "composition_id": "comp-9ba2d85ecde383bd",
  "input": "か",
  "language": "ja",
  "mode": "universal",
  "requested_style": "vccv",
  "voicebank_id": "vb-ja-test",
  "candidates": [],
  "diagnostics": [
    "no vccv candidates for this input in the selected voicebank"
  ]
}
```

Thai script detects as `"th"` and uses the rule-based Thai VCCV converter;
unsupported spellings remain explicit unresolved phones rather than being
silently dropped. Neighbor-aware CV/VC and part-boundary alias selection are
not part of the single-lyric composition read yet.

### note.set_pronunciation (command)

The committed form persisted on the authoritative session model:

```json
{
  "language": "ja",
  "mode": "voicebank_native",
  "style": "cv",
  "strategy": "mora_cv",
  "language_phones": ["か"],
  "universal_phones": ["か"]
}
```

`language_phones` and `strategy` are retained because the universal sequence
alone cannot reconstruct language-specific syllable boundaries or bank-style
sequence construction (for example Japanese VCV or Thai VCCV). They remain
engine-owned semantic inputs; actual bank aliases are still derived.

Command sketch (exact envelope follows the v2 session command shape):

```json
{
  "type": "note.set_pronunciation",
  "project_id": "...",
  "part_id": "...",
  "note_id": "...",
  "expected_revision": 41,
  "pronunciation": {
    "language": "ja",
    "mode": "voicebank_native",
    "style": "cv",
    "strategy": "mora_cv",
    "language_phones": ["か"],
    "universal_phones": ["か"]
  }
}
```

The `pronunciation` value may be `null` to clear a choice. Failures
(`expected_revision` mismatch, unknown note, empty or over-limit sequences,
unsupported IDs) return the typed error without partial mutation. The commit
is one undo step and one revision transition. `note.set_lyric` clears a stale
committed pronunciation in the same transaction; clients may group the lyric
and new pronunciation commands for one undo step.

Derived aliases are re-read per voicebank:

```json
[
  { "universal_phone": "か", "alias": "か", "kind": "exact" }
]
```

`kind`: `exact` (direct/bridged alias hit), `nearest` (closest supported
phone substituted), `unresolved` (verbatim, unrenderable). Without a
voicebank every phone is `unresolved`.

## Semantics (shared across hosts)

- **Determinism and bounds.** Identical request + voicebank produce
  byte-identical results (fixed strategy order, resolved-first stable sort).
  Input ≤256 chars; candidates ≤16. No hash-map iteration in output paths.
- **Transient ids.** `composition_id`/`candidate_id` are deterministic for
  the request context but are NOT persisted; commits carry the canonical
  phone sequence, never a row index.
- **Provenance.** `strategy` records which layer produced each resolved
  alias: `direct_alias`, `phonetic_hint`, `japanese_vcv`, `mora_cv`,
  `g2p_cvvc`, `letter_fallback`, `universal_ipa`.
- **Style by construction.** Strategy-built candidates tag the style they
  were built for; shape classification (`classify_alias`) applies only to
  aliases the bank already provides. A style without matching aliases is
  reported, never guessed.
- **Unresolved is explicit.** `unresolved_phones` lists phones the selected
  bank cannot render, in the candidate's own sequence granularity. Nothing
  is silently dropped or replaced.
- **Persistence boundary.** Only the committed form (universal phones +
  language + mode + style) belongs to the authoritative model. Bank aliases
  are derived and must not be duplicated into frontend project state.
  USTX-imported notes without such metadata keep using their lyric/hint as
  input.

## Session wiring plan and current implementation map

1. Keep `.ustx` unchanged and preserve the existing `rusted-utau.ids/1` file. Store choices in a
   separate `<project>.ustx.rusted-utau-pronunciations.json` sidecar with schema `rusted-utau.pronunciations/1`,
   `project_id`, and a stable-note-ID → committed-form map. Open tolerates a missing/corrupt or
   wrong-project pronunciation sidecar by dropping the choices with a warning. Save As forks
   the project ID while retaining entity IDs and pronunciation choices in its staged bundle.
2. `project-session` accepts the committed form as in-memory document state so undo/redo and
   group transactions include it; render and snapshots consume that state. Note deletion drops
   the choice; import creates a clean identity domain; lyric changes clear a stale choice.
3. `phoneme.compose` is a session read (revision-tagged) with the
   selected voicebank resolved by the host. Working-tree entry points are
   `Session::phoneme_compose_json`, WASM
   `session_phoneme_compose_v2`, and native
   `POST /api/v2/sessions/{id}/phonemes/compose`.
4. `note.set_pronunciation` / `note.set_lyric` use stable IDs through the
   shared `session_apply_v2` semantics. The WASM and native adapters share
   this path; the server route must continue to use the v2 session command
   endpoint. The HTTP route is part of this draft and is not a frozen §6
   promise yet.
5. Render snapshots are captured at the requested revision. The native adapter
   passes an immutable USTX snapshot and stable-ID-aligned committed choices to
   the shared pipeline; WASM passes the same choices directly from `Session`.
   The pipeline re-composes the chosen strategy/style against the active
   voicebank and validates the reconstructed phone sequences against committed
   values. Japanese VCV uses contextual part processing for preceding-note
   continuity. This path does not modify saved USTX. Runtime parity and broader
   neighboring-note coverage still need acceptance.
6. `engine-wasm` and `synth-server` adapters map the same DTOs (JSON shapes
   above) per host transport; no new HTTP on WASM.
7. Editor UI requirements are drafted in T014. Frontend implementation stays deferred until
   session/native/WASM conformance is established.

## Conformance vectors

- [`tests/composition_conformance.rs`](../../native/crates/phonemizer/tests/composition_conformance.rs)
  pins the canonical request/result/restriction/commit JSON above.
- Unit coverage for determinism, bounds, styles, diagnostics, universal
  mapping and alias rederivation lives in `composition.rs` (`#[cfg(test)]`).
- Cross-adapter vectors (native HTTP + WASM) are required before any adapter
  advertises support — same rule as CONTRACT-v2 gates.

## Additional requirements for T014 merge (draft; UI implementation deferred)

Merge these requirements into the T014 session-extension proposal before any frontend UI work:

- Candidate reads and pronunciation commits use stable project/part/note IDs and the same
  Rust-owned session semantics on native and WASM.
- The lyric and selected pronunciation commit together as one transaction/undo step; canceling
  composition does not mutate committed state.
- The session sidecar preserves choices across save/reopen and Save As, and defines behavior for
  import replacement, note removal, ID repair, undo, and redo. CONTRACT-v1's `rusted-utau.ids/1`
  format is not silently changed; choose a separately versioned session sidecar or reviewed
  schema revision in the combined draft.
- Render rederives bank aliases from the stored universal sequence using the active voicebank.
  T014's per-phoneme expressions/overrides must define their relationship to sequence changes.
- Frontend requirements are presentation-only: raw text/highlight/popup state, Up/Down/Enter/
  Escape, stale-result discard, visible provenance/unresolved diagnostics, and authoritative
  refresh after commit/undo/redo. No frontend conversion or validation rules.
- T013 backend conformance must be complete before UI integration starts; UI is a later T014
  slice and is intentionally not part of the current implementation.

## Open questions for the combined T014 review

1. Command/read naming: `phoneme.compose` / `note.set_pronunciation` vs the
   existing v2 verb families.
2. Does compose need `expected_revision` (read-staleness) or is
   revision-tagging at the session layer enough?
3. Which versioned sidecar schema wraps stable IDs and pronunciations while
   preserving compatibility with existing `rusted-utau.ids/1` files.
4. Whether `mode` belongs to the committed form or the UI (it records which
   view the user edits; render should be driven by the sequence either way).
5. How per-phoneme expressions/overrides behave when a committed phone sequence changes.
