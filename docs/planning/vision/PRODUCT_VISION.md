# Rusted UTAU / Android Voice Synth — Product Vision

**สถานะ:** Revised — 2026-09-12

---

## Vision Statement

สร้าง **singing synthesis engine + pen-first editor** ที่ใช้ voicebank มาตรฐาน
OpenUtau/UTAU ได้โดยตรง ทำงานได้ทั้ง native/Android และ static web โดยแชร์ Rust
domain/feed/render pipeline เดียวกัน — render แบบ incremental, ใช้ทรัพยากรเหมาะกับ
tablet และต่อ renderer ใหม่ได้.

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

2. RENDERER = native plugin หรือ pure-Rust backend
   ├─ native: libworldline.so + ONNX renderers (R2/DiffSinger/MoE)
   ├─ portable: worldline-rs + classic → wasm32 โดยไม่มี dlopen
   └─ capability declaration → core จับคู่ + เตรียม feed

3. FEED ทั้งหมดอยู่ core (12 transformers)
   ├─ phonemizer → oto mapper → tokenizer
   ├─ pitch/curve sampling (points+eqn → dense per-frame)
   └─ plugin เหลือ pure renderer

4. VOICEBANK COMPAT
   ├─ oto.ini / character.txt / config.yaml / prefix.map
   └─ project .ustx (YAML) — version 0.9

5. HOST ADAPTERS
   ├─ Android: app storage + LAN synth-server (SAF import/export เป็น product gap)
   └─ Web: OPFS/HTTP sample adapters + wasm-bindgen (product flow กำลังทำ)
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

## Proven foundation

- [x] libworldline.so รันบน Android และ render/listen บน Mi Pad ผ่าน
- [x] Rust core + commands/undo + phonemizer/feed/runtime ทำงาน
- [x] ใช้ voicebank จริง (oto.ini + wav/frq) และ .ustx ได้
- [x] pure-Rust WORLDLINE-R compile/run บน WASM และเทียบ native ผ่าน
- [x] Flutter client และ browser editor vertical slice ใช้งานได้

เกณฑ์ product completion ที่ยังเหลืออยู่ใน
[Roadmap](../roadmap/ROADMAP.md): import/export/voicebank flow, multi-track/curves,
tablet pen QA, PWA/deploy และ renderer acceptance ที่ยังไม่ครบ.

## Out of scope / later

- Cloud sync, collaboration, VST และ MIDI/recording
- Training neural models ใน browser (ต้อง WebGPU spike แยก)
- Neural enhancer — experimental lane
- Voicevox engine (JP อย่างเดียว, ROI ต่ำ)
