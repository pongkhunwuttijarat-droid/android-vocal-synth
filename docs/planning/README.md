# Planning

> Vision, Roadmap, Milestones, Sprints สำหรับ Android Voice Synth

เอกสารนี้เก็บเป้าหมายและสถานะ milestone โดยรวม; สำหรับงานที่กำลังทำและข้อมูล
ส่งต่อข้าม session/harness ให้ดู [Agent Task Board](../work/BOARD.md).

---

## Documents

| เอกสาร | เนื้อหา |
|---|---|
| [Vision](vision/PRODUCT_VISION.md) | ทำไมต้องทำ, แนวทาง, target users |
| [Roadmap](roadmap/ROADMAP.md) | MS1-MS4 + ลำดับ dependency |
| [CONTRACT-v2 freeze proposal](contract-v2-freeze-proposal.md) | Draft blockers, adapter acceptance gates and decisions before freezing Project Session API |
| [Contract v2 extension proposal (T014)](contract-v2-extension-t014.md) | Proposed structural track commands, per-note properties and bounded reads; §6 stays frozen, nothing here is implemented |
| [CONTRACT-MK3 proposal](contract-mk3-proposal.md) | Single draft for progressive streaming, SoundCraft/RustySynth instruments, mixer inserts, legacy USTX MixFx, and `.rustx` interoperability; unapproved |
| [High-frequency editing flow](high-frequency-editing-flow.md) | How curve/note previews become one Rust-authoritative mutation on pointer-up |
| [T009 curve stroke spec](../work/tasks/T009-curve-stroke-streaming-simplification.md) | OpenUtau-compatible curve simplification, safe preview reduction and commit/test plan |
| [Milestones](milestones/) | รายละเอียดแต่ละ milestone + sprints |

## Milestone Status

| MS | ชื่อ | Sprints | สถานะ |
|---|---|---|---|
| [MS1](milestones/ms1/MS.md) | Main engine core | 5 + plugin track | ✅ Complete |
| [MS2](milestones/ms2/MS.md) | WORLDLINE + golden + Android | 4 | ✅ Core acceptance complete |
| [MS3](milestones/ms3/MS.md) | Renderer เต็ม (classic/v2/DiffSinger/Vogen) | 5 | 🟡 In progress |
| [MS4](milestones/ms4/MS.md) | Productize (Flutter + Web, tablet pen-first) | 4 + web phases | 🟡 In progress |

งาน WASM/Web เป็น migration track ที่พาดผ่าน MS3-MS4 ไม่ใช่ milestone ใหม่:
pure-Rust WORLDLINE-R และ `engine-wasm` ผ่าน parity แล้ว ส่วน browser editor อยู่ระดับ
vertical slice และยังต้องต่อ worker pool/OPFS/multi-track/PWA ให้ครบ product flow.

## Sprint Board

| Sprint | เป้าหมาย | Status |
|---|---|---|
| 1.1-1.5 | Domain, commands, phonemizer, feed, runtime | ✅ |
| P.1-P.4 | Plugin ABI/FFI/NDK/golden reference | ✅ |
| 2.1-2.4 | WORLDLINE integration + golden + Android | ✅ |
| 3.1 | Classic mode | 🟡 Rust parity ผ่าน; flags/product integration ยังไม่ครบ |
| 3.2 | WORLDLINE v2 | ✅ Native render + golden |
| 3.3 | DiffSinger | 🟡 Tensor→WAV ผ่าน; timing/device acceptance ยังเหลือ |
| 3.4 | Vogen | ⬜ |
| 3.5 | Cache + performance | 🟡 Native cache และ WASM worker/cache พิสูจน์แล้ว; device/stress gate ยังเหลือ |
| 4.1-4.4 | Flutter productization | 🟡 Editor/render path ใช้ได้; import/export/release acceptance ยังเหลือ |
| Web P0-P8 | Static WASM editor | 🟡 P0 core ผ่าน (เหลือ phonemize API), editor vertical slice ผ่าน; P3-P8 ยังไม่ครบ |

## ความคืบหน้าปัจจุบัน

```
✅ Rust engine core + command/undo + feed/runtime
✅ Native R1/R2, synth-cli/server, Android device render และ golden comparison
✅ Pure-Rust WORLDLINE-R + Classic kernel + wasm32 engine parity
✅ Web smoke/pool tests และ browser editor vertical slice
🟡 DiffSinger timing/device acceptance; Vogen ยังไม่เริ่ม
🟡 Web product flow: ต่อ RenderPool, OPFS, multi-track, curves, PWA/deploy
```

รายละเอียดล่าสุด: [Roadmap](roadmap/ROADMAP.md) ·
[WASM migration](wasm-migration-status.md) · [Web UI](web-ui-design.md)
