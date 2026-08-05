# Lilt — Backlog / Task List

> อัปเดตล่าสุด: 2026-08-05
> สรุปงานค้างทั้งหมด — ใช้ต่อข้าม session

---

## ✅ เสร็จแล้ว (ล่าสุด)

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
| 2 | **Phonemizer multi-layer (IPA)** — **plan เขียนแล้ว: `docs/.hermes/plans/2026-08-04_phonemizer-ipa.md`** — detect → IPA → voicebank phonemes + manifest + CapabilityManager + NearestPhoneme | ใหญ่ |

### ตรวจ/รอ user
| # | งาน | สถานะ |
|---|---|---|
| 3 | Golden re-run เต็ม (dotnet กลับมา — verify regression หลัง v2 default) | พร้อมทำ |
| 4 | ~~Device deploy ตรวจ stop/mixer/phoneme~~ — **ผ่านแล้ว 2026-08-05**: stop ระหว่างเล่น ✅, mixer post-fx (fader เปลี่ยนเสียง) ✅, phoneme labels ✅ | done |

### MS3 — Renderers
| # | งาน | สถานะ |
|---|---|---|
| 8 | **MS3.1 Classic — merge เข้า worldline** (ไม่แยก engine): `Resample()` FFI มีแล้ว + `build_requests` ซ้ำได้ + **Wavtool port ใน Rust** (concat+crossfade+envelope — งานหลัก) + wire `WorldlineEngine` mode=Classic + golden classic — flags B/H เหลือเพิ่ม | เริ่มได้เลย |
| 9 | **MS3.2 v2 neural** — nsf-hifigan (vocoder.yaml+onnx) + WORLD→mel (ONNX) + v2 pipeline + golden v2 — *v2 config ทำแล้ว = default* | ใหญ่ |
| 10 | **MS3.3 DiffSinger** — onnxruntime vendored .so + DsSinger loader + tensor feed + variance/pitch predictor + renderer + test bank | ใหญ่ |
| 11 | **MS3.4 Vogen** — model loader (.vogen) + tensor feed + VSVocoder (DecodeMgc/Bap + WORLD) + test (Mandarin/Cantonese) | ใหญ่ |
| 12 | **MS3.5 Perf** — 3.5.2 tensor cache + 3.5.3 benchmark (1 note <1s, RAM <1GB) + 3.5.4 optimize + 3.5.5 stress 30-min — *3.5.1 cache ✅* | กลาง |

---

## 🎨 UI (ของใหม่ที่ port แล้ว + ของที่เหลือ)

- **Mock หลัก:** `ui-mock/index.html` — editor รวม: OM trackstrip + toolbar ตรง Flutter + mixer bottom sheet (⊞ Mixer) + FX overlay (⚡ FX) + SynthV phoneme เหนือ note + collapsible panel (◀) — **อ้างอิงหลักสำหรับ port ต่อ**
- **Port แล้วใน Flutter:** MixerPanel (bottom, toggleable → post-fx), phoneme labels (painter derive), FX overlay, collapse panel, note-height scale, mixer scroll, stop playback
- **Flutter ยังไม่มี (จาก mock):** undo/redo (ปุ่มมีแต่ no-op), BPM chip popup, snap-div popup, trackstrip แนวนอนจริง (ตอนนี้เป็น vertical track list), parts/guide track, singer portrait
- **Plan UI:** docs/planning/milestones/ms4/MS.md (deferred)
- หลัก: "trace ui-mock/index.html ก่อนแก้ Flutter" — port additive ห้ามแตะ roll logic เดิม

---

## 🔧 Environment (สำคัญข้าม session)

- **Working dir:** `/home/seal/project/android-voice-synth` (app/ = Flutter, native/ = Rust crates)
- **Device:** Mi Pad `d370854b` — USB หลุดบ่อย (มี reconnect ระหว่าง session); `flutter run -d d370854b` (debug keystore เดียวกัน); **ถ้า USB หลุด kill flutter run แล้ว rerun** (hot reload ไม่ได้เมื่อ stdin ปิด); **terminal guard มี bug กับ inline adb path** → ใช้ wrapper script `/tmp/adbw.sh` (`exec /home/seal/Android/Sdk/platform-tools/adb "$@"`) หรือ Python subprocess
- **Env:** dotnet 8.0.423 @ ~/dotnet; Rust 1.97.1 @ ~/.cargo/bin; JDK21 @ ~/jdk21 (JAVA_HOME); cmake @ ~/cmake-3.31.6-linux-x86_64/bin; SDK @ ~/Android/Sdk (NDK 28.2.13676358); flutter @ ~/project/flutter/bin
- **Android build:** `bash scripts/bundle-android.sh` (ตอนนี้ bundle libsynthserver + libworldline + **libmixerfx** + demo.ustx + voicebank subset 29 wavs) — **rm .so ก่อน bundle ถ้าแก้ Rust**
- **Tests:** Flutter **36 passed + 4 skipped** (`test/mixer_features_test.dart` = phoneme/mixer/collapse/FX/params-emission; `engine_client_test.dart` = cancel/setMixerParams/postFx); cargo: api_real 4/4 (รวม cancel + mixer-params + post-fx), render_real 4/4, workspace 0 errors
- **Voicebank:** `test/golden/teto-english/` (library/) — golden render ผ่าน golden-renderer (OPENUTAU_REF=/home/seal/openutau-ref — copy ของ ref ที่ path ธรรมดา ไม่มีวงเล็บ)
