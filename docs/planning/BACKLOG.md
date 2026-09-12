# Lilt — Backlog / Task List

> อัปเดตสถานะ: 2026-09-12
> เอกสารนี้เก็บรายละเอียด/ประวัติ native backlog เดิม. ใช้
> [Roadmap](roadmap/ROADMAP.md), [WASM Migration](wasm-migration-status.md) และ
> [Web UI Design](web-ui-design.md) เป็น source of truth สำหรับลำดับงานปัจจุบัน.

---

## ✅ เสร็จแล้ว (ล่าสุด)

- [x] **Phonemizer wire เข้า app (labels เหนือ note = server จริง)** — editor เรียก `/phonemize` (debounce + load + track switch) → `phonemesByNote` → roll merge → painter **server ก่อน** (user: "server ก่อนซิ") → hint → letter-split; `/phonemize` route ตาม track phonemizer (Japanese→VCV かさ→か,a,さ,a) + **segment whole-word alias สำหรับ labels** ("la"→["l","A"] ไม่ลอก lyric — render path ยังใช้ direct alias ตาม golden); regression `phonemesForLabel` (top-level) 3 cases ✅
- [x] **Unknown lyric ไม่ skip ทั้ง phrase (เพลงเหลือ 750ms)** — root cause: app ประกาศ `phonemizer: 'English'` → English CVVC path เจอคำไม่รู้จัก (g2p miss → "shi") → verbatim → oto ไม่มี alias → partial mapping → **skip ทั้ง phrase** → เพลง render แค่ phrase แรก 750ms ("เล่นไม่จบ"); แก้: app ใช้ **Universal** (letter-split + NearestPhoneme → shi→S/I) → 9 phrases 0 skipped — verified host + device (rendered raw 721668 = full 14.5s) ✅
- [x] **Playhead duration จริง (เล่นไม่จบหลังสลับ engine)** — playhead clamp ใช้ `totalBeats×spb` (14.5s) แต่ wav แต่ละ engine ยาวต่างกัน → position ไปไม่ถึง 1.0 → playhead ค้าง/จบไม่ตรง; แก้: `onDurationChanged` → `playbackDuration` stream → `_playCtrl.duration` = duration จริง + `onPlayerComplete` push ตำแหน่งสุดท้าย ✅

- [x] **Playback sync (playhead ตรงเสียงจริง)** — 2 bugs: (1) roll hardcode BPM 128 แต่ render ที่ 110 → playhead เร็ว 6% จบก่อน → ตัดท้าย ("not sync"); (2) audioplayers startup latency — playhead เริ่มก่อนเสียง. fix: `PianoRoll.secondsPerBeat` (editor ส่ง `60/_bpm`) + **position-driven playhead** (`player.onPositionChanged` → stream → `_playCtrl.value`) — immune ต่อ latency/tempo/drift — regression `playhead_sync_test` ✅
- [x] **Dirty flag — ไม่เล่น audio เก่าหลัง edit** — เดิม `_playDemo` เล่น `_latestAudio` ทันทีที่ non-null (edit → debounce 500ms ยังไม่ทัน render → ได้ยินของเก่า "ใช้ old cache") — `_audioDirty` set บน edit/engine-switch, clear หลัง render → play ต้อง render ก่อนเสมอ; device log: edit → full re-render 5-6s → play cache 20-30ms ✅
- [x] **Zoom red screen fix (pinch เกิน limit → จอแดง)** — pinch clamp 30-300 อยู่แล้ว แต่ **X/Y Sliders มี [min,max] แคบ (60-130 / 50-150)** — pinch ผลัก value ออกนอก range → `Slider` assert → red screen — **ลบ sliders ออก** (ย้ายเป็น pinch + ± buttons ตาม user) + `_applyZoom` hard clamp — regression pinch test ✅
- [x] **Server chunk cache เปิดจริง** — `render_service.rs` ส่ง `cache = None` (dead code) → `RenderCache::new_in_dir(voicebank/cache, 64MB)` — device render 6.6s → 20-37ms ข้าม restart ✅
- [x] **Pre-render ตอน load + engine switch rerender** — initState post-frame `_scheduleReRender` (play ไม่รอ render แรก); dropdown engine → clear audio + rerender (cache salt ต่อ engine — ถูกต้องที่ re-render เต็มครั้งแรก) ✅
- [x] **Chunk pitch-curve rebase (scale demo ตอนลง pitch ผิด)** — chunking rebase position_ms แต่ pitches_cents/curves ยัง full-phrase → chunks หลังแรก sample pitch ผิด (ซ้ำ notes 0-7) → slice ตาม chunk origin — 4 chunks f0 ตรง note targets ทุกตัว ✅
- [x] **v2↔r2 golden ปิด (auto_gain logistic sign)** — C# `Exp(5−10r)` vs Rust `Exp(10r−5)` กลับข้าง → s gain 1.86× → fix → **s 1.00 / l 1.03 / 3 0.94 / la 1.02** + mel powers identical กับ C# — golden report §3.7 ✅
- [x] **v1↔r1 = official (AutoGain revert) + third_party กู้ครบ** — world @2bb515f + patches (apply จากใน repo), libgvps/libpyin/spline/libnpy; r1 match OpenUtau Mobile path (RMS 0.85-1.13); seam 2 chunks เนียน ✅
- [x] **.so split v1/v2** — `WORLDLINE_FRAME_MS_DEFAULT` compile define: build-linux (11.61ms v2) + build-linux-v1 (10ms official) — รากฐาน engine select ✅
- [x] **INTERNET permission (release)** — main manifest ไม่มี → Android 10+ bind EPERM ("Operation not permitted" บน 18080) — เพิ่ม + deploy ✅

- [x] **Stop จริง (server-side cancel)** — `pipeline.rs` ตรวจ `cancel: Arc<AtomicBool>` ระหว่าง chunk loop → คืน `"render cancelled"`; `RenderService.cancel()` **set flag ตรงๆ** (ไม่ผ่าน channel — worker block ใน render รับ job ไม่ได้); `POST /cancel` — device verify: หยุดเสียงได้ ✅
- [x] **Mixer post-synth (แยก rendering กับ playback)** — `POST /post-fx`: ส่ง raw wav + `x-mixer-params` header → server apply mixer chain → wav ใหม่ (**ไม่ synth ใหม่** — fader/EQ เปลี่ยนทันที); editor แยก `_rawAudio` (synth ล้วน) + `_mixerParams` + `_applyMixerFx()`; **dB→linear fix** (`10^(dB/20)` — เดิม `dB/20` ทำให้ 0dB=0 เงียบ/level ไม่ต่าง)
- [x] **Stop playback ("graphic หยุดแต่เสียงไม่")** — `PianoRoll.onPlayStopped` hook (toggle-off + playhead-complete → editor `_stopPlayback()` หยุด AudioPlayer); menu `⋯` แสดง `⏹ Stop` ตอน `_renderActive || _isPlaying` (เดิมเฉพาะตอน render ซึ่งเร็วมาก); `_isPlaying` เคลียร์ด้วย `onPlayerComplete` (audioplayers `play()` คืนทันทีที่เริ่ม ไม่ใช่จบ)
- [x] **Mixer plugin เต็ม backend** — EQ rewrite (RBJ cookbook: low shelf 200Hz + mid peak 1kHz + high shelf 4kHz, DF2T stateful — sine sweep 10/10) + wire `synth-server --mixer-so --mixer-params` → `WorldlineEngine.set_mixer()` + Android build (`mixerfx-android.sh` NDK r28c) + bundle jniLibs — E2E passthrough rel diff 0.0000000000
- [x] **Chunking × partial-oto panic fix** — `pipeline.rs` gate ก่อน chunking: oto.len() != phonemes.len() → skip phrase ครั้งเดียว (แทน panic `base.oto[ctx_range]`) — test partial-mapping restore; `phrases_rendered` นับ chunks (demo-song 1 phrase → 2)
- [x] **UI mock รวม (index.html)** — OM trackstrip trace + Flutter-mirror toolbar (View/Select/Draw/Pitch/Split/Erase + Select All→Notes→Curve + capture rate + zoom X/Y) + mixer bottom sheet (draft-1: fader/pan/EQ/comp) toggleable + FX overlay (chunk marks + GR curve + hot notes) + **SynthV phoneme labels เหนือ note** + collapsible left panel (◀)
- [x] **Flutter port (additive — roll logic เดิม untouched)** — `MixerPanel` widget ใหม่ + `PianoRoll.showPhonemes` (paint-only) + `showFxOverlay` (⚡ FX toolbar → `_FxOverlayPainter`: chunk marks/GR curve/hot rings) + `_panelCollapsed`/`_mixerOpen` toggles + phoneme จาก lyric `[hint]` (painter derive — แก้ lyric แล้ว labels เปลี่ยนทันที ไม่ต้อง parent rebuild) + **note height scale ตาม Y zoom** (`_noteHeightFor(rowHeight)` แทน const 24px) + **mixer ตกจอ fix** (SingleChildScrollView editor + SafeArea + bounded roll) + **scroll-to-notes ตอนเปิด** (jump ไป pitch band ของ notes)
- [x] **Worldline v2 (WORLDLINE-R2) default** — frame 11.61ms (hop 512) — f0 แม่นกว่า v1 ตอน scale ลง (err 150Hz → 32Hz) — user ยืนยัน v2 ดีกว่า
- [x] **Vocal Scale demo** — 29 notes "la[l A]" C4→C6 ขึ้น+ลง, 0 gaps, ตรง golden (RMS 0.995×) — แทนเพลง Senbonzakura/Machine Love ที่พัง
- [x] **Engine adapter** — `Engine` trait + `WorldlineEngine` (cache ข้าม request) — server ใช้ `Box<dyn Engine>` — E2E output ตรง CLI เป๊ะ (rel diff 0.000000)
- [x] **Render chain bugs** — length=dur+leading, skip=C# formula, frq ส่ง C++ (FrqEstimator), loudness calibration (volume VOL×0.44 → RMS 0.995× golden)
- [x] **Cache** — res-{hash}.bin (OpenUtau pattern) — device 15ms hit — 3.5.1 ✅
- [x] **frq pointer cross-platform fix** — `c_char` = i8 (Linux) / u8 (Android) → cast ผ่าน `std::os::raw::c_char`
- [x] **bpm fix** — editor `_bpm = 110` (ตรง Vocal Scale) + ส่ง buildUstx — ไฟล์ app = engine reference
- [x] **Play UX** — snackbar "Rendering…" (เสียงไม่ดูเหมือนมาหลังจบ)
- [x] **Re-render ตอนแก้** — hook onNotesChanged → debounce 500ms → render ผ่าน cache (`editor_screen.dart`)
- [x] **Chunk-level render** — `Chunker` (split + overlap + XXH64 hash) + mixer crossfade for incremental re-render
- [x] **Golden infra กลับมา** — dotnet 8.0.423 @ ~/dotnet + golden-renderer --no-build (MSB3554 environment issue — ไม่บล็อก golden)

---

## 📋 งานค้าง

### งานหลัก
| # | งาน | ขนาด |
|---|---|---|
| 1 | **Per-track mixer FX** — design ตัดสินใจแล้ว (user: "แยก effect ต่อ track/ช่วงเวลาได้ไหม") — ตอนนี้ mixer ตัวเดียวบน final mix; ต้อง per-track `MixerFx` + params จาก ustx + time-based FX (pos_ms ว่าง) | กลาง |
| 2 | **Phonemizer multi-layer (IPA)** — **plan: `.hermes/plans/2026-08-04_phonemizer-ipa.md`** — **Tasks 1-7 เสร็จ 2026-08-06** (verified 9/9): L3 PhonemeManifest (derive จาก oto จริง), EngineCapabilities trait (domain — worldline implement, phonemizer ไม่ depend worldline), L1 IpaDictionary (ARPABET↔IPA) + detect ภาษา, L2 IpaAdapter + NearestPhoneme (a→A, r→r3, R→None) + CapabilityManager, UniversalPhonemizer (hint path ไม่ regression), pipeline PhonemizerKind::Universal (default ใหม่) — E2E render_real 4/4 + workspace 427 passed | ~~ใหญ่~~ **เสร็จ** |

### ตรวจ/รอ user
| # | งาน | สถานะ |
|---|---|---|
| 3 | ~~Golden re-run เต็ม~~ — **เสร็จ 2026-08-08**: v2↔r2 ปิด (0.94-1.03 + mel identical), v1↔r1 = official path — golden report §3.7/§4 | done |
| 4 | ~~Device deploy ตรวจ stop/mixer/phoneme~~ — **ผ่านแล้ว 2026-08-05**: stop ระหว่างเล่น ✅, mixer post-fx (fader เปลี่ยนเสียง) ✅, phoneme labels ✅ | done |
| 5 | ~~Device deploy ตรวจ play UX~~ — **ผ่าน 2026-08-08**: pre-render + playhead sync + edit→rerender + pinch zoom (no red) — "ครบอยู่นะ" | done |
| 6 | **Engine select per-phrase UI (ระหว่างท่อน)** — server รองรับแล้ว (`RenderRequest.engines[]` + render_service resolve per phrase); app มี dropdown engine เดียว (r1/r2/classic) — ยังไม่มี UI เลือก engine ต่อท่อน | ค้าง |

### MS3 — Renderers
| # | งาน | สถานะ |
|---|---|---|
| 8 | **MS3.1 Classic** — pure-Rust resampler + concat/crossfade/envelope และ oracle parity ทำแล้ว; เหลือ flags B/H/P กับ product-level song golden | 🟡 |
| 9 | **MS3.2 v2 neural** — native R2, mel/vocoder pipeline และ v2↔r2 golden ทำแล้ว; WASM neural ยังไม่ทำ | ✅ Native / ⬜ WASM |
| 10 | **MS3.3 DiffSinger** — loader/tensors/acoustic+vocoder/engine ถึง tensor→WAV แล้ว; เหลือ variance/pitch, timing gap และ device listening | 🟡 |
| 11 | **MS3.4 Vogen** — model loader (.vogen) + tensor feed + VSVocoder (DecodeMgc/Bap + WORLD) + test (Mandarin/Cantonese) | ใหญ่ |
| 12 | **MS3.5 Perf** — 3.5.2 tensor cache + 3.5.3 benchmark (1 note <1s, RAM <1GB) + 3.5.4 optimize + 3.5.5 stress 30-min — *3.5.1 cache ✅* | กลาง |

---

## 🎨 UI (ของใหม่ที่ port แล้ว + ของที่เหลือ)

- **Mock หลัก:** `ui-mock/index.html` — editor รวม: OM trackstrip + toolbar ตรง Flutter + mixer bottom sheet (⊞ Mixer) + FX overlay (⚡ FX) + SynthV phoneme เหนือ note + collapsible panel (◀) — **อ้างอิงหลักสำหรับ port ต่อ**
- **Port แล้วใน Flutter:** MixerPanel (bottom, toggleable → post-fx), phoneme labels (painter derive), FX overlay, collapse panel, note-height scale, mixer scroll, stop playback
- **Flutter ยังไม่มี (จาก mock):** undo/redo (ปุ่มมีแต่ no-op), BPM chip popup, snap-div popup, trackstrip แนวนอนจริง (ตอนนี้เป็น vertical track list), parts/guide track, singer portrait
- **Plan UI:** `docs/planning/milestones/ms4/MS.md` (กำลังทำทั้ง Flutter และ Web)
- หลัก: "trace ui-mock/index.html ก่อนแก้ Flutter" — port additive ห้ามแตะ roll logic เดิม

---

## 🔧 Environment (สำคัญข้าม session)

- **Working dir:** ใช้ repository root ปัจจุบัน (ไม่ผูกกับ absolute path เก่าที่เคยเป็น `/home/seal/project/...`)
- **Device:** Mi Pad `d370854b` — USB หลุดบ่อย (มี reconnect ระหว่าง session); `flutter run -d d370854b` (debug keystore เดียวกัน); **ถ้า USB หลุด kill flutter run แล้ว rerun** (hot reload ไม่ได้เมื่อ stdin ปิด); **terminal guard มี bug กับ inline adb path** → ใช้ wrapper script `/tmp/adbw.sh` (`exec /home/seal/Android/Sdk/platform-tools/adb "$@"`) หรือ Python subprocess
- **Env:** dotnet 8.0.423 @ ~/dotnet; Rust 1.97.1 @ ~/.cargo/bin; JDK21 @ ~/jdk21 (JAVA_HOME); cmake @ ~/cmake-3.31.6-linux-x86_64/bin; SDK @ ~/Android/Sdk (NDK 28.2.13676358); flutter @ ~/project/flutter/bin
- **Android build:** `bash scripts/bundle-android.sh` (ตอนนี้ bundle libsynthserver + libworldline + **libmixerfx** + demo.ustx + voicebank subset 29 wavs) — **rm .so ก่อน bundle ถ้าแก้ Rust**
- **Historical test snapshot (2026-08-05):** Flutter **36 passed + 4 skipped**; cargo api/render suites ผ่าน. สำหรับ WASM/browser checkpoint ล่าสุดดู `wasm-migration-status.md`.
- **Voicebank:** `test/golden/teto-english/` (library/) — golden render ผ่าน golden-renderer (OPENUTAU_REF=/home/seal/openutau-ref — copy ของ ref ที่ path ธรรมดา ไม่มีวงเล็บ)
