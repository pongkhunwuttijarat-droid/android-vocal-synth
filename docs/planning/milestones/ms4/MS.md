# MS4 — Productize (UI + App)

**สถานะ:** 🟡 In progress — Flutter client ใช้งานได้; browser editor เป็น vertical slice
**Requires:** MS3

---

## Goal

App ที่ใช้ได้จริงบน Mi Pad 6/8 — UI tablet + pen-first (อ้างอิง VocalSona)

## Current checkpoint (2026-09-12)

แผนเดิมเขียนก่อน Flutter app และ WASM target ถูกลงมือจริง จึงมีสอง product path:

1. **Flutter/Android:** editor, piano roll, render/play/stop, mixer post-FX,
   project save/open/autosave, voicebank list และ WAV export มี implementation แล้ว;
   app เชื่อม engine ผ่าน LAN `synth-server` แทน MethodChannel ในแผนเดิม.
2. **Static Web/WASM:** browser editor ผ่าน headless vertical-slice test สำหรับ
   boot/canvas/edit/undo/draw/marquee/render/zoom. Worker pool ผ่าน test แยก แต่ยัง
   ไม่ได้ wire เข้าหน้า editor; OPFS product flow, curves, multi-track mix, PWA และ
   deploy ยังเหลือ.

ดังนั้น MS4 ยังไม่ complete จน acceptance ด้าน import/export, pen hardware และ
renderer switching ครบใน target ที่เลือกส่งมอบ.

---

## Sprint 4.1 — Flutter UI

| # | Task | Detail | Status |
|---|---|---|---|
| 4.1.1 | App skeleton | Flutter project + tablet landscape layout | ✅ |
| 4.1.2 | Piano roll | notes grid + pen drawing (CustomPainter + stylus events) | ✅ |
| 4.1.3 | Track panel | singer, volume, mute/solo | 🟡 Track UI มีแล้ว; per-track audio semantics ยังไม่ครบ |
| 4.1.4 | Parameter panel | curves (PITD/DYN/GENC) — pen วาด | 🟡 Pitch/FX UI มีบางส่วน; lane set ยังไม่ครบ |
| 4.1.5 | Transport | play/stop/BPM/position | ✅ Sync กับ audio position/duration จริง |
| 4.1.6 | Engine client | เชื่อม editor กับ engine | ✅ ใช้ LAN HTTP client (แทน FakeEngine/MethodChannel) |

**Checkpoint:** Flutter UI และ live engine client ใช้งานได้; per-track/curve breadth ยังไม่ครบ

---

## Sprint 4.2 — Wire + Playback

| # | Task | Detail | Status |
|---|---|---|---|
| 4.2.1 | Engine wire v1 | Editor → engine contract | ✅ LAN HTTP; offline/native wire ยังเป็น follow-up |
| 4.2.2 | Playback | render แล้ว play/stop + position sync | ✅ (`audioplayers`) |
| 4.2.3 | Render progress UI | progress bar + cancel | 🟡 Render state/cancel มี; chunk progress UI ยังไม่ครบ |
| 4.2.4 | ฟังจริงบน Mi Pad | end-to-end: วาดโน้ต → render → ฟัง | ✅ Device verification ผ่าน |

**Checkpoint:** วาดโน้ตบน tablet → render ผ่าน LAN server → ได้ยินเสียงจริง

---

## Sprint 4.3 — Import/Export + Voicebank Manager

| # | Task | Detail | Status |
|---|---|---|---|
| 4.3.1 | Import voicebank | SAF picker → copy sandbox → scan | ⬜ |
| 4.3.2 | Export | WAV + SAF save dialog | 🟡 WAV save บน Android มีแล้ว; SAF flow ยังไม่ครบ |
| 4.3.3 | Singer list UI | character image + config + renderer select | 🟡 Live voicebank list/engine dropdown มี; import flow ยังไม่ครบ |
| 4.3.4 | Project save/load | .ustx ผ่าน StorageService | ✅ Server codec + save/open/autosave |

**Exit target:** import Teto ผ่าน SAF → เลือกใน app → render ได้

---

## Sprint 4.4 — Enunu + Polish

| # | Task | Detail | Status |
|---|---|---|---|
| 4.4.1 | EnunuOnnx | ONNX models (ไม่ต้อง Python) | ⬜ |
| 4.4.2 | Pen polish | hover, pressure, palm-rejection (hardware), snap | ⬜ |
| 4.4.3 | UX polish | VocalSona-style theme, animations, empty states | ⬜ |
| 4.4.4 | Release prep | ABI splits, size, crash reporting | ⬜ |

**Exit target:** app พร้อมใช้งานจริง

---

## Acceptance Criteria (MS4)

- [x] วาดโน้ตบน Mi Pad → render → ฟัง (LAN server path)
- [ ] import voicebank ผ่าน SAF
- [x] export WAV บน Android (SAF save UX ยังต้อง polish)
- [ ] pen UX ดี (palm rejection จาก hardware)
- [ ] สลับ renderer ได้ (worldline/diffsinger)
