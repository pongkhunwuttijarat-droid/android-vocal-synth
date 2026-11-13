# T003 — Add browser voicebank import and OPFS flow

**Status:** done
**Owner:** Codebuff
**Last updated (UTC):** 2026-10-01T12:10:00Z

## Goal

Allow a user to import, select, persist, and use a voicebank in the browser
without relying on a development HTTP fixture server.

## Completion criteria

- [x] Confirm current OPFS and `OpfsBank` implementation and identify the missing user flow.
- [x] Provide a supported import path and persist bank files/metadata in OPFS.
- [x] Select the imported bank in the editor and load the required samples for render.
- [x] Verify project/browser reload behavior and report import/read errors clearly.

## Source planning

- [WASM migration status](../../planning/wasm-migration-status.md)
- [Web UI design](../../planning/web-ui-design.md)

## Repository checkpoint

- Branch: `wasm-native-rust` (dirty working tree predates this task; the files
  below are the ones this task touched).
- Files changed:
  - `web/samples.js` — `OpfsBank.importFiles` now also accepts `{path, bytes}`
    entries (zip/test path); new static `OpfsBank.installedRoots()` and
    `OpfsBank.removeRoot(root)`.
  - `web/app/editor.js` — bank selection at boot (imported OPFS bank →
    fixture; `?voicebank=<root>` forces an imported bank, `?voicebank=full`
    keeps the HTTP full-Teto escape hatch), Voicebanks sidebar panel,
    `importVoicebank()` (import + oto sanity check + rollback),
    `switchVoicebank(root|null)` (stop/cancel, `set_voicebank`, pool
    rebuild, re-render), `renderVoicebankPanel()` (select/delete rows).
  - `web/editor.html` — Voicebanks panel (Import folder… + list).
  - `web/editor-test.html` — step 13: build a synthetic bank from fixture
    bytes, import → OPFS → switch → render → fallback → reload check → delete.
  - `web/README.md` — voicebank OPFS section, browser matrix, checkpoint list.

## Progress

- 2026-09-30: Added from 2026-09-23 notes saying project OPFS storage exists while voicebank flow still uses HTTP fixtures. Verify current code first.
- 2026-10-01: Gap confirmed — `OpfsBank` existed (samples.js, harness
  `index.html`) but the editor boot hardcoded `HttpBank` on the fixture and
  had no import UI or bank switch. Implemented the flow above; imported banks
  persist under `<root>/voice/…` (same layout `HttpBank` serves), render goes
  through the existing per-chunk sample loading so large banks never load
  fully into the engine.

## Verification

- `bash web/run-editor.sh` (headless Chromium, wasm engine): **RESULT: PASS**,
  all 13 steps, including the new step 13 —
  `importFiles wrote 5 entries` → `switched -> "test-bank-…" (opfs) · aliases=4`
  → `render on imported bank OK · 639343 samples` → `fallback -> "Teto (demo)"
  (fixture)` → reload-from-OPFS byte-size equality → delete → `remaining = []`.
- Syntax checked with `node --check` (editor.js, samples.js).

## Blockers and dependencies

- Resolved: directory-picker support differs by browser; the picker path is
  used for imports (Chrome/Edge/Firefox) and `{path, bytes}` import remains
  available for zip unpacking. On Safari (no full OPFS `createWritable`) the
  editor still boots with the fixture bank.

## Next step

None — done. Possible follow-up (not part of this task): zip import via
`{path, bytes}` + a bank metadata sidecar (display name / singer info).

## Handoff notes

Keep browser storage as an adapter for engine data; do not move project or
musical authority into the browser layer. Voicebank roots live at OPFS top
level and are discovered by the `voice/oto.ini` marker; `projects/` stays
reserved for the ProjectStore.
