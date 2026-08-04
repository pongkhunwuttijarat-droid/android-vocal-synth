# MS1 — Main Engine Core (Rust, engine-first)

**สถานะ:** In planning — engine เริ่มเป็นตัวแรก
**โครงสร้าง:** 5 sprints หลัก + plugin track คู่ขนาน

---

## หลักการ

```
✅ .so บน Android = พิสูจน์แล้ว (OpenUtau Mobile มี prebuilt, เราเคยใช้ .so)
✅ Main engine (Rust) = งานใหญ่สุด + test ได้โดยไม่ต้อง .so (cargo test)
→ สลับลำดับ: engine ก่อน, plugin ทำคู่ขนาน

Engine track (หลัก):  domain → commands → phonemizer → feed → runtime
Plugin track (คู่):   prebuilt .so FFI → NDK build (ไม่บล็อก engine)
```

---

## Sprint 1.1 — Domain Model (.ustx compat)

| # | Task | Detail | Status |
|---|---|---|---|
| 1.1.1 | UProject structs | project/part/track/note/curve/expression/phoneme | ⬜ |
| 1.1.2 | YAML serialization | save/load .ustx (v0.9 + migration hooks) | ⬜ |
| 1.1.3 | TimeAxis | tick↔ms, tempo/time-sig segments | ⬜ |
| 1.1.4 | Expression registry | abbr → range/flag map | ⬜ |
| 1.1.5 | Unit tests | roundtrip + timeAxis | ⬜ |

**Done:** cargo test ผ่าน — .ustx load/save ถูกต้อง

---

## Sprint 1.2 — Command System

| # | Task | Detail | Status |
|---|---|---|---|
| 1.2.1 | Command trait | execute/unexecute + validate | ⬜ |
| 1.2.2 | Note/Track/Part commands | add/move/delete/resize | ⬜ |
| 1.2.3 | Curve/Expression commands | set curve, change expression | ⬜ |
| 1.2.4 | Undo/Redo stack | bounded + groups | ⬜ |
| 1.2.5 | Unit tests | undo = redo inverse | ⬜ |

**Done:** command roundtrip tests ผ่าน

---

## Sprint 1.3 — Phonemizer Framework

| # | Task | Detail | Status |
|---|---|---|---|
| 1.3.1 | Phonemizer trait | process(notes, singer) → phonemes | ⬜ |
| 1.3.2 | Japanese VCV | rule-based kana → VCV | ⬜ |
| 1.3.3 | English CVVC | phonetic hints (test กับ Teto English) | ⬜ |
| 1.3.4 | G2p base | dictionary lookup + SP/AP symbols | ⬜ |
| 1.3.5 | Unit tests | ref test banks (ja_cv_integration ฯลฯ) | ⬜ |

**Done:** phonemize ถูกต้อง + oto parser (Teto shift_jis)

---

## Sprint 1.4 — Feed Pipeline (12 transformers)

| # | Task | Detail | Status |
|---|---|---|---|
| 1.4.1 | VoicebankLoader | oto.ini/character/prefix.map + wav reader (Teto) | ✅ (voicebank crate) |
| 1.4.2 | **Storage trait** | `&dyn Storage` — FsStorage (linux) + JniStorage stub (Android path ผ่าน Kotlin) — feed ทั้งหมดอ่านผ่านนี้ | ✅ (storage crate) |
| 1.4.3 | PitchComputer | vibrato + PITD + points + toneShift | ✅ (feed/pitch.rs) |
| 1.4.4 | CurveSampler | points+eqn → per-5-tick → per-frame | ✅ (feed/curve.rs) |
| 1.4.5 | OtoMapper + Envelope + Flags | phoneme+tone → oto, p1-p5, g/B/H/P | ✅ (feed/oto.rs, envelope.rs) |
| 1.4.6 | Neural transforms | tokenizer + duration→frames + ToneToFreq | ✅ (feed/f0.rs + music_math) |
| 1.4.7 | RenderInput builder | superset ตาม capability | ✅ (feed/render_input.rs) |
| 1.4.8 | Unit tests | build RenderInput จาก .ustx (Teto) | ✅ 28 tests |

**Done:** RenderInput จาก project ครบ + feed ผ่าน Storage trait ✅

---

## Sprint 1.5 — Runtime (Scheduler/Chunker/Cache/Mixer)

| # | Task | Detail | Status |
|---|---|---|---|
| 1.5.1 | Scheduler | job queue, cancel, progress, retry | ✅ (runtime/scheduler.rs) |
| 1.5.2 | Chunker | split → chunks (parallel-ready) | ✅ (runtime/chunker.rs) |
| 1.5.3 | Cache | hash-based (XXH64) + LRU | ✅ (runtime/cache.rs + hash.rs) |
| 1.5.4 | Mixer | align + sum + dynamics + fade | ✅ (runtime/mixer.rs) |
| 1.5.5 | WAV writer | export 16/32-bit | ⬜ (MS2 — ผ่าน voicebank wav reader ย้อนกลับได้) |
| 1.5.6 | Unit tests | chunk/mix/cache logic | ✅ 54 tests |

**Done:** runtime ครบ ✅

---

## Plugin Track (คู่ขนาน — ไม่บล็อก engine)

| # | Task | Detail | Status |
|---|---|---|---|
| P.1 | plugin_abi.h | capability struct + C ABI v1 | ✅ (worldline-sys + abi) |
| P.2 | FFI smoke test | Rust โหลด prebuilt libworldline.so (linux + arm64) | ✅ (worldline-sys smoke — 13 symbols resolve) |
| P.3 | NDK build | CMakeLists + build เอง — **linux .so ผ่านแล้ว + smoke test ผ่าน (behavior ตรง ref)** | ✅ linux / ⬜ android arm64 script พร้อม |
| P.4 | golden reference | OpenUtau desktop render Teto → reference wav | ⬜ (ไม่มี dotnet บนเครื่อง — defer ไป MS2 2.3) |

---

## Acceptance Criteria (MS1)

- [x] .ustx load/save compat + commands + undo ทำงาน (unit test)
- [x] phonemizer JP/EN + voicebank loading (Teto จริง)
- [x] RenderInput ครบทุก transformer
- [x] scheduler/chunker/cache/mixer ทำงาน (pure Rust test)
- [x] plugin_abi.h + FFI smoke test ผ่าน (prebuilt .so)
- [x] **libworldline.so build เอง + synthesis smoke test ผ่าน (ตรง ref)** ← เพิ่ม

**ผลรวม: 309 tests ผ่าน / clippy clean / .so รันได้บน linux**
