# Planning

> Vision, Roadmap, Milestones, Sprints สำหรับ Android Voice Synth

---

## Documents

| เอกสาร | เนื้อหา |
|---|---|
| [Vision](vision/PRODUCT_VISION.md) | ทำไมต้องทำ, แนวทาง, target users |
| [Roadmap](roadmap/ROADMAP.md) | MS1-MS4 + ลำดับ dependency |
| [Milestones](milestones/) | รายละเอียดแต่ละ milestone + sprints |

## Milestone Status

| MS | ชื่อ | Sprints | สถานะ |
|---|---|---|---|
| [MS1](milestones/ms1/MS.md) | POC: worldline .so + Rust skeleton | 4 (1.1-1.4) | ⬜ In planning |
| [MS2](milestones/ms2/MS.md) | Core Engine เต็ม | 6 (2.1-2.6) | ⬜ |
| [MS3](milestones/ms3/MS.md) | Renderer เต็ม (v2/DiffSinger/Vogen) | 5 (3.1-3.5) | ⬜ |
| [MS4](milestones/ms4/MS.md) | Productize (UI tablet pen-first) | 4 (4.1-4.4) | ⬜ Deferred |

## Sprint Board

| Sprint | เป้าหมาย | Status |
|---|---|---|
| **1.1** | Domain model (.ustx compat) — engine-first | ⬜ **ถัดไป** |
| 1.2 | Command system + undo/redo | ⬜ |
| 1.3 | Phonemizer + voicebank loading (Teto) | ⬜ |
| 1.4 | Feed pipeline (12 transformers) | ⬜ |
| 1.5 | Runtime: scheduler/chunker/cache/mixer | ⬜ |
| P.1-P.4 | Plugin track คู่ขนาน (abi/FFI/NDK/golden ref) | ⬜ |
| 2.1-2.4 | Worldline integration + golden + Android | ⬜ |
| 3.1-3.5 | classic/v2/DiffSinger/Vogen/cache | ⬜ |
| 4.1-4.4 | UI/wire/import/export/polish | ⬜ |

## ความคืบหน้าปัจจุบัน

```
✅ Design/Architecture — docs ครบ
✅ Voicebank — Teto English CVVC (test/golden/teto-english)
✅ Planning — MS1-MS4 + sprints (engine-first)
✅ พิสูจน์ .so — OpenUtau Mobile prebuilt (arm64) พร้อมใช้
⬜ Sprint 1.1 — เริ่มได้เลย
```
