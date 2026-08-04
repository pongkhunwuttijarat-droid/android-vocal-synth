# Roadmap

> ลำดับการพัฒนา Android Voice Synth — engine-first
> อัปเดต: 2026-08-02

---

## Milestones Overview

```
MS1 ────────► MS2 ────────► MS3 ────────► MS4
Main Engine    Worldline      Renderers      Productize
(Rust core)    Integration    เต็ม           (UI — deferred)
+ plugin track  + golden test
(คู่ขนาน)
```

---

## MS1 — Main Engine Core (engine-first)

**เป้าหมาย:** Rust core ครบ — domain + commands + phonemizer + feed + runtime (test ได้โดยไม่ต้อง .so)

| Sprint | เนื้อหา | Status |
|---|---|---|
| 1.1 | Domain model (.ustx compat) + TimeAxis + expressions | ⬜ |
| 1.2 | Command system + undo/redo | ⬜ |
| 1.3 | Phonemizer (JP VCV / EN CVVC) + oto parser | ⬜ |
| 1.4 | Feed pipeline (12 transformers) + RenderInput | ⬜ |
| 1.5 | Runtime: scheduler/chunker/cache/mixer | ⬜ |
| P.1-P.4 | Plugin track (คู่ขนาน): plugin_abi.h, FFI smoke, NDK build, golden ref | ⬜ |

**Done:** engine core ครบ + FFI smoke test ผ่าน (prebuilt .so)

## MS2 — Worldline Integration + Test

**เป้าหมาย:** engine ↔ libworldline.so → render .ustx → golden ผ่าน → Android

| Sprint | เนื้อหา | Status |
|---|---|---|
| 2.1 | Worldline plugin crate + NDK build | ⬜ |
| 2.2 | Render full project (phrase grouping + mixer) | ⬜ |
| 2.3 | Golden test vs OpenUtau desktop | ⬜ |
| 2.4 | LAN API + Android (Mi Pad) | ⬜ |

## MS3 — Renderers เต็ม

| Sprint | เนื้อหา | Status |
|---|---|---|
| 3.1 | classic mode (UTAU flags compat) | ⬜ |
| 3.2 | worldline v2 (WORLD + NSF-HiFiGAN) | ⬜ |
| 3.3 | DiffSinger (onnxruntime + variance/pitch) | ⬜ |
| 3.4 | Vogen (f0_man + VSVocoder) | ⬜ |
| 3.5 | Cache + performance (1 note < 1s, RAM < 1GB) | ⬜ |

## MS4 — Productize (deferred — user จะสั่งอีกที)

| Sprint | เนื้อหา | Status |
|---|---|---|
| 4.1 | Flutter UI tablet pen-first (อ้างอิง VocalSona) | ⬜ |
| 4.2 | Wire MethodChannel + playback | ⬜ |
| 4.3 | Import/Export (SAF) + voicebank manager | ⬜ |
| 4.4 | Enunu + polish | ⬜ |

---

## เหตุผล engine-first

```
1. .so บน Android = พิสูจน์แล้ว (OpenUtau Mobile prebuilt + เคยใช้)
   → ไม่ต้อง "พิสูจน์ความเสี่ยง" ก่อน
2. Engine = งานใหญ่สุด + test ได้เอง (cargo test ไม่ต้อง .so)
3. Feed/domain เป็นของที่ทุก plugin ต้องใช้ → ทำก่อนเสมอ
4. Plugin ต่อท้ายได้เลย (มี contract + prebuilt .so)
```
