# MS4 — Productize (UI + App)

**สถานะ:** Not started — **deferred (user จะสั่งอีกที)**
**Requires:** MS3

---

## Goal

App ที่ใช้ได้จริงบน Mi Pad 6/8 — UI tablet + pen-first (อ้างอิง VocalSona)

---

## Sprint 4.1 — Flutter UI

| # | Task | Detail | Status |
|---|---|---|---|
| 4.1.1 | App skeleton | Flutter project + tablet landscape layout | ⬜ |
| 4.1.2 | Piano roll | notes grid + pen drawing (CustomPainter + stylus events) | ⬜ |
| 4.1.3 | Track panel | singer, volume, mute/solo | ⬜ |
| 4.1.4 | Parameter panel | curves (PITD/DYN/GENC) — pen วาด | ⬜ |
| 4.1.5 | Transport | play/stop/BPM/position | ⬜ |
| 4.1.6 | FakeEngine | interface ใน Dart — ยังไม่ wire | ⬜ |

**Done:** UI ครบ mock data — ยังไม่ wire engine

---

## Sprint 4.2 — Wire + Playback

| # | Task | Detail | Status |
|---|---|---|---|
| 4.2.1 | MethodChannel v1 | FakeEngine → NativeEngine (contract ที่ freeze) | ⬜ |
| 4.2.2 | AudioTrack playback | streaming + seek | ⬜ |
| 4.2.3 | Render progress UI | progress bar + cancel | ⬜ |
| 4.2.4 | ฟังจริงบน Mi Pad | end-to-end: วาดโน้ต → render → ฟัง | ⬜ |

**Done:** วาดโน้ตบน tablet → ได้ยินเสียงจริง

---

## Sprint 4.3 — Import/Export + Voicebank Manager

| # | Task | Detail | Status |
|---|---|---|---|
| 4.3.1 | Import voicebank | SAF picker → copy sandbox → scan | ⬜ |
| 4.3.2 | Export | WAV + SAF save dialog | ⬜ |
| 4.3.3 | Singer list UI | character image + config + renderer select | ⬜ |
| 4.3.4 | Project save/load | .ustx ผ่าน StorageService | ⬜ |

**Done:** import Teto ผ่าน SAF → เลือกใน app → render ได้

---

## Sprint 4.4 — Enunu + Polish

| # | Task | Detail | Status |
|---|---|---|---|
| 4.4.1 | EnunuOnnx | ONNX models (ไม่ต้อง Python) | ⬜ |
| 4.4.2 | Pen polish | hover, pressure, palm-rejection (hardware), snap | ⬜ |
| 4.4.3 | UX polish | VocalSona-style theme, animations, empty states | ⬜ |
| 4.4.4 | Release prep | ABI splits, size, crash reporting | ⬜ |

**Done:** app พร้อมใช้งานจริง

---

## Acceptance Criteria (MS4)

- [ ] วาดโน้ตด้วยปากกาบน Mi Pad → render → ฟัง
- [ ] import voicebank ผ่าน SAF
- [ ] export wav
- [ ] pen UX ดี (palm rejection จาก hardware)
- [ ] สลับ renderer ได้ (worldline/diffsinger)
