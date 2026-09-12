# MS3 — Renderers เต็ม

**สถานะ:** 🟡 In progress
**ระยะเวลา:** 5 sprints
**Requires:** MS2 (feed + scheduler ทำงาน)

---

## Goal

Renderer ทั้งหมด + คุณภาพเสียง: classic mode, worldline v2 (neural vocoder), DiffSinger, Vogen — สลับ renderer ต่อ track ได้

## Current checkpoint (2026-09-12)

- Native R1/R2 ใช้งานผ่าน CLI/server/app และ golden v1↔r1 / v2↔r2 ผ่านแล้ว.
- Pure-Rust `worldline-rs` และ Classic resampler เทียบ `.so` ผ่าน; Classic flags
  และ product-level OpenUtau song golden ยังไม่ครบ.
- DiffSinger ทำ loader/tensor/acoustic+vocoder/engine ได้ถึง tensor→WAV; timing
  ระหว่าง phrase, variance/pitch predictor และ device listening gate ยังเหลือ.
- MoE เป็น experimental renderer คู่ขนานและมี engine/training-prep แล้ว แต่ยังไม่ใช่
  acceptance criterion แทน Vogen.
- Vogen ยังไม่เริ่ม; การสลับ renderer ต่อ track และ stress/device benchmark ยังไม่ปิด.

---

## Sprint 3.1 — Worldline Classic Mode

| # | Task | Detail | Status |
|---|---|---|---|
| 3.1.1 | Classic API export | Resample() per-phone + concat ใน worldline C++/Rust | ✅ Rust resampler path ทำงาน |
| 3.1.2 | Classic feeder (Rust) | ResamplerItem build + concat/crossfade/envelope | ✅ parity kernel ผ่าน |
| 3.1.3 | Flags compat | g/B/H/P/Mt/Mb/Mv full | 🟡 g/Mt/Mb/Mv มีแล้ว; B/H/P ยังไม่ครบ |
| 3.1.4 | Golden test | เทียบกับ OpenUtau classic render | 🟡 Oracle parity ผ่าน; song/product golden ยังเหลือ |

**Exit target:** classic mode พร้อม flags และ song golden เทียบ OpenUtau ผ่าน

---

## Sprint 3.2 — Worldline v2 (Neural Vocoder)

| # | Task | Detail | Status |
|---|---|---|---|
| 3.2.1 | Vocoder package | nsf-hifigan (vocoder.yaml + onnx) — dependency path | ✅ |
| 3.2.2 | Mel conversion | WORLD features → mel (ONNX) | ✅ mel powers ตรง C# |
| 3.2.3 | v2 pipeline | f0/sp/ap → mel → hifigan → audio (+ease in/out) | ✅ Native R2 |
| 3.2.4 | Golden test | เทียบ OpenUtau worldline2 | ✅ v2↔r2 ผ่าน |

**Done:** v2 เสียงคุณภาพสูง เทียบ golden ผ่าน

---

## Sprint 3.3 — DiffSinger

**Plan:** `DIFFSINGER.md` (ในโฟลเดอร์นี้) — port จาก C# `ref-openutau-clean/.../DiffSinger/`

| # | Task | Detail | Status |
|---|---|---|---|
| 3.3.1 | onnxruntime | vendored .so + ort crate (Android support) — ort มีอยู่แล้วจาก R2 | ✅ reuse |
| 3.3.2 | DsSinger | dsconfig.yaml + phonemes.txt + tokenizer (**M1**) | ✅ — `crates/diffsinger` — 14 tests |
| 3.3.3 | Tensor feed | tokens/durations/f0 (**M2**) — `tensors.rs` — round-half-even ตรง C# | ✅ |
| 3.3.4 | Variance/Pitch predictor | dsvariance + dspitch (optional) (**M3**) | ⬜ |
| 3.3.5 | Renderer | acoustic + vocoder session run (**M4**) — `model.rs` + mel base transform + shape checks | ✅ |
| 3.3.6 | Engine + CLI (**M5**) | `DsEngine` + `--engine diffsinger --ds-model` + **fake-model E2E** (14.5s, regression test) | ✅ |
| 3.3.7 | Golden (**M6**) | **C# golden harness ทำงานแล้ว** (bank + fake dsdur + nsf_hifigan — C# render 4040ms ✓); **token mapping EXACT** (dict-driven — case kept — 26 phonemes ตรง); **dsdur path ใน backend ของเรา** (OpenUtau architecture — linguistic+dur runs + stretch); **wav-level ยัง FAIL 15%**: pipeline split 2 phrases (gap) vs C# 1 phrase (cross-note consonant bridge) + timing — ต้อง adopt C# phonemizer timing | 🔨 timing gap |
| 3.3.8 | Device + app dropdown (**M7**) | android server build ✓ + bank bundle ✓ + dropdown "DiffSinger" ✓ — **รอ device install + ฟังจริง** | 🔨 รอ device |

**Done:** DiffSinger render ผ่าน (tensor → wav)

---

## Sprint 3.4 — Vogen

| # | Task | Detail | Status |
|---|---|---|---|
| 3.4.1 | Model loader | .vogen (meta + model bytes) | ⬜ |
| 3.4.2 | Tensor feed | notePitches/noteDurs/phs/phDurs/f0/breAmp | ⬜ |
| 3.4.3 | VSVocoder | DecodeMgc/DecodeBap + synthesis (WORLD-based) | ⬜ |
| 3.4.4 | Test | Vogen voicebank (Mandarin/Cantonese) | ⬜ |

**Exit target:** Vogen render ผ่าน

---

## Sprint 3.5 — Cache + Performance

| # | Task | Detail | Status |
|---|---|---|---|
| 3.5.1 | Render cache เต็ม | src-/res-/cat- hash files (OpenUtau pattern) | ✅ Native chunk/render cache ใช้งานจริง |
| 3.5.2 | Tensor cache | DiffSinger mel cache | ⬜ |
| 3.5.3 | Performance | benchmark: render time/note, peak RAM (Mi Pad) | 🟡 มี native/WASM desktop metrics; Mi Pad matrix ยังไม่ครบ |
| 3.5.4 | Optimize | chunk size, thread pool, memory limits | 🟡 Chunking/cache/Web Worker pool มีแล้ว; product wiring ยังเหลือ |
| 3.5.5 | เสถียรภาพ | 30-min stress test, crash-free | ⬜ |

**Exit target:** benchmark ผ่านเป้าหมาย (1 note < 1s, RAM < 1GB)

---

## Acceptance Criteria (MS3)

- [ ] classic/worldline/v2/diffsinger/vogen สลับได้ต่อ track
- [ ] golden test ผ่านทุก renderer
- [ ] cache ลด render time (hit > 50%)
- [ ] benchmark: 1 note < 1s, RAM < 1GB
