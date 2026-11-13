# T019 — Close OpenUtau Web UI workflow gaps

**Status:** active · **Owner:** Codex · **Last updated (UTC):** 2026-10-06T13:26:36Z

## Goal

Bring the Web OpenUtau clone (`ui-mock/react-dock`) closer to OpenUtau's
everyday workflows while keeping all project/media/render state authoritative
in Rust. This task implements the UI side and coordinates with engine work
already tracked in T014, T015, T017, and T018; it does not replace those task
records or freeze the draft Project Session v2 extension.

## Completion criteria

- [x] Export selected track as a downloadable WAV from the current Rust session.
- [x] Export whole-project stereo mixdown through a Rust-owned render operation
      shared by direct WASM and HTTP hosts; the frontend only downloads the
      engine artifact.
- [x] Import audio into a Rust-owned media source and create a wave part through
      the draft session command path; persist bytes in browser OPFS and restore
      them into either active host. Waveform rendering/display is provided by
      the T017 path.
- [x] Enable track operations that have shared Rust command semantics (pan,
      track add/remove/reorder, and connected track settings) in the Web UI.
- [x] Keep unavailable parity controls visibly disabled until both active host
      paths implement them; record any remaining gaps and verification.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`
- The checkout contains extensive pre-existing dirty and untracked work. This
  task owns only its recorded UI/docs deltas; do not claim or revert unrelated
  working-tree changes.
- Related active work: T014 (session command gaps), T015 (media/mixdown), T017
  (wave audio UI/render), and T018 (flags/vibrato/FX).

## Progress

### 2026-10-06T09:35:03Z — track WAV download

- Added a File menu action labeled `Export track N as WAV…`.
- It renders the selected track from the engine, preserves both returned
  channels, and packages those samples as a browser-downloaded stereo PCM16
  WAV. This is only a host file-save adapter; it does not calculate musical
  state or mix tracks in the frontend.
- `npm run build` in `ui-mock/react-dock`: PASS (existing missing external
  asset/model and large chunk warnings only).
- Tests were not added or run.

### 2026-10-06T10:53:00Z — mixdown, audio import, and track controls

- Added one Rust whole-project stereo mixdown path shared by Direct WASM and
  native HTTP. The UI starts/polls the HTTP render job or calls the WASM method,
  then downloads returned stereo samples; it does not sum tracks in JS.
- Added browser audio import: supported audio bytes are validated/decoded by
  Rust, stored in OPFS under an opaque media id, registered with the active Rust
  media source, then referenced by a `part.import_audio` session command.
  Project open re-registers the persisted bytes. The server currently retains
  uploaded media in process memory, so server restart persistence is a known
  integration gap.
- Added `track.create`, `track.remove`, and `track.reorder` session commands and
  connected Add/Move/Delete, pan, volume, mute/solo, and FX controls. The
  unsupported track-settings gear and user vibrato preset controls remain
  visibly disabled.
- Added USTX track merge through a Rust `project.import_tracks` command and
  recorded the unratified command/route shapes in
  `docs/planning/contract-v2-extension-t019-media-mixdown.md`. The frozen §6
  contract was not changed by this task.
- Browser media now survives refresh in OPFS; this host storage remains
  separate from portable USTX. Projects imported from elsewhere with unknown
  `asset:` refs report the missing browser asset on open.
- Verification: `npm run build` PASS; `cargo check -p project-session -p
  synth-server --features synth-cli/cli` PASS; `cargo check -p engine-wasm
  --target wasm32-unknown-unknown` PASS; release WASM build and wasm-bindgen
  completed, with generated bindings exposing `session_register_media` and
  `session_render_mixdown`. Existing missing optional asset/model and large
  chunk warnings remain. Tests were not run.

## Remaining work

### 2026-10-06T11:56:04Z — real USTX in the visible Web editor

- Opened `test-ustx-example/golden-the-end-of-hutsune-miku.ustx` through File →
  Import USTX in the Codex sidebar browser. The document loaded in both Direct
  WASM and HTTP session modes with 5 tracks and 1,709 notes; the selected part
  reports 319 notes. No import error was shown.
- In HTTP mode, Add Track changed the count to 6, Undo returned it to 5, and
  Redo returned it to 6. A final Undo restored the displayed project to 5
  tracks. Revisions advanced through the commands.
- Rendering is **not a valid full-bank acceptance result**: this run used the
  small development fixture bank, which only mapped 6 of 65 phonemes for the
  selected phrase; HTTP reported 0/12 phrases rendered. The workspace contains
  only the fixture bank and a Teto English test bank, not the Japanese full
  bank referenced by this project. This is the engine/bank coverage gap the
  user called out, not evidence that the project lacks notes.
- The USTX's wave part points to a user Downloads MP3 path outside the project.
  The browser import did not upload those external bytes, so wave-part playback
  cannot be claimed from this run. Stereo mixdown was invoked but no download
  event arrived within the 60-second observation window; treat it as
  **unverified**, not as a confirmed engine failure.
- Manual browser acceptance: import in Direct WASM and HTTP **PASS**; track
  add/undo/redo in HTTP **PASS**; full-bank voice render, external wave media,
  save/reopen of this real file, and completed mixdown download **UNVERIFIED**.
  The browser remains open on the imported file in the HTTP session.
- Next: repeat render/mixdown with the Japanese full bank installed in the
  browser/server session and the referenced MP3 made available via the media
  import flow. Also complete save/reopen acceptance for this USTX.

### 2026-10-06T13:26:36Z — wire ZIP voicebank import and track Apply

- The supplied `TETO-OUset240323.zip` contains an empty root `oto.ini` and
  multiple non-empty subbank OTO files. The previous ZIP flow installed the
  WAVs but read only one OTO, while `installedRoots()` failed to discover a
  bank whose usable OTO lived below the root.
- `OpfsBank.openAllOtos()` now merges every non-empty OTO set and prefixes its
  sample paths with the original subbank directory. Both ZIP/folder import
  summaries and direct WASM lazy sample loading use the full bank. Installed
  root discovery now checks nested non-empty OTO files.
- Replaced the Singer dialog's disabled “Apply (coming later)” footer with an
  Apply-to-active-track action. It dispatches `track.set_singer` through the
  Rust session and no longer displays success if the engine rejects the
  command. The Rust v2 document router previously treated this command as an
  unsupported generic `track.*` operation before its legacy implementation;
  it now plans the mutation and emits a track-scoped change.
- Manual sidebar check (Direct WASM): installed bank appeared in the Singer
  dialog; clicking Apply showed “Applied … to Track1”, advanced session
  revision 5 → 6, and updated the engine-backed Track1 singer field. Reopening
  the dialog showed the selected installed bank and disabled Apply because it
  was already applied. The dialog now says this explicitly and its button help
  explains that the singer is already applied. The browser now stays open on
  this state.
- `cargo test -p project-session --test track_singer`: PASS (7 tests).
  `RUSTFLAGS='-C target-feature=+simd128' cargo build --release --target
  wasm32-unknown-unknown -p engine-wasm`: PASS. `wasm-bindgen --target web
  --out-dir web/pkg native/target/wasm32-unknown-unknown/release/engine_wasm.wasm`:
  PASS. `npm run build` in `ui-mock/react-dock`: PASS (existing missing
  optional asset/model and large chunk warnings only).
- A fresh Direct WASM session with the installed bank discovered it, but its
  four-note starter part still reports partial OTO mapping (1/7 phonemes).
  That starter phrase is not the supplied Japanese song; full-song render and
  the merged OTO alias/sample counts therefore remain unverified in the
  browser. The attempt to reopen the USTX import chooser in the fresh tab did
  not produce a file chooser, so no new USTX import result is claimed here.

Next: verify the imported OTO/sample counts and render the supplied USTX in
Direct WASM using the installed full bank, then repeat mixdown and save/reopen.
The earlier HTTP server run did load the native bank scanner (6,216 aliases,
857 WAVs at 44.1 kHz); that does not substitute for the outstanding browser
full-bank acceptance.

Decide whether HTTP media uploads need durable server-side storage across
process restarts; that is outside the current browser OPFS persistence path.

## Follow-up (2026-10-07T08:46:02Z) — full-bank multitrack check

Ran a real full-project mixdown of `golden-the-end-of-hutsune-miku.ustx` with
the supplied extracted Teto bank and the MP3 referenced by the USTX. Tracks
0–3 rendered 20/8/71/54 phrases (153 total); track 4 rendered one wave part.
The result was stereo, 44.1 kHz, 288.3918 s. A Direct-WASM browser retest also
rendered the `harm` singer preview (258.9 s) and the relinked Track 5 audio
preview (288.4 s).

The imported USTX names its singer by its bank-folder id, while the current
browser's Singer picker shows that id as “not installed” and separately lists
the installed bank under a different display/root name. Direct-WASM preview
still rendered via its first-installed-bank fallback in this single-bank
session, but choosing the correct bank is ambiguous when several banks are
installed. Track order itself matches the USTX `track_no` values (voice 0–3,
wave 4); the four source tracks also all carry the literal name `Track1`, which
explains the repeated labels rather than indicating that parts were reordered.

The complete browser **Export mixdown** download remains unverified. The CLI
PCM16 output clipped 5,373 samples (0.0211%) from an internal peak of 1.8682;
review master headroom/limiting separately from track loss. `cargo test -p
synth-cli --test mixdown_wave_parts` passes 10/10 after its old mono-output
assertion was updated for interleaved stereo.
