# Lilt — Backlog / Task List

> อัปเดตล่าสุด: 2026-08-04
> สรุปงานค้างทั้งหมด — ใช้ต่อข้าม session (UI จะทำอีก session)

---

## ✅ เสร็จแล้ว (ล่าสุด)

- [x] **Worldline v2 (WORLDLINE-R2) default** — frame 11.61ms (hop 512) — f0 แม่นกว่า v1 ตอน scale ลง (err 150Hz → 32Hz) — user ยืนยัน v2 ดีกว่า
- [x] **Vocal Scale demo** — 29 notes "la[l A]" C4→C6 ขึ้น+ลง, 0 gaps, ตรง golden (RMS 0.995×) — แทนเพลง Senbonzakura/Machine Love ที่พัง
- [x] **Engine adapter** — `Engine` trait + `WorldlineEngine` (cache ข้าม request) — server ใช้ `Box<dyn Engine>` — E2E output ตรง CLI เป๊ะ (rel diff 0.000000)
- [x] **Render chain bugs** — length=dur+leading, skip=C# formula, frq ส่ง C++ (FrqEstimator), loudness calibration (volume VOL×0.44 → RMS 0.995× golden)
- [x] **Cache** — res-{hash}.bin (OpenUtau pattern) — device 15ms hit — 3.5.1 ✅
- [x] **frq pointer cross-platform fix** — `c_char` = i8 (Linux) / u8 (Android) → cast ผ่าน `std::os::raw::c_char`
- [x] **bpm fix** — editor `_bpm = 110` (ตรง Vocal Scale) + ส่ง buildUstx — ไฟล์ app = engine reference
- [x] **Play UX** — snackbar "Rendering…" (เสียงไม่ดูเหมือนมาหลังจบ)
- [x] **Golden infra กลับมา** — dotnet 8.0.423 @ ~/dotnet + golden-renderer --no-build (MSB3554 environment issue — ไม่บล็อก golden)

---

## 📋 งานค้าง (12 รายการ)

### งานหลัก (1-4)
| # | งาน | ขนาด |
|---|---|---|
| 1 | **Mixer plugin** — 1 track + full FX (gain→3-band EQ→compressor→softclip); passthrough ต้องไม่เปลี่ยนเสียง (RMS 0.995× golden) — pattern เดียวกับ worldline .so (FFI: MxFxCreate/Process/Destroy) | กลาง |
| 2 | **Re-render ตอนแก้** (POC a) — hook onNotesChanged/onCurveChanged → debounce 500ms → render ผ่าน cache (เงียบ ไม่ขัดจังหวะ) | เล็ก |
| 3 | **Chunk-level render** (POC b) — wire `Chunker` (มี infra แล้ว — chunk_size+overlap) เข้า pipeline — scale 1 phrase → แก้โน้ตเร็ว ~1-2s | ใหญ่ |
| 4 | **Phonemizer multi-layer (IPA)** — **plan เขียนแล้ว: `docs/.hermes/plans/2026-08-04_phonemizer-ipa.md`** — detect → IPA → voicebank phonemes + manifest + CapabilityManager + NearestPhoneme | ใหญ่ |

### ตรวจ/รอ user (5-7)
| # | งาน | สถานะ |
|---|---|---|
| 5 | Golden re-run เต็ม (dotnet กลับมา — verify regression หลัง v2 default) | พร้อมทำ |
| 6 | ~~ฟัง verdict v1/v2~~ — ✅ user ยืนยัน v2 | done |
| 7 | ~~Deploy device ครบชุด~~ — ✅ cache 15ms hit | done |

### MS3 — Renderers (8-12)
| # | งาน | สถานะ |
|---|---|---|
| 8 | **MS3.1 Classic — merge เข้า worldline** (ไม่แยก engine): `Resample()` FFI มีแล้ว + `build_requests` ซ้ำได้ + **Wavtool port ใน Rust** (concat+crossfade+envelope — งานหลัก) + wire `WorldlineEngine` mode=Classic + golden classic — flags B/H เหลือเพิ่ม | เริ่มได้เลย |
| 9 | **MS3.2 v2 neural** — nsf-hifigan (vocoder.yaml+onnx) + WORLD→mel (ONNX) + v2 pipeline + golden v2 — *v2 config ทำแล้ว = default* | ใหญ่ |
| 10 | **MS3.3 DiffSinger** — onnxruntime vendored .so + DsSinger loader + tensor feed + variance/pitch predictor + renderer + test bank | ใหญ่ |
| 11 | **MS3.4 Vogen** — model loader (.vogen) + tensor feed + VSVocoder (DecodeMgc/Bap + WORLD) + test (Mandarin/Cantonese) | ใหญ่ |
| 12 | **MS3.5 Perf** — 3.5.2 tensor cache + 3.5.3 benchmark (1 note <1s, RAM <1GB) + 3.5.4 optimize + 3.5.5 stress 30-min — *3.5.1 cache ✅* | กลาง |

---

## 🎨 UI (อีก session — อ้างอิง HTML draft)

- **Draft:** `ui-mock/index-om.html` (50KB — "OpenUtau Mobile flow — Lilt review" — trace UI ของ OpenUtau Mobile MAUI, MIT)
- **Views ใน draft:** home (New/Open/Singers/Options + Recent projects) / editor (titlebar save-undo-redo-more + chips BPM/time-sig/key + trackstrip + piano roll + float tools ✎🧲⛶▶) / singers (Teto card + FAB) / detail / options / settings / install / about
- **Flutter ยังไม่มี (จาก draft):** undo/redo, BPM chip, snap, float tools circular, parts/guide track, trackstrip แนวนอน
- **Plan UI:** docs/planning/milestones/ms4/MS.md (deferred — user จะสั่งอีกที)
- หลัก: "trace ui-mock/index-om.html ก่อนแก้ Flutter" (memory)

---

## 🔧 Environment (สำคัญข้าม session)

- **Working dir:** `/home/seal/project/android-voice-synth` (app/ = Flutter, native/ = Rust crates)
- **Device:** Mi Pad `d370854b` — USB หลุดบ่อย; `flutter run -d d370854b` (debug keystore เดียวกัน — ไม่เจอ signature mismatch)
- **Env:** dotnet 8.0.423 @ ~/dotnet; Rust 1.97.1 @ ~/.cargo/bin; JDK21 @ ~/jdk21 (JAVA_HOME); cmake @ ~/cmake-3.31.6-linux-x86_64/bin; SDK @ ~/Android/Sdk (NDK 28.2.13676358); flutter @ ~/project/flutter/bin
- **Android build:** `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=.../aarch64-linux-android24-clang cargo build -p synth-server --target aarch64-linux-android --release` — bundle: `bash scripts/bundle-android.sh` (build เฉพาะเมื่อ .so ไม่มี → **rm .so ก่อน bundle ถ้าแก้ Rust**)
- **Tests:** Flutter 26 passed + ~4 skipped; cargo 397 passed (worldline-plugin 22, phonemizer 23)
- **Voicebank:** `test/golden/teto-english/` (library/) — golden render ผ่าน golden-renderer (OPENUTAU_REF=/home/seal/openutau-ref — copy ของ ref ที่ path ธรรมดา ไม่มีวงเล็บ)
