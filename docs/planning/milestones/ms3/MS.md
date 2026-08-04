# MS3 — Renderers เต็ม

**สถานะ:** Not started
**ระยะเวลา:** 5 sprints
**Requires:** MS2 (feed + scheduler ทำงาน)

---

## Goal

Renderer ทั้งหมด + คุณภาพเสียง: classic mode, worldline v2 (neural vocoder), DiffSinger, Vogen — สลับ renderer ต่อ track ได้

---

## Sprint 3.1 — Worldline Classic Mode

| # | Task | Detail | Status |
|---|---|---|---|
| 3.1.1 | Classic API export | Resample() per-phone + concat ใน worldline C++ | ⬜ |
| 3.1.2 | Classic feeder (Rust) | ResamplerItem build + SharpWavtool port (crossfade/envelope) | ⬜ |
| 3.1.3 | Flags compat | g/B/H/P/Mt/Mb/Mv full | ⬜ |
| 3.1.4 | Golden test | เทียบกับ OpenUtau classic render | ⬜ |

**Done:** classic mode เสียงเทียบ OpenUtau ได้

---

## Sprint 3.2 — Worldline v2 (Neural Vocoder)

| # | Task | Detail | Status |
|---|---|---|---|
| 3.2.1 | Vocoder package | nsf-hifigan (vocoder.yaml + onnx) — dependency path | ⬜ |
| 3.2.2 | Mel conversion | WORLD features → mel (ONNX) | ⬜ |
| 3.2.3 | v2 pipeline | f0/sp/ap → mel → hifigan → audio (+ease in/out) | ⬜ |
| 3.2.4 | Golden test | เทียบ OpenUtau worldline2 | ⬜ |

**Done:** v2 เสียงคุณภาพสูง เทียบ golden ผ่าน

---

## Sprint 3.3 — DiffSinger

| # | Task | Detail | Status |
|---|---|---|---|
| 3.3.1 | onnxruntime | vendored .so + ort crate (Android support) | ⬜ |
| 3.3.2 | DsSinger | dsconfig.yaml + phonemes.txt + tokenizer | ⬜ |
| 3.3.3 | Tensor feed | tokens/durations/f0 (+gender/velocity embeds) | ⬜ |
| 3.3.4 | Variance/Pitch predictor | dsvariance + dspitch (optional) | ⬜ |
| 3.3.5 | Renderer | acoustic + vocoder session run | ⬜ |
| 3.3.6 | Test | ใช้ DiffSinger voicebank จริง (โหลด test bank) | ⬜ |

**Done:** DiffSinger render ผ่าน (tensor → wav)

---

## Sprint 3.4 — Vogen

| # | Task | Detail | Status |
|---|---|---|---|
| 3.4.1 | Model loader | .vogen (meta + model bytes) | ⬜ |
| 3.4.2 | Tensor feed | notePitches/noteDurs/phs/phDurs/f0/breAmp | ⬜ |
| 3.4.3 | VSVocoder | DecodeMgc/DecodeBap + synthesis (WORLD-based) | ⬜ |
| 3.4.4 | Test | Vogen voicebank (Mandarin/Cantonese) | ⬜ |

**Done:** Vogen render ผ่าน

---

## Sprint 3.5 — Cache + Performance

| # | Task | Detail | Status |
|---|---|---|---|
| 3.5.1 | Render cache เต็ม | src-/res-/cat- hash files (OpenUtau pattern) | ⬜ |
| 3.5.2 | Tensor cache | DiffSinger mel cache | ⬜ |
| 3.5.3 | Performance | benchmark: render time/note, peak RAM (Mi Pad) | ⬜ |
| 3.5.4 | Optimize | chunk size, thread pool, memory limits | ⬜ |
| 3.5.5 | เสถียรภาพ | 30-min stress test, crash-free | ⬜ |

**Done:** benchmark ผ่านเป้าหมาย (1 note < 1s, RAM < 1GB)

---

## Acceptance Criteria (MS3)

- [ ] classic/worldline/v2/diffsinger/vogen สลับได้ต่อ track
- [ ] golden test ผ่านทุก renderer
- [ ] cache ลด render time (hit > 50%)
- [ ] benchmark: 1 note < 1s, RAM < 1GB
