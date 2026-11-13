# T020 — Arrangement sizing and WASM worker isolation

**Status:** done  
**Owner:** Codex  
**Last updated (UTC):** 2026-10-07T13:08:18Z

## Goal

Make the Web clone's Arrangement mixer column resizable, align each track card
with its timeline lane, move Direct-WASM session/render processing off the browser
UI thread, keep playback and all views on the engine playhead, and show audio-wave
and synth-note mini-previews inside Arrangement parts.

## Completion criteria

- [x] Arrangement header width can be adjusted with keyboard; pointer drag is wired.
- [x] Track headers have the same row geometry and synchronized vertical scroll as
  the corresponding timeline lanes.
- [x] Direct-WASM session/WASM render and PCM analysis run in a Web Worker; UI
  receives snapshots and audio results through a narrow RPC adapter.
- [x] Browser UI remains responsive during a long render of the supplied USTX
  with the supplied voicebank.
- [x] Playback streams through AudioWorklet and produces audio; live Direct-WASM
      playback advanced beyond both 14 s and 28 s (clock 01:14.388 at 74.4 s,
      AudioWorklet tick 142,886 at 240 BPM).
- [x] Transport clock follows AudioWorklet output frames; Arrangement, Piano
      Roll and Expression Lanes displayed the moving project playhead together.
- [x] Play renders the full project mixdown and caches it by session revision and
      voicebank epoch; replay began immediately without `Rendering project mixdown…`.
- [x] Arrangement draws an audio waveform for wave parts and pitch/time note
      miniatures inside synth parts; waveform-only worker RPC does not retain or
      replace the selected-preview PCM take.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base/HEAD: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`
- The checkout contains many unrelated modified and untracked paths. The whole
  `ui-mock/react-dock/` and `docs/work/` trees are untracked in Git. Preserve
  unrelated work; attribute only the T020 deltas described in this record.
- This continuation's owned implementation/test deltas are in
  `ui-mock/react-dock/src/openutau/arrangement-geometry.js`,
  `ui-mock/react-dock/src/openutau/arrangement-panel.jsx`,
  `ui-mock/react-dock/src/openutau/clone-state.jsx`,
  `ui-mock/react-dock/src/openutau/transport-state.js`,
  `ui-mock/react-dock/src/openutau/wasm-session.worker.js`,
  `ui-mock/react-dock/src/session-backends.js`,
  `ui-mock/react-dock/tests/arrangement-geometry.test.js`,
  `ui-mock/react-dock/tests/audio-playback.test.js`,
  `ui-mock/react-dock/tests/session-backends.test.js`, and
  `ui-mock/react-dock/tests/transport-state.test.js`. Prior T020 worker,
  playback, and layout work remains in the same untracked tree; do not claim
  neighboring files or unrelated changes.
- Durable artifacts: this record, `docs/work/BOARD.md`, and the listed T020
  source/test deltas.

## Progress

- 2026-10-07T13:04:41Z Live acceptance and Arrangement-preview completion at
  `http://localhost:4187/?backend=wasm`: reimported the supplied USTX after HMR
  reset, with 5 tracks / 1,709 notes / 240 BPM, and relinked its off-vocal via
  File ▸ Relink Audio. `renderTrack(4, { waveformOnly: true })` returned 44.1 kHz,
  12,718,080 frames (288.4 s) and envelope peaks. At Arrangement scrollX=10,500,
  browser screenshots showed the audio waveform and pitch/time note miniatures
  inside synth blocks. Full mixdown took about 60 s, then live playback advanced
  past 14 s and 28 s: at UI `01:14.388`, AudioWorklet tick was 142,886 at 240 BPM;
  later it continued past 2 min. A live screenshot showed the same moving
  playhead in Arrangement, Piano Roll and Expression Lanes. Stop/replay began
  immediately with `Pause (Space)` and no `Rendering project mixdown…`, confirming
  cache reuse. The UI stayed responsive during render; meter samples were −11 to
  −19 dB, with no clipping observed, but no true-peak measurement was taken.

- 2026-10-07T08:57:09Z Render/play wiring audit: the connected preview effect
  calls `engine.renderTrack(activeTrackNo)` and stores one active-track take;
  the AudioWorklet player consumes that take. The whole-project Rust
  `renderMixdown()` path is wired to File ▸ Export mixdown, not to Play. The
  WASM worker retains only `lastTake`, and `session_render_mixdown` creates a
  fresh temporary `RenderCache` per call (`let mut cache = None`), so there is
  no cross-track render queue or persistent rendered-audio cache for transport.
  The pool's chunk cache is bypassed by the Direct-WASM preview branch.
  On this USTX, first vocal notes start around 106.9 s while the transport stops
  at 28 s; that explains silence on an active vocal track before notes begin.
  Track 5's relinked audio preview does produce nonzero output, as noted below.
  Next: make Play render/cache the project mix (or per-track cached buses), and
  fix the independent 28-second clock stall; keep track-only preview for the
  waveform strip.

- 2026-10-07T08:46:02Z Follow-up with the real USTX/MP3: playback now produces
  changing master output (−33.2 → −13.7 → −16.7 dBFS), but repeated reads
  confirmed the transport display is frozen at exactly `00:28.000` while Play
  remains active. This narrows the remaining failure to transport clock/playhead
  progression rather than a silent singer or mixer path. Track 1 preview renders
  258.9 s and Track 5 audio preview renders 288.4 s. See T017 follow-up for the
  full multitrack CLI mixdown and clipping measurement.

- 2026-10-07T09:05:26Z The AudioWorklet now reports its played frame every 512
  samples, and `tickNow()` reads that frame directly instead of estimating from
  the AudioContext clock. Arrangement now draws and follows the global playhead;
  Piano Roll follows the same global tick and its scroll range includes the
  active part's absolute start; Expression Lanes map the global tick to the
  active part's local curve time, then draw/follow it on the same beat scale as
  the roll. The clock chip now converts ticks using the engine tempo rather than
  the fixed 120 BPM UI default. `npm run build` passes. The currently visible browser session has a
  starter phrase render failure (partial OTO mapping, 1/7 phonemes), so this
  change has not yet been manually verified by playback past 28s.

- 2026-10-07T09:19:38Z Direct-WASM Play now calls the Rust whole-project stereo
  `renderMixdown()` path, caches the worker-owned take for the current
  session/revision/bank epoch, and reuses it for replay. The WAV preview remains
  per-track. The worker now retains one track take and one mixdown take instead
  of overwriting all render results through one `lastTake`, so rendering the
  preview cannot invalidate the transport buffer. When a wave part still has a
  source-machine path instead of a browser `asset:` reference, playback shows
  an actionable File → Relink Audio warning; the mixdown still renders the
  other available tracks. The worker cache retains only stereo channel buffers
  for playback, not a third mono PCM copy. `npm run build` passes. Full browser
  playback with this supplied USTX and relinked MP3 remains unverified.

- 2026-10-07T10:06:09Z Isolated browser reproduction with the supplied USTX
  imported five tracks (four voice tracks and the off-vocal wave track). After
  relinking the MP3, the master meter remained nonzero while the clock stopped
  at `00:14.000`; this rules out the project being truncated to the selected
  track, but does not prove the singer tracks rendered (that browser profile
  had no installed voicebank). The tested page then became inaccessible under
  browser security policy, so no workaround was attempted. AudioWorklet frame
  reports now anchor the clock and `AudioContext.currentTime` interpolates
  between reports, preventing a delayed report from freezing the visible
  playhead. Transport/status labels now distinguish full-project mixdown from
  selected-track waveform preview failures. `npm run build` passes; browser
  playback verification of this change is pending.

- 2026-10-07T10:57:59Z Re-ran the attached project with the supplied Teto bank
  through the native engine/CLI. Track 0 rendered 20 phrases with 0 skipped
  (258.9 s, peak 0.9076). Whole-project `mixdown` reported track 0: 20 phrases,
  track 1: 8, track 2: 71, track 3: 54, and track 4: one wave part; stereo output
  is 288.4 s. The WAV is at `/tmp/golden-the-end-mixdown.wav` (peak 1.8682 before
  optional master limiting). This confirms the engine can render all five
  project tracks with the attached bank and wave part. Browser retry on
  `127.0.0.1:4187` is still blocked by the saved browser permission setting,
  separate from filesystem full-access; no alternate browser access was used.

- 2026-10-07T11:15:34Z Resumed T020 and added `tests/audio-playback.test.js`
  to protect the worker-clock interpolation from the reported 14-second freeze:
  it injects a stale AudioWorklet progress report at 14 s, advances the
  AudioContext to 29.5 s, and asserts the playhead is past 28 s. The focused
  test passes. This is deterministic clock logic coverage, not real
  browser/audio verification. Full suite: 81 pass, 2 existing failures, 3
  skipped; build passes. The browser-tool permission/tab inventory mismatch
  remains. Do not switch browser surfaces or use shell/CDP to work around it;
  continue when the tool can see the allowed tab.

- 2026-10-07T11:08:51Z User shared the Agent permissions screen showing
  `http://127.0.0.1:4187` set to Always allow for Browsing and Downloads. A
  fresh browser-tool attempt still reports a saved user permission block, and
  its inventory reports no tabs even though the user-visible browser has one.
  This is a browser permission propagation/inventory mismatch. Do not switch
  browser surfaces or use shell/CDP to work around it; continue when the tool
  can see the allowed tab. User requested the remaining work be handed off.

- 2026-10-07T05:00:21Z Read board and inspected arrangement layout, session
  adapter and render path. Arrangement uses fixed 220px header, unconstrained
  card height, 92px timeline rows and no vertical scroll synchronization.
  Direct WASM boot/render and stereo conversion run on the UI thread; waveform
  peak extraction scans the rendered PCM there as well.
- 2026-10-07T05:12:31Z Arrangement mixer width now ranges 160–420 px, is
  keyboard-adjustable and persisted in localStorage. Track cards are fixed at
  136 px to match Rust-timeline lane geometry (plus the shared 28 px ruler), and
  vertical scrolling is mirrored between header/timeline panes. Manual DOM
  measurement with the supplied five-track project confirmed rows at y=98, 234,
  370, 506, 642 (136 px apart); header restored to 220 px.
- 2026-10-07T05:12:31Z Direct-WASM SessionDriver, voicebank preparation, render,
  stereo split and peak extraction now run in `wasm-session.worker.js`; UI uses
  RPC and latest engine snapshots. Rendered PCM stays worker-owned until play,
  then transfers to `audio-take.worklet.js`. Removed duplicate play scheduling.
  Installed supplied bank via UI: 3,919 files, 13 OTO sets, 6,216 aliases,
  1,822 samples. Supplied USTX imports as 5 tracks / 1,709 notes; active vocal
  track rendered 258.9 s from Rust WASM while the UI remained responsive.
- 2026-10-07T05:12:31Z Earlier playback issue reproduced: after starting the
  rendered take, transport display stopped at `00:28.000` while the play control
  remained active; the master meter reported `−100 dB`. This historical failure
  is superseded by the 2026-10-07T13:04:41Z live browser acceptance at the top of
  Progress, where the clock advanced beyond 28 s and all views followed.

## Verification

- `node --test` from `ui-mock/react-dock` — 84 pass, 1 fail, 3 skipped. All
  T020-focused cases passed, including arrangement note geometry, transport
  bounds beyond the mock 14-measure limit, AudioWorklet interpolation past 28 s,
  and waveform-only Direct-WASM worker RPC. The sole failure is the unrelated
  pre-existing `tests/openutau-engine.test.js:52` contract mismatch: it expects
  `null`, but current code returns `[{ type: 'track.create', name: 'New Track' }]`.
  Three live-server cases are skipped unless `RUSTED_UTAU_SERVER_URL` is set.
- `npm run build` (from `ui-mock/react-dock`) — PASS; existing optional asset
  resolution and >500 kB chunk warnings only.
- Browser: visible Direct-WASM preview and playback acceptance is recorded in the
  latest Progress entry above; clock passed both 14 s and 28 s, all three views
  showed the moving playhead, and replay reused the full mixdown.
- Browser output meter samples ranged from −11 to −19 dB with no observed
  clipping. No true-peak measurement was captured; the earlier native CLI peak
  1.8682 before optional limiting remains a separate historical measurement.

## Blockers and dependencies

- No remaining blocker for T020; browser access worked in the visible agent
  window at `http://localhost:4187/?backend=wasm`.
- The full suite still has one unrelated contract-test mismatch at
  `ui-mock/react-dock/tests/openutau-engine.test.js:52`; it is not caused by the
  T020 changes and was left untouched to avoid widening scope.
- Browser meter samples showed no clipping, but this run did not capture a
  true-peak measurement. The native CLI peak 1.8682 before optional limiting is
  historical evidence, not a measurement of the current browser output.

## Next step

- None for T020; completion criteria are met. If separately scoped, reconcile the
  `track.create` test/command contract and capture a true-peak measurement for
  the full mixdown.

## Handoff notes

Rust remains the only authority for document state. The worker is a host
adapter: it owns the WASM Engine and ProjectStore runtime; React consumes
disposable snapshots and sends the same Rust commands through RPC.

T020 is complete. Latest live acceptance is on `http://localhost:4187/?backend=wasm`;
the imported USTX and relinked off-vocal are visible in Arrangement, and the
installed Teto bank remains available in OPFS. If HMR or a reload resets the
project, reimport the supplied USTX and use File ▸ Relink Audio for its MP3.
The full-mix cache remains keyed by session ID, revision and voicebank epoch.
Do not call the entire Node suite green: its remaining `track.create` assertion
mismatch is recorded above and is outside T020.
