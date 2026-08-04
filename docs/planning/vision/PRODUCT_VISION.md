# Android Voice Synth — Product Vision

**สถานะ:** Draft v1 — 2026-08-02

---

## Vision Statement

สร้าง **singing synthesis app บน Android** ที่ใช้เสียงร้องจาก voicebank มาตรฐาน (OpenUtau/UTAU format) ได้โดยตรง แต่มี **engine ใหม่** ที่ออกแบบเพื่อ mobile-first — render เร็ว ใช้ RAM น้อย และ extensible ด้วยระบบ plugin renderer

## ปัญหาที่แก้

| ปัญหา | ผลกระทบ |
|---|---|
| OpenUtau Desktop/Mobile = engine C# เดิม ออกแบบเพื่อ desktop | render ทั้ง phrase ก่อนเล่น, RAM สูง, แก้ note แล้ว re-render ทั้งหมด |
| งาน port หลายภาษา (C#, C++, Python) | maintain ยาก |
| Enunu/Voicevox ต้อง Python server | APK ใหญ่ +300MB |
| User ต้องการใช้ voicebank เดิมที่สะสมไว้ | ต้อง compat format เดิม |

## แนวทาง (How)

```
1. ENGINE ใหม่ (Rust core)
   ├─ Project model + commands + undo/redo
   ├─ Global RenderContext (curves/metadata/settings)
   ├─ Scheduler + Chunker (incremental/streaming render)
   └─ Mixer

2. RENDERER = Plugin .so (C ABI)
   ├─ libworldline.so (C++ WORLD kernel — vendor)
   ├─ libdiffsinger.so / libvogen.so / libenunu.so (ONNX)
   └─ capability declaration → core จับคู่ + เตรียม feed

3. FEED ทั้งหมดอยู่ core (12 transformers)
   ├─ phonemizer → oto mapper → tokenizer
   ├─ pitch/curve sampling (points+eqn → dense per-frame)
   └─ plugin เหลือ pure renderer

4. VOICEBANK COMPAT
   ├─ oto.ini / character.txt / config.yaml / prefix.map
   └─ project .ustx (YAML) — version 0.9

5. STORAGE — Kotlin layer เดียว
   ├─ sandbox-first (ทุกอย่างใน filesDir)
   └─ SAF เฉพาะ import/export
```

## Rendering Systems

```
1. VOICEBANK-BASED (main)   worldline v1/v2 + classic mode
2. MODEL-BASED              DiffSinger → Vogen → Enunu (ONNX)
3. EXPERIMENTAL (แยก lane)  neural enhancer (ปรับ params ก่อน render)
```

## Target Users

- ผู้ใช้ UTAU/OpenUtau ที่มี voicebank อยู่แล้ว อยากใช้บนมือถือ
- ผู้ที่ต้องการ AI singing (DiffSinger) บน Android
- ผู้พัฒนา/นักวิจัยที่อยากต่อยอด plugin renderer

## Success Criteria (MS1)

- [ ] libworldline.so รันบน Android (synth 1 note → .wav ได้ยิน)
- [ ] Rust core skeleton + JNI bridge ทำงาน
- [ ] ใช้ voicebank จริง (oto.ini + wav) ได้

## Out of Scope (ตอนนี้)

- UI mobile (Flutter) — **deferred: user จะสั่งอีกทีเมื่อต้องการทำ**
  - Design reference: **VocalSona** (UI สวยกว่าแนวทางอื่น — อ้างอิงตอนออกแบบ)
  - Tablet target (Mi Pad 6/8) + pen-first (palm rejection = hardware ของ active pen)
- neural enhancer — experimental lane
- Voicevox engine (JP อย่างเดียว, ROI ต่ำ)
