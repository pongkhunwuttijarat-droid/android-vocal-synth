# Roadmap

> ลำดับการพัฒนา Android Voice Synth — engine-first
> อัปเดต: 2026-09-12

---

## Milestones Overview

```
MS1 ✅ ─────► MS2 ✅ ─────► MS3 🟡 ─────► MS4 🟡
Main Engine    Worldline      Renderers       Productize
(Rust core)    + golden       native/WASM     Flutter + Web
                  │               │               │
                  └──── pure-Rust WORLDLINE-R / WASM migration ────┘
```

---

## MS1 — Main Engine Core (engine-first)

**เป้าหมาย:** Rust core ครบ — domain + commands + phonemizer + feed + runtime (test ได้โดยไม่ต้อง .so)

| Sprint | เนื้อหา | Status |
|---|---|---|
| 1.1 | Domain model (.ustx compat) + TimeAxis + expressions | ✅ |
| 1.2 | Command system + undo/redo | ✅ |
| 1.3 | Phonemizer (JP VCV / EN CVVC) + oto parser | ✅ |
| 1.4 | Feed pipeline (12 transformers) + RenderInput | ✅ |
| 1.5 | Runtime: scheduler/chunker/cache/mixer | ✅ |
| P.1-P.4 | Plugin track (คู่ขนาน): plugin ABI, FFI smoke, NDK build, golden ref | ✅ |

**Done:** engine core ครบ + FFI smoke test ผ่าน (prebuilt .so)

## MS2 — Worldline Integration + Test

**เป้าหมาย:** engine ↔ libworldline.so → render .ustx → golden ผ่าน → Android

| Sprint | เนื้อหา | Status |
|---|---|---|
| 2.1 | Worldline plugin crate + NDK build | ✅ |
| 2.2 | Render full project (phrase grouping + mixer) | ✅ |
| 2.3 | Golden test vs OpenUtau desktop | ✅ Current acceptance; ดูหมายเหตุ timing/alignment ใน MS2 |
| 2.4 | LAN API + Android (Mi Pad) | ✅ |

## MS3 — Renderers เต็ม

| Sprint | เนื้อหา | Status |
|---|---|---|
| 3.1 | classic mode (UTAU flags compat) | 🟡 Pure-Rust resampler parity ผ่าน; flags ยังไม่ครบ |
| 3.2 | worldline v2 (WORLD + NSF-HiFiGAN) | ✅ Native + golden |
| 3.3 | DiffSinger (onnxruntime + variance/pitch) | 🟡 Tensor→WAV ผ่าน; timing/device ยังเหลือ |
| 3.4 | Vogen (f0_man + VSVocoder) | ⬜ |
| 3.5 | Cache + performance (1 note < 1s, RAM < 1GB) | 🟡 Cache/desktop benchmark ผ่านบางส่วน; device/stress gate ยังเหลือ |

## MS4 — Productize (กำลังทำ)

| Sprint | เนื้อหา | Status |
|---|---|---|
| 4.1 | Flutter UI tablet pen-first (อ้างอิง VocalSona) | 🟡 Editor ใช้งานและแก้โน้ตได้ |
| 4.2 | Wire engine + playback | 🟡 ใช้ LAN synth-server; ยังไม่ใช่ final offline/native wire |
| 4.3 | Import/Export (SAF) + voicebank manager | 🟡 Project save/open/autosave + WAV save + live list มีแล้ว; SAF import flow ยังเหลือ |
| 4.4 | Enunu + polish | ⬜ |

## WASM/Web migration track

| งาน | Status |
|---|---|
| Pure-Rust WORLDLINE-R + Classic kernel | ✅ เทียบ native `.so` ผ่าน |
| `engine-wasm` in-memory project/command/render API | ✅ 11/11 tests ผ่าน (2026-09-12) |
| Browser engine smoke | ✅ PCM/WAV ตรง native fixture |
| Worker pool + per-chunk loading/incremental cache | ✅ Test harness ผ่านและ bit-identical |
| Browser editor vertical slice | ✅ Boot/canvas/edit/undo/draw/marquee/render/zoom ผ่าน headless test |
| ต่อ editor เข้ากับ RenderPool + progress/cancel | 🟡 Pool มีแล้ว แต่ editor ยัง render main thread |
| OPFS project/voicebank flow, multi-track/mixer, curves | ⬜ |
| Service worker/PWA, deploy, neural engines บน WASM | ⬜ |

อ้างอิง: [WASM migration status](../wasm-migration-status.md) ·
[Web UI implementation plan](../web-ui-design.md)

## Verification snapshot (2026-09-12)

| ชุดตรวจ | ผล |
|---|---|
| `cargo test -p engine-wasm` | ✅ 11/11 |
| `web/run-smoke.sh` | ✅ PCM/WAV fixture PASS |
| `web/run-pool.sh` | ✅ cold/cache/one-note incremental bit-identical |
| `web/editor-test.html` (headless Chromium) | ✅ Vertical slice PASS |
| `flutter test --no-pub` | 🟡 48 pass, 9 skipped, 2 fail ใน `note_edit_test.dart` |

Flutter failures ปัจจุบันอยู่ที่ gesture test ของ lyric tap และ right-edge resize:
log แสดงว่า test หา vertical `Scrollable`/พิกัด roll ผิดหลัง layout มี nested scroll
(`resize=null`, dialog ไม่เปิด). จึงจัดเป็น **test-harness/integration issue ที่ต้อง
วินิจฉัยต่อ** ยังไม่ใช้เป็นหลักฐานว่าฟีเจอร์บน device regression และไม่ปิด MS4.

---

## เหตุผล engine-first

```
1. .so บน Android = พิสูจน์แล้ว (OpenUtau Mobile prebuilt + เคยใช้)
   → ไม่ต้อง "พิสูจน์ความเสี่ยง" ก่อน
2. Engine = งานใหญ่สุด + test ได้เอง (cargo test ไม่ต้อง .so)
3. Feed/domain เป็นของที่ทุก plugin ต้องใช้ → ทำก่อนเสมอ
4. Plugin ต่อท้ายได้เลย (มี contract + prebuilt .so)
```
