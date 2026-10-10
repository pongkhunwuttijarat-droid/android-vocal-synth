# High-Frequency Editing Data Flow (Draft)

**Purpose:** Explain how React/Pixi UI should edit frequently changing curves/automation and notes while preserving the Project Session contract and Rust-owned state.

**Status:** Design note only. `ui-mock/react-dock/` is still an illustrative prototype; no project commands, WASM session adapter, curve input gestures, or audio render are wired there yet.

## Decision in one line

**Do not send one Rust mutation per pointer event. Keep a disposable preview locally while the pointer is moving, then send one semantic command on pointer-up. Rust validates, canonicalizes, commits, and returns the authoritative revision/result; the UI replaces its preview with that result.**

```text
pointer down
  └─ begin ephemeral gesture preview in UI
pointer move / pen samples (many, up to hundreds or thousands)
  └─ update Pixi preview only; zero persistent engine mutations
pointer up
  └─ build one semantic command from the completed gesture
      ├─ note.move / note.resize / bulk note command, or
      └─ curve.apply_stroke (replace | clear)
          └─ WASM Engine → project-session::Session
              ├─ check expected_revision and stable IDs
              ├─ validate/apply atomically; normalize curve points in Rust
              ├─ one undo entry + one revision for one committed gesture
              └─ return mutation result + patch/invalidation changes
                  └─ React/ Pixi reconcile preview to the Rust-authoritative result
```

The normative source remains [CONTRACT-v1 §6.5–6.6](../CONTRACT-v1.md#65-high-frequency-gesture-model-frozen). This draft keeps those rules unchanged.

## Curve stroke request

For an existing curve, the pointer-up request is conceptually:

```json
{
  "expected_revision": 12,
  "command": {
    "type": "curve.apply_stroke",
    "part_id": "part-uuid",
    "curve_id": "curve-uuid",
    "abbr": "pitd",
    "mode": "replace",
    "start_tick": 1200,
    "end_tick": 1680,
    "points": [
      { "tick": 1205, "value": 18 },
      { "tick": 1210, "value": 42 },
      { "tick": 1215, "value": 31 }
    ]
  },
  "include_snapshot": false
}
```

A stroke creating a new curve uses the contract's `create: true`, registered `abbr`, and no authoritative `curve_id`; Rust mints it. A clear request sends `mode: "clear"` and no points. Exact required-field and interval rules are in §6.6.2; this note must not redefine them.

The gesture is still **one committed command**, not a batch of `set_point` mutations. To reduce traffic, the frontend MAY simplify the outgoing stroke before sending only when it has the authoritative committed curve at the exact `expected_revision`, and uses the same §6.7.2 algorithm after applying §6.6.2 interval semantics. This is transport compression, not project state. If revision/base is missing or client parity is not tested, send the raw captured points (within advertised limits) or ask the user to retry after resync. Rust always repeats validation/canonicalization/simplification and its committed output wins.

The shared project-session core enforces `max_command_bytes = 1 MiB` and `max_stroke_points = 20,000`; clients should respect the adapter's advertised limits. A gesture that exceeds the limits after safe compression must be rejected atomically with `payload_too_large`—never truncate silently.

## Agreed simplification direction (draft semantics)

We agree to reuse **OpenUtau-compatible curve simplification** rather than invent a new curve shape rule. OpenUtau's `SetCurveCommand.Merge()` simplifies after a series of edit commands are merged into one undo command; our logical equivalent is: collect UI preview/batches, then simplify once at the single Rust commit boundary.

The acceptance details to port are being made explicit in [CONTRACT-v1 §6.7.2](../CONTRACT-v1.md#67-canonicalization--resampling-ownership-frozen): simplify the **whole curve** after stroke merge (as `UCurve.Simplify()` does), descriptor-relative tolerance `min(5, range * 0.005)`, perpendicular-distance recursive simplification, strict greater-than threshold, preserve endpoints and deterministic first-point tie break. This can remove redundant control points outside the drawn interval, but only within the declared tolerance. This still needs Rust implementation plus native/WASM parity tests; the current engine does not yet implement this simplifier.

### Curve resolution: ticks, not a fixed Hz rate

The domain does **not** define a fixed sample frequency in Hz. Curve X coordinates use part-relative ticks at 480 ticks per quarter note. `UCurve::INTERVAL = 5` is a snap grid used by point-setting/canonicalization; it means one grid interval is `5 / 480` quarter notes, not “always store a sample every interval.” Sparse curves are valid and linearly interpolated between stored points; storage density depends on which points are sent/edited.

If a UI does choose to emit one point at every 5-tick snap position, the equivalent grid density is:

```text
points_per_second = BPM * 480 / (60 * 5) = 1.6 * BPM
```

Thus at 120 BPM it is 192 grid positions per second (115,200 positions over 10 minutes); at 110 BPM it is 176/s (105,600 over 10 minutes). These are **grid positions**, not a mandatory curve sampling rate or a guaranteed point count. A 10-minute stroke at that density is far above the current 20,000-point per-command limit, so interval-batched transport/draft semantics would be necessary if the product wants strokes that dense/long without deliberate thinning. Transport batch cadence is separate from curve resolution and must not change musical tick semantics or create multiple undo steps for one gesture. Note: native HTTP body-limit → 413 mapping is an adapter gate to verify, and WASM must apply the same logical request-byte/point limits before parsing/mutation. Any UI-side point thinning is permitted only as presentation/transport optimization: it must not become a competing authority for canonical curve data. For the first implementation, prefer sending collected samples up to the negotiated limit; add local downsampling only if measured payload/performance needs it and document its error policy as preview/input policy, not as a replacement for Rust validation/normalization.

## UI-side state allowed during a gesture

For the currently selected curve, ephemeral component/gesture state may hold:

```js
{
  pointerId,
  baseRevision,
  curveId,
  startTick,
  endTick,
  rawPoints: [{ tick, value }, ...],
  pointerDown: true
}
```

This object is temporary and disposable. It must not be copied into a project store, an autosave payload, a render plan, OPFS project data, or any frontend serializer. It exists only to draw the live preview and construct the final user-intent command on pointer-up. Canceling/Escape/pointer-cancel drops the preview without issuing a committed mutation.

A display-only selected curve/point cache is allowed if it is revision-tagged and replaceable. It is not the source for the next committed curve state: the command is an intent over `part_id`/`curve_id`/range/samples, and Rust applies it against its current canonical document after checking `expected_revision`.

## Mutation result and reconciliation

A successful commit result must include the current authoritative revision, whether state changed, undo availability, and explicit changes. For `curve.apply_stroke`, the curve change should preferably carry the **canonical curve patch**; if the chosen result is invalidation-only, the adapter/client immediately requests the curve bounded read tagged with the same/current revision.

Client reconciliation rules:

1. If success returns a complete `curve.changed` patch at revision R+1, discard raw preview samples and render the patch.
2. If the result invalidates the curve, read the bounded curve DTO. Apply it only if its revision is not older than the revision currently held by the client.
3. If `revision_conflict` occurs, discard or mark the stale preview, fetch the authoritative session/curve read, update the revision, and require the user/gesture layer to retry from current data. Do not silently replay stale pointer samples on a newer curve.
4. If validation fails, keep no committed frontend copy of the stroke. Show the error and either discard preview or let the user explicitly restart the gesture.
5. Save/export/render always call the engine's committed session path; they never accept preview state as input.

## Undo, grouping, and high sample rate

- One completed freehand curve stroke is one semantic mutation, revision transition and undo step.
- One note drag/resize is one semantic note command or bulk command, not one command per pointermove.
- Groups are reserved for genuinely composite semantic actions. Do not use a group as a transport for every raw mouse/pen sample.
- If one gesture modifies multiple entities and no single bulk semantic command exists, one isolated group may wrap a bounded number of semantic child commands. The group still commits atomically as one undo step. It is not a license to push hundreds of raw preview samples into authoritative state.
- Preview cancellation before pointer-up needs no Rust undo because it never mutated the session.

## Planned React + Pixi + WASM boundary

1. **React shell** owns dock layout/workspace and component presentation state only.
2. **PixiJS view** draws authoritative snapshot/read models plus ephemeral pointer preview overlays. It emits a gesture intent; it does not create note/curve domain objects or decide project validity.
3. **WASM `Engine` adapter** exposes the existing Rust-owned session methods. A thin `WasmSessionClient` should own only call/marshalling mechanics, not a JS project document model.
4. **Rust `project-session`** remains the only persistent state/mutation authority.
5. **Render/Save adapters** snapshot the committed engine session at a known revision and report that revision; a live preview is never rendered/saved as though committed.

`ui-mock/react-dock/src/main.jsx` does not yet implement this boundary: its notes, curve plot, and Inspector example values are local illustration data and its controls do not call Engine. Do not label it an engine-connected editor until the adapter and conformance checks are wired.

## Implementation/test slice before enabling real curve editing

1. Add a small WASM client wrapper for `session_new_json`, `session_open_yaml`, `session_snapshot_json`, `session_capabilities_json`, `session_apply_v2`, `session_part_notes_json`, `session_curve_json`, group/history and sidecar payload operations.
2. Create/open the session and render the first screen from Rust snapshot/partial reads; retain only selected stable IDs and viewport/tool state in React.
3. Implement a Pixi pointer gesture object as above; on pointer-up send exactly one `curve.apply_stroke` with `expected_revision` and stable IDs.
4. Reconcile the returned canonical curve patch/read; make Save/Render consume only committed Engine session state.
5. Test: 500 pointer moves → one mutation call, one undo entry, one revision increment; preview canceled → zero mutations; failed/stale commit → Rust state/history unchanged; save/render before commit use committed state; native and WASM canonical stroke result/revision match.

## Things this note intentionally does not decide

- It does not freeze the final TypeScript/WASM glue function names.
- It does not add generalized blend modes, client-side curve simplification, or a second curve model.
- It does not change raw point byte/point limits already set in §6.15.
- It does not change the draft contract or claim adapter parity is complete.
