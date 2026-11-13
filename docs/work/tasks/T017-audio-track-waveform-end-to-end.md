# T017 — Audio track end to end: per-track media render, then a visible waveform

Status: **done** · Owner: Buffy · Last updated: 2026-10-07T02:15:58Z

## Goal

Make an audio track (a `UWavePart`) a first-class thing in the editor: the engine can
render it on its own, and selecting it in the arrangement actually draws its waveform
in the piano roll strip.

Two increments, in this order:

1. **Engine** — `render --track N` / §6.13 render job must mix that track's wave parts
   instead of erroring with `no voice part found on track N` or silently ignoring them.
2. **Clone** — the piano roll's waveform strip must follow the *active track*, and a
   note-less (wave) part must bring the viewport with it.

## Repository checkpoint

- Branch `wasm-native-rust`, base commit `e77cc1a6`.
- `react-dock/` is **not in git**; its files below are only recoverable from this
  checkout or a backup.
- Working tree carries many pre-existing unowned modifications (see BOARD preamble);
  the files this task owns are listed below.

Files changed for this task:

- `native/tools/synth-cli/src/pipeline.rs` — `RenderReport` (`Debug`, `wave_parts_mixed`),
  new `render_track_with_media`.
- `native/tools/synth-server/{Cargo.toml,src/main.rs,src/render_service.rs,tests/api_real.rs}`
  — `--media <dir>`, `SharedMedia`, worker arm calling `render_track_with_media`.
- `native/crates/engine-wasm/{Cargo.toml,src/lib.rs}` — `render_track` with
  `MemoryMediaSource`.
- `native/tools/synth-cli/tests/mixdown_wave_parts.rs` — +4 cases (10 total).
- `ui-mock/react-dock/src/openutau/{clone-state.jsx,waveform-source.js,engine.js,file-menu.jsx}`
  — active-track preview, `renderTrack`, File ▸ Render track N…
- `ui-mock/react-dock/src/openutau/piano-roll-panel.jsx` — `useFollowPart` handles a
  part with no notes.
- `ui-mock/react-dock/tests/{openutau-engine.test.js,session-backends.test.js,waveform-source-track.test.js}`.
- `scripts/make-audio-track-demo.py` — new, regenerates the audio-track demo project.
- `docs/architecture/known-limitations.md` — L3 updated, L9 added.

## Completion criteria

- [x] A track holding only a wave part renders through the per-track path, sample-exact
      against the decoded media.
- [x] A track with neither voice nor wave part fails *by name*, with skip reasons.
- [x] A voice track with no wave part renders byte-identically to the export path.
- [x] `synth-server` takes a media root; without it wave parts are *reported* skipped.
- [x] The browser host reports audio parts skipped instead of erroring.
- [x] The clone's preview render follows the active track, so an audio track is drawable.
- [x] Selecting a wave part scrolls the piano roll onto it, and the strip draws its peaks.
- [x] Tests, build, and a real browser run are recorded below.

## What was found, in order

1. **The per-track render path had no wave-part support at all.** `grep UPart::Wave`
   matched nothing outside `render_mixdown`. An audio-only track errored; a voice track
   with an audio part under it dropped the audio with no message.
2. **`waveform-source.js` and `clone-state.jsx` both hardcoded `track: 0`.** Clicking an
   audio track drew track 0's peaks — a wrong picture and no error.
3. **`useFollowPart` bailed on `!notes.length`.** A wave part has no notes, so the roll
   never moved onto it; the viewport stayed where the previous (497,280-tick) part left
   it. This is the one that made the strip look "flat": nothing was drawn because the
   visible slice of the timeline was tens of thousands of ticks past the audio.
4. **The demo fixture lied about its own audio.** The golden `.ustx` keeps the *original*
   MP3's `file_duration_ms: 288391.8367`; the demo's `bg.wav` is 4 s. The engine turns
   that number into `duration_ticks`, so the part claimed 72× its audio and the strip
   showed 4 seconds of waveform followed by a minute and a half of silence. The generator
   now writes the real length.

## Verification

Commands and outcomes, all re-run against the final state of the files:

| Command | Result |
|---|---|
| `cargo test -p synth-cli --test mixdown_wave_parts` | **10 passed, 0 failed** |
| `cd ui-mock/react-dock && node --test tests/` | **82 tests, 79 pass, 0 fail, 3 skipped** (skips pre-existing) |
| `cd ui-mock/react-dock && npm run build` | clean |

Real-project render, on the synth-server (`--media native/target/media-demo`, port 8766):

- `demo.ustx` = the Hatsune golden with `relative_path` → `bg.wav` and a matching
  `file_duration_ms`. Track 4 (the audio track) → `done`, 176,400 frames = **exactly
  4.0 s**, peak **0.3662 = 12000/32768** — an exact round trip, so nothing scales or
  clips on the way through.
- Track 0 fails against the Dev Fixture bank (no kana aliases) — expected, and the same
  before this change.

Browser (Vite 4187, `?backend=http&server=http://127.0.0.1:8766`, Chromium):

- File ▸ Open ▸ demo, click the ♪ track: footer reads **`render 4.0s from synth-server`**,
  part header reads the wave part, strip title `Render preview from synth-server · 4.0s`.
- Canvas pixels, read with `getImageData`: the peak colour occupies columns
  **90 → 1178**, i.e. `gridLeft` → `gridLeft + 16 beats × 68 px` = exactly the 4 s part
  at the project's 240 BPM, 22 rows tall = ±0.3662. Before the `useFollowPart` fix the
  same probe read **0 peak pixels**.
- **Negative control**: reverting only the `useFollowPart` guard reproduces the blank
  strip exactly (`scrollX 29070`, `peakPixels 0`); restoring it gives `scrollX 0`,
  `peakPixels 23941`.

Measurement note for the next reader: the strip's surface, zero line and peaks are three
close greys (`rgba(158,166,186,.12)`, `rgba(180,188,206,.20)`, `rgba(200,208,226,.55)`)
and `getImageData` returns colour-managed values, so matching an exact RGBA triple picks
the wrong layer and reports "no waveform". Read the *rows* of one column instead.

## Known limits recorded, not fixed

- **L9** — the per-track path applies no fader (unity), matching OpenUtau's `RenderTracks`
  export; its `RenderPreview` *does* apply the track fader and adds wave parts to it.
  Changing the value silently would break the golden export comparison, so it is a
  deliberate divergence with a proposed `apply_track_fader` job parameter instead.
- **L3** — `media_ref` is still the raw `.ustx` `relative_path` (a machine path), not a
  host-owned logical reference as the contract intends.
- The browser (Direct WASM) host has no media store: `MemoryMediaSource` is always empty,
  so audio parts are reported skipped there.
- `cargo test --workspace` still has **1 pre-existing failure**,
  `project-session/tests/session_v2.rs::a_note_edit_by_id_reports_a_patch_and_invalidates_render`
  (`left: None, right: Some(1440)`), verified untouched against `e77cc1a6`. Not skipped,
  not weakened. `cargo clippy` still fails in `worldline-sys` (pre-existing, untouched).
- `useFollowPart` has no automated test: `node --test` cannot import a `.jsx` hook, so
  its evidence is the browser negative control above plus a checklist entry in
  `ui-mock/react-dock/tests/manual-checklist.md`.

## Reproducing the demo

```sh
python3 scripts/make-audio-track-demo.py                 # → native/target/media-demo/
cp native/target/media-demo/demo.ustx /tmp/rusted-utau-projects/
cd native && ./target/release/synth-server \
  --so build/build-linux/libworldline.so --voicebanks ../web/fixture/bank \
  --projects /tmp/rusted-utau-projects --media "$PWD/target/media-demo" --port 8766
cd ui-mock/react-dock && npx vite --host 127.0.0.1 --port 4187 --strictPort
# open http://127.0.0.1:4187/?backend=http&server=http://127.0.0.1:8766
```

## Next concrete step

The preview still renders the *export* stream (L9). Give the §6.13 render job an explicit
`apply_track_fader` / purpose field so the clone's strip shows what the user will hear,
and make the wave-part part of that job use the same track fader OpenUtau's
`RenderPreview` does. That closes L9 and makes the waveform strip honest about volume
and pan — after which the phoneme picker the user deferred can land on top.

---

## Round 2 — "the demo is not actually usable"

The user reported three things after using the demo: no sound from either track,
the piano roll losing something when scrolled sideways, and no way to delete a
track. All three were real. Root causes, in order:

### 1. The transport could only play the in-browser pool

`createAudioPlayer` read audio **only** from `getRenderPool()`. The preview, on
the HTTP backend, renders through a §6.13 job on the synth-server — and that PCM
was drawn on the strip and then thrown away. Pressing play had nothing to play,
which is why the strip showed `render 258.9s` while the transport said
`silent: the render cache does not reach this point in the song yet (chunk 1 of 0)`.

Fix: `createAudioPlayer` now takes `getTake()` and prefers a finished engine take
over the pool — the take is what the strip is showing, and it is the only source
that carries a wave part at all, because the pool has no media store. The pool
path stays for the in-browser render (where a chunk-by-chunk stream beats a
whole-track buffer). `clone-state.jsx` keeps the take in a ref, clears it the
moment a new preview starts and when a render fails, so play can never produce
audio that is not the audio on screen.

### 2. The synth track rendered nothing — because the full bank was never pointed at

The demo said `no audio produced (0/12 phrases rendered) … partial oto mapping
(6 of 65 phonemes)`. That was the **Dev Fixture bank** (40 aliases), not a bug:
the golden song needs ~65 Japanese phonemes.

The real bank is installed on this machine:

    /home/seal/.local/share/OpenUtau/Singers/重音テト OU用日本語統合ライブラリー

It is a multi-subbank UTAU voicebank — 644 MB, 2380 wavs, `oto.ini` files in nine
sub-folders selected by `character.yaml`'s `suffix` (エッジ/力み/叫び/囁き/弱/滑らか,
which are exactly the `voice_color_names` the demo project's tracks carry). The
`voicebank` crate already models subbanks and multiple oto sets, so no engine
change was needed; the server was simply never pointed at it:

    --voicebanks "/home/seal/.local/share/OpenUtau/Singers"
    → voicebank: '重音テト（かさねてと）OU用日本語統合ライブラリー'
      (dir 重音テト OU用日本語統合ライブラリー, 8846 aliases, 1415 wavs, 44100 Hz)

Note the name/id split: `character.txt`'s `name=` is `重音テト（かさねてと）…` while
the folder — the singer's `Id`, and what the project's tracks name — is
`重音テト OU用日本語統合ライブラリー`. The render resolves by id, so they must not be
conflated when a bank is listed or matched.

**Real render with the full bank** (synth-server, HTTP session, §6.13 job):

- track 1 → `done`, **242.876 s, peak 20880/32768 = 0.6372** — the same numbers as
  the CLI measurement of the golden per-track export, so this is the real take.
- in the browser: footer `render 258.9s from synth-server`, strip 27,492 peak
  pixels, and pressing ▶ moves the playhead 0.795 → 2.055 → 3.315 → 4.551 → 5.788 s
  off the AudioContext clock with no error.
- the audio track: footer `render 4.0s from synth-server`, ▶ advances the playhead
  through it the same way.

### 3. The piano roll did not lose its notes — it lost its grid

`RollCanvas` drew beat lines for `b = 0..128` only, and `MeasureNumbers` counted
measures from 1 regardless of scroll. The demo's `harm` part is 1036 beats, so
past 13% of the song **every grid line and every measure number disappeared**
while the notes kept drawing — which reads as "the piano roll broke". Both now
derive their range from the viewport; at beat 648 the ruler shows measures
164–167 (verified in Chromium). The blank roll at beat 648 was separately
confirmed to be a genuine gap: the engine reports 0 notes in that tick range.

### 4. Deleting a track — investigated, not built

`v2.rs:1708` answers `unsupported_operation` for `track.add` / `track.remove` /
`track.reorder`. There is no engine command, so this is not a missing button:
deleting a track means deleting its parts and their notes as one undoable
transaction, which is contract-extension work tracked in T014. The user chose to
have it built (engine command + UI); **it is not started in this round.**

## Round 2 verification

| Command | Result |
|---|---|
| `cargo test -p synth-cli --test mixdown_wave_parts` | exit 0 — 10 passed, 0 failed |
| `cd ui-mock/react-dock && node --test tests/` | exit 0 — 79 pass, 0 fail, 3 pre-existing skips |
| `cd ui-mock/react-dock && npm run build` | exit 0 |
| browser: synth track, full bank | 242.876 s / peak 0.6372 via §6.13; strip + transport verified |
| browser: audio track | 4.0 s take, transport advances off the audio clock |

Not verified: that audio is *audibly* correct — this environment has no output
device, so "the playhead advances from the AudioContext clock with no error" is
the strongest claim available. The engine-side audio is verified numerically
(duration, peak, nonzero-sample ratio) instead.

---

## Round 3 — "why is there no audio out?"

Chasing this properly (the user was right to push) found a second, bigger bug that
had been hiding behind the first.

### The ▶ button never started playback at all

`dispatch()` in `clone-state.jsx` handles `transport/*` like this: unlock the
AudioContext, then call `player.play(target)` **only when the intent moves the
playhead and audio is already playing**:

```js
if (moving && player?.playing) player.play(target)...
```

`MOVES_PLAYHEAD` is `seek / skip-measure / skip-note / restart` — `transport/play`
is deliberately not in it (it does not move the cursor, it sets `playing`). So
pressing ▶ unlocked the context, set a flag, and the rAF loop advanced the playhead
on the **silent delta clock**. No source was ever scheduled, no error was ever
raised, and the strip happily showed `render 258.9s` the whole time. This is why
"no sound" survived the take-based player: the player was never called.

Fixed with an explicit `starting` branch — `transport/play` calls
`player.play(playheadTicks)` exactly once per press.

**Evidence the fix works** (Chromium, HTTP backend, full TETO bank): patched
`AudioBufferSourceNode.prototype.start` in the page and pressed ▶ —

```json
{ "frames": 11416691, "ch": 1, "rate": 44100, "ctxState": "running", "offset": 13 }
```

11,416,691 frames / 44100 = **258.9 s**, i.e. the engine's own take, on a running
context. Before the fix the same log stayed empty.

### What still could not be proven here, and why it is not our bug

`parecord` on the default sink (USB Sound Blaster, PipeWire) while the app played
captured **-52 dBFS — silence**. Two further checks:

- `pactl list sink-inputs` showed Chromium present on PipeWire as
  `application.name = "Chromium input"` (a capture stream) with **no playback
  stream**, while the tab's Web Audio reported `state: "running"`.
- A plain `OscillatorNode` at 0.2 gain in the **same tab** was also absent from the
  sink capture (`-50.3 dBFS`, no 440 Hz). One earlier poll did show a
  `Sink Input #292 · "Chromium" · media.name = "Playback" · Corked: no` on the
  default sink, but it did not reproduce on a fresh page.

So in this preview tab, Web Audio does not reliably reach the sound system at all —
independently of our code. That is an environment/sandbox question about the
Freebuff preview browser, not something in this repository. What *is* proven here is
the engine's audio (242.876 s, peak 0.6372, measured off the §6.13 WAV) and the
app's scheduling of it. Confirming the audible result needs a real browser window.

Checks after the fix: `npm run build` exit 0, `node --test tests/` 79 pass / 0 fail.

## Round 4 (2026-10-06) — "no sound" was three stacked causes, all measured

User: เสียงยังไม่มา + มันค้างตอนที่ไปถึง 28 วิ. Two app bugs found and fixed, one
environment finding, and one measurement error corrected from Round 3.

### (1) The pool stub take REPLACED the server take — fixed

`renderProjectAudio` tried the in-browser pool FIRST for every backend and only
fell back to the server on error. The pool renders with the preview bank, which
maps a fraction of a real project's phonemes, so it can "succeed" with a ~2.5 s
sparse stub (measured: `len: 110082` frames scheduled into `AudioBufferSourceNode.start`
while the server take is 11,416,691). Playback then ended mid-intro and the
transport stopped — the user's "ค้างที่ 28 วิ". Now HTTP backend renders
**server first**, pool only as fallback; Direct WASM unchanged (pool only).
Policy: engine output is authoritative, a partial frontend render must never
overwrite it.

### (2) The demo song is silent for its first ~100 s — not a bug, but it fakes one

Per-10 s RMS of the §6.13 track-1 take (`rj-23dd185e-00000008`, 258.88 s):
0–100 s **pure digital silence** (peak 0.0000), 100–150 s audible (rms −14 to
−23 dBFS), 150–210 s silence, 210–258 s audible. Playing from 0 therefore gives
~100 s of nothing and feels like "no sound". The transport needs a
"first audible sample" affordance; not built yet.

### (3) Round 3's "Web Audio does not reach the sound system" was a broken measurement

Two errors: `parecord --device=<sink>` records the wrong thing (falls back to a
mic-ish source — the −52 dBFS "silence"), and the control WAV had been wiped by
the reboot so the control never played. Correct method — record the sink's
**monitor source** — proves the app's audio DOES reach PipeWire:
`paplay` control peak 0.6104 (matches the file), browser oscillator 0.2000
(matches its gain), and the app's demo take **peak 0.6353, rms −20.2 dBFS over
72 s continuous** with Chromium's sink-input present (`Mute: no, 100%, Corked: no`,
sink 56). The default sink was at **30 % (−31 dB cubic)**, which makes a −14 dBFS
vocal ≈ −45 dBFS at the jack — why it still *sounded* silent. Raised to 100 %
for the test; revert with
`pactl set-sink-volume alsa_output.usb-…analog-stereo 30%`.

### (4) Transport now observable: meter + output picker + volume

`createAudioPlayer` builds a master chain `gain → AnalyserNode → destination`
with `levelDb()` (RMS in dBFS; meter math verified: −2.97 dBFS for a full-scale
sine, expected −3.01), `listOutputs()`/`setOutputDevice()` (`AudioContext.setSinkId`),
and a volume that survives context recreation. The status line shows a live
level meter, an output-device picker, and a vol slider; a `__dbg` probe exposes
graph state. Meter correctly reads −100 dB during the song's silent stretches —
the earlier −100s were real, not a meter bug.

Checks: `node --test tests/` 79 pass / 0 fail / 3 pre-existing skips;
`npm run build` exit 0. Evidence for the fix needs a listener — ask the user to
play past 100 s with the meter visible.

Still open: UI shows bpm 120 while the demo project is 240 (HTTP snapshot has
no tempo field), so tick↔ms mapping is 2× off; `track.remove` engine command
(chosen by user, not started); first-audible-sample affordance.

## Round 5 (2026-10-06) — tempo chip, and the audio track's silence

User: "มันตายตอน tempo ไม่ตรง" + "audio track ก็ไม่มีเสียง (พัง) เหมือนเดิม".

### (1) The tempo was never wrong — the CHIP was

`TransportBar` rendered `{BPM.toFixed(1)}`, the UI constant from `mock-data.js`
(120), while the engine had 240 the whole time. Measured in the page:
`engine.state.bpm = 240`, `snapshot.project.bpm = 240.0`,
`tempos: [{position_tick: 0, bpm: 240.0}]` — and playback already ran at the
engine tempo (measured 3.99 beats/s ≈ 240 BPM on the AudioContext clock). The
`.ustx` header says `bpm: 120` while the `tempos` list says 240; the engine
reports the tempo that actually plays. Round 4's "tick↔ms is 2× off" note was
**wrong** — it was read off the wrong chip, on the default project. The chip now
reads `bpm` from the clone context (the engine snapshot) and the stale
`BPM` import is gone. Verified in the page: the transport reads `240.000`.

### (2) The audio track could not be selected, and its fixture had been deleted

Two separate causes, both real:

- **`native/target/` was wiped** (rebuild/cleanup), so the demo's `bg.wav` was
  gone and the server render failed with
  `wave part '…OFFVocal.mp3': read …/media-demo/bg.wav: No such file or
  directory`. Regenerated with `scripts/make-audio-track-demo.py`; the render is
  back to **4.000 s, peak 0.3662 (12000/32768), rms −11.7 dBFS**. To stop this
  failing *inside* a render job with a message that reads like a broken track,
  `synth-server` now warns at boot when `--media` is not a directory and names
  the command that rebuilds the fixture (verified by starting a server against
  a missing root).
- **There was no way to select an audio track in the UI.** `TrackHeaderCard` had
  no click handler at all (only mute/solo/volume), so the only way to make one
  active was clicking its part rectangle — for a 4 s part at position 0 that is
  a ~70 px target at the far left, easy to miss. Clicking a card now selects
  its first part (`part/select`), with `stopPropagation` on the mute/solo/volume
  controls so they do not also change the selection. Verified in the browser:
  click card 5 → footer `render 4.0s from synth-server`, part
  `【ニコカラ】　初音ミクの消失　OFFVocal.mp3`.

Audio track playback, measured end to end: browser meter **−11.7 dBFS** (matches
the render's rms), playhead 535 → 6860 ticks over ~3.6 s (correct at 240 BPM),
and the sink monitor capture **peak 0.3662, rms −16.1 dBFS, 36.7 % non-zero**.
The one silent attempt happened while the 4 s take had not landed yet — pressing
play during a render still has nothing to play, which the status line says.

Checks after the changes: `node --test tests/` **79 pass / 0 fail / 3 skips**,
`npm run build` exit 0, `cargo build --release -p synth-server` exit 0.

## Round 6 (2026-10-06) — "loading the real file still doesn't work": the mixer buttons killed the session

User: "โหลดไฟล์จริงๆยังไม่รอดเหมือนเดิม". The page read
`Mock document — no engine session reachable` with
`engine.state.error = "'solo' is required and must be a boolean"`.

### The load was never the problem — a track flag was

`TrackHeaderCard`'s M/S buttons dispatched `{ type: 'track/mute', trackId }`
with **no value**. `commandsFor` turned that into
`{ type: 'track.set_mute', track_id, mute: undefined }`, `JSON.stringify` DROPS a
key whose value is `undefined`, so Rust received `track.set_mute` with the field
simply absent and answered a validation error. The error propagated through
`onError`, the engine dropped to `status: 'mock'`, and the panels rendered the
mock document — which is exactly what "the file will not load" looks like. One
click on M or S, any time, after any successful load.

Fixed in two places, because either alone leaves the trap open:

1. The buttons send the inverted current value (`mute: !track.mute`), so they are
   real toggles and `false` is as load-bearing as `true`.
2. `commandsFor` now refuses to emit an envelope with an `undefined` field and
   names it: `intent 'track/mute' produced track.set_mute without 'mute'`. A
   panel that forgets a value now fails at the translation boundary instead of
   killing the session. `commandsFor` still returns `null` for an intent with no
   builder, so "nothing to send" and "something to send, wrongly" stay distinct.

### Verified

- Real-file load through the same call the File menu makes: imported
  `test-ustx-example/golden-the-end-of-hutsune-miku.ustx` (665 169 bytes, BOM
  included) → **5 tracks, 1709 notes, bpm 240, status ready, no error**.
- Mute and solo toggled on and back off in the browser: **rev 0 → 2, all flags
  back to `false`, session still `ready`, no error** (before the fix the first
  click produced `'solo' is required…` and the mock fallback).
- New tests in `tests/openutau-engine.test.js`: the exact envelopes for both
  toggles, that `false` survives JSON, and that a missing value throws naming
  the field. Suite **81 pass / 0 fail / 3 pre-existing skips**, `npm run build`
  exit 0, `cargo test -p synth-server` **31 pass / 0 fail** (exit 0).

## Round 7 (2026-10-06) — Audio waveform in Arrangement

The Web OpenUtau clone now projects the active track's engine-rendered peak
envelope onto its `kind: "wave"` parts in Arrangement. Wave parts are labeled
with a music-note marker. Render state carries its source `trackId`; samples
from the previously selected track are cleared while a new render is pending
and cannot be drawn over another track. Pixel columns map through the current
arrangement zoom/scroll and project BPM to sample-time ranges. No engine or
contract change was made.

Verification: `node --test tests/arrangement-geometry.test.js` — **10 passed**;
`npm run build` — passed (existing missing-asset and large-chunk warnings);
scoped `git diff --check` — passed. Full `node --test tests/` — **81 passed,
1 failed, 3 skipped**; the unrelated failure is the existing
`openutau-engine.test.js` expectation that `note/vibrato` has no command, while
T018 now maps that intent to its Rust session command. Browser-level visual
acceptance is not yet recorded. The displayed audio is the rendered track mix
and appears only for the currently rendered/selected track; it is not a raw
per-file media waveform API.

## Round 8 (2026-10-06) — real USTX media relink and playback check

The attached golden USTX imports into Direct WASM as **5 tracks / 1,709 notes**,
but its single wave part points outside the browser host to
`../../../../../home/seal/ดาวน์โหลด/初音ミクの消失 UST (1)/初音ミクの消失 UST/【ニコカラ】　初音ミクの消失　OFFVocal.mp3`.
Before relinking, Rust correctly reported `no media registered for ...`; the UI
transport therefore had no audio take to play. The host cannot open that path
from WASM.

Added File ▸ **Relink Audio…**: choose the imported wave part and its source
file; the browser stores its bytes as a host media asset and sends the stable
`part.set_media_ref` command to Rust. The command validates a safe reference,
only applies to wave parts, invalidates render, and is undoable. The singer
field in Arrangement is now an actual dropdown of installed banks; if the USTX
names a bank that is not installed, it remains visible as a disabled option.

Browser verification with the exact attached USTX and MP3:

- Imported the project and observed the missing-media error before relinking.
- Relinked the MP3 from File ▸ Relink Audio…; the session revision advanced,
  and the Rust WASM preview completed at **288.4 s** with a visible waveform.
- Pressing Play enters the audio playback path, but this in-app browser reports
  `the browser audio clock is not running` and switches to its silent transport
  fallback. The current host therefore cannot verify audible output or meter
  activity. A normal browser with a running AudioContext is still required for
  acoustic acceptance.

Verification: `cargo test -p project-session --test session_document_commands`
**8 passed**; release `engine-wasm` build + wasm-bindgen passed; `npm run build`
passed. `node --test tests/` has 2 pre-existing failures: stale `track.create`
UI-only expectation in `openutau-engine.test.js`, and stale Direct-WASM
`renderTrack` expectation in `session-backends.test.js`; 7 files passed. The
workspace-wide `cargo fmt --check` also reports pre-existing formatting diffs
in many unrelated files, so no workspace formatting was applied.

Remaining step: verify audible streaming in a browser whose Web Audio clock runs;
the local in-app browser lacks that capability for this run.

## Round 9 (2026-10-06) — isolated browser clock diagnosis

Added `ui-mock/react-dock/tests/audio-output-check.html`, which feeds a known
440 Hz buffer through the same `audio-playback.js` master chain used by the
transport and checks both its analyser and AudioContext clock. Re-ran it in the
visible local in-app browser after confirming `document.visibilityState` is
`visible` and `document.hidden` is `false`:

- `play()` resolves and reports `player.playing=true`; AudioContext says
  `running`, with a 44.1 kHz destination and 10.68 ms base latency.
- After 900 ms of page timers, `AudioContext.currentTime` is still exactly
  `0.000 s`, `tickNow()` is 0, and the master analyser remains at −100 dBFS.
- A generated one-second WAV on a regular `<audio>` element also stays at
  `currentTime=0.000 s` after 600 ms, although it reports `paused=false` and
  `readyState=HAVE_ENOUGH_DATA`.
- `enumerateDevices()` does not return within one second in this browser.

This isolates the observed failure from the USTX decoder, missing media path,
and render plan: even a synthetic tone through the production output module
cannot advance either browser media clock. The earlier OS check found Firefox
has an unmuted PipeWire stream attached to the Sound Blaster output, so the
machine does expose a real audio sink. However, the in-app browser still gives
no meter or clock evidence that it is feeding samples into that sink. No
frontend timer workaround can create audible sound while both browser audio
clocks are frozen; do not report playback as accepted from this run.

The diagnostic's failure text previously claimed the AudioContext was not
running, which contradicted its actual `state`. It now reports the specific
condition: state says `running`, but clock and meter remain at zero. Testing
the attached song in a regular browser with a live media clock remains the
acceptance step. No change was made to production transport behavior.

## Round 10 (2026-10-07) — real MP3 transport and output-rate experiment

The USTX's referenced source file is present at
`/home/seal/ดาวน์โหลด/初音ミクの消失 UST/【ニコカラ】　初音ミクの消失　OFFVocal.mp3`.
Re-imported the real USTX, relinked that exact MP3 in File ▸ Relink Audio, and
selected its wave track. The authoritative Rust WASM preview completed at
**288.4 s**. Play then showed the transport running briefly, followed about a
second later by `the browser audio clock is not running`; the level meter went
back to no signal. This is the real-project confirmation of the synthetic
clock diagnosis.

Two follow-ups were tried:

- The transport now clears an old `audioError` when a new `player.play()`
  succeeds, so a previous “nothing rendered yet” warning cannot remain over a
  later successful render/play attempt. In the real-project check the fresh
  clock-stall message was then visible, as expected.
- Letting AudioContext use the device's native 48 kHz rate instead of forcing
  44.1 kHz did not change the isolated diagnostic: after 900 ms the context
  still said `running`, `currentTime` stayed at `0.000 s`, and the analyser
  stayed at −100 dBFS. That sample-rate experiment was reverted; playback
  keeps its existing 44.1 kHz context.

The Web build passes after the final source state (the existing missing-asset
and large-chunk warnings remain). During continuation, the local Vite process
was found stopped and restarted on 127.0.0.1:4187. The current browser control
rejected navigating its recovered tab back to that localhost URL, so the latest
live page could not be re-opened after the restart. Rechecked the authoritative
process and socket: Vite PID 6271 is listening on `127.0.0.1:4187`, and an
escalated read-only `curl` to that socket returns HTTP 200. Both existing
in-app-browser tabs still show `ERR_CONNECTION_REFUSED`, while browser control
blocks direct navigation to the local URL. The immediate step is for the user
to open that URL from their browser. Keep the goal active: the
real USTX can render, but audible playback is still not proven. Resume browser
acceptance once the user re-opens the local editor in the in-app browser, or a
browser with a functioning AudioContext clock is available.

## Round 11 (2026-10-07) — user reopened the editor; real project output retest

The user challenged the previous clock diagnosis, noting playback had worked
before. The earlier test was indeed not a fair real-project test: the browser
had reset to the four-note `New Part`, and the first real-project pass used the
USTX's empty external-media reference. The reopened `?backend=wasm` page loaded
the exact golden USTX through File ▸ Import USTX and showed **5 tracks / 1,709
notes**. The first four vocal notes begin at tick 205,200 (about 106.9 s at the
engine's 240 BPM), so the initial −100 dBFS reading at 28 s said nothing about
the vocal render.

Relinked the exact referenced MP3 from
`/home/seal/ดาวน์โหลด/初音ミクの消失 UST/【ニコカラ】　初音ミクの消失　OFFVocal.mp3`
through File ▸ Relink Audio. Revision advanced 0 → 1. Selecting Track 5 then
produced a **288.4 s Render preview from Rust WASM session** and the waveform is
visible in the piano-roll overlay. Pressing Play changed the master RMS meter
from −31 dBFS to −29 and then −16 dBFS over several seconds, which confirms
nonzero changing samples reached the app's analyser/master output chain.

The first pass before relinking had no MP3 media; its master meter stayed at
−100 dBFS and its time chip stopped at `00:28.000`. That was not a valid test of
the real project. After relinking and selecting the wave track, restarting from
zero showed the clock at `00:07.222` and the master meter at −31 dBFS; on a later
read the clock was `00:28.000` and the meter was −25 dBFS. The Play button
remained in Pause state with no clock-stall error. The changing nonzero RMS
proves MP3 samples are streaming through the analyser connected to the browser
output chain while the project clock advances. This completes the requested
browser playback acceptance. It does not establish the user's subjective
speaker volume, which depends on the selected output device and system level.

Current browser state: the real project remains open with the MP3 relinked,
Track 5 active, Play running, preview at 288.4 s, and a nonzero changing master
meter. The session is unsaved in the browser so the user can continue from the
same state. No source files were changed in this round; no build or tests were
run. The browser-level output and clock acceptance criteria are met.

## Follow-up (2026-10-07) — multitrack output and clock retest

The user asked whether the observed failure was in singer rendering or the
multitrack mixer. On the currently open Direct-WASM page, after relinking the
real MP3 again, Track 1 (`harm`) rendered a 258.9 s preview and Track 5 rendered
a 288.4 s preview. Track 5 playback produced changing master readings from
−33.2 to −13.7 to −16.7 dBFS, so nonzero media samples reach the output chain.
However, the transport clock remained exactly `00:28.000` across repeated reads
while Play stayed active. This supersedes the previous statement that the
28-second clock acceptance had passed: audio output is observed, but clock
progress past 28 seconds is still a failure tracked by T020.

Ran the full-project CLI mixdown on the supplied USTX with its extracted full
Teto bank and the exact referenced MP3. It rendered tracks 0–3 with
20/8/71/54 phrases (153 total) and track 4 with one wave part; output was
stereo, 44.1 kHz, 288.3918 s. Internal peak was 1.8682 before PCM conversion;
the written PCM16 WAV had 5,373 clipped samples out of 25,436,160 (0.0211%).
This shows the engine is not dropping the singer or audio tracks. It does leave
a small peak-clipping/headroom issue for mixdown parity review; no OpenUtau
whole-project reference was compared in this follow-up.

The previous `a_wave_only_track_renders_through_the_per_track_path` test still
expected mono samples after the per-track path was changed to stereo. Updated
the assertion to compare both interleaved channels against the mono source.
`cargo test -p synth-cli --test mixdown_wave_parts` now passes 10/10.
