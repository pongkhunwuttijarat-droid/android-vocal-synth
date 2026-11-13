# T016 — ต่อ UI clone เข้ากับ contract §6.13 (render)

Status: active · Owner: Buffy · Last updated: 2026-10-04T09:40:38Z

## Goal

The user asked to connect the Web clone to the engine now, and explicitly to do
the **contract-conformant** work first, leaving the phoneme-picker (choosing a
phoneme while typing) for afterwards.

## What was found first

`ui-mock/react-dock` already had a preview render, but it took two different
paths per host and only one of them followed the contract:

- the in-browser pool rendered from an **exported USTX string**
- the HTTP fallback already used `POST /api/v2/sessions/{id}/render` (§6.13)

Neither backend exposed a render call, and there was no user-triggered render —
the preview always rendered **track 0** whenever the document changed.

## What was built

- `src/session-backends.js`
  - `DirectWasmBackend.renderTrack(track)` — wraps the existing
    `SessionDriver.renderTrack` (`session_render_project`). The direct host has
    no job, so it stays a synchronous call; inventing a job id to poll would be
    a fiction the UI then has to special-case.
  - `HttpSessionBackend.renderTrack(track, { voicebank, engine })` — the §6.13
    flow: POST the job with the **mandatory `expected_revision`**, poll
    `GET /api/v2/render/{id}` until terminal, then fetch the wav artifact.
  - `listVoicebanks()` — the render route takes a bank **name**, never a path.
    With no bank given, it falls back to the one installed bank that actually
    has samples; picking an empty one would only produce silence.
  - Both return the same `{ sampleRate, samples }`, so the UI never branches on
    backend kind. The WAV decode is a container unwrap, not musical state.
- `src/openutau/clone-state.jsx` — `renderTrack` action plus a `renderJob`
  state, kept separate from the existing debounced preview `render` state
  (one is a user's explicit job, the other is an automatic track-0 refresh).
- `src/openutau/file-menu.jsx` — **File ▸ Render track N…**, rendering the track
  the piano roll is showing. The track NUMBER is the index in the engine-ordered
  snapshot, which is what the route addresses.

## Bug caught while building

The first version of the poll loop waited for `status === 'completed'`. The
contract's terminal success is **`done`** (`render_jobs.rs: JobStatus::Done`), so
that loop would have spun until its 30-minute timeout. Found by reading the enum
before trusting the existing preview code, which already used `done`.

## Verification

| check | outcome |
|---|---|
`node --test tests/session-backends.test.js` | 6 pass / 1 skipped — new cases assert the POST body (`expected_revision`, `voicebank`, `track`, `engine`), that the loop **polls to terminal** (2 polls, not 1), that a `failed` job rejects instead of returning silence, and that no job starts when no usable bank is installed |
`node --test tests/` (whole clone) | 78 tests, **75 pass, 0 fail**, 3 pre-existing skips |
`npm run build` | ✓ built in 2.63 s |
end-to-end against the real `synth-server` | imported `golden-the-end-of-hutsune-miku.ustx` through the same backend module the UI uses, then `renderTrack(1)` with **no voicebank argument** (the menu path): 10 710 837 samples @ 44.1 kHz = 242.88 s, peak 0.6372, rms 0.0354, 0.6 s wall — peak matches the CLI's track-1 render exactly |

The end-to-end run drives `src/session-backends.js` itself, not a reimplementation,
so it exercises the code the File menu calls.

## Not done (deliberately, per the user's ordering)

- **The phoneme picker.** Left for afterwards, as asked.
- The preview path still renders track 0 only, and still falls back to an
  exported-USTX render in the browser pool. Switching it to the session render
  job is the next contract-conformance step; it is not done here.

## Repository checkpoint

Branch `wasm-native-rust`, base `e77cc1a6`. **Not committed** — the user has asked
repeatedly that nothing be committed. Files touched by this task are only the
three under `ui-mock/react-dock/src/` plus its test file; the rest of the dirty
tree is unowned by this task.