# Rendering Systems

> ระบบ render ทั้งหมดในโปรเจกต์ + สถานะ
> อัปเดต: 2026-09-12

---

## สรุป: 2 ระบบหลัก + 1 experimental

```
┌────────────────────────────────────────────────────────────┐
│  1. VOICEBANK-BASED (main — เริ่มที่ตัวนี้)                  │
│  └── libworldline.so (รวม classic แล้ว)                    │
│      ├── mode "classic"    ← compat UTAU-style            │
│      ├── mode "worldline"  ← v1: WORLD synthesis          │
│      └── mode "worldline2" ← v2: WORLD + NSF-HiFiGAN      │ ← ใช้ตัวนี้
│                                                           │
│  2. MODEL-BASED (main — ทำทีหลัง)                          │
│  ├── DiffSinger (acoustic + vocoder ONNX)                 │
│  ├── Vogen (f0_man + singer model)                        │
│  └── Enunu (EnunuOnnx — ไม่ต้อง Python)                   │
│                                                           │
│  3. HYBRID ENHANCER (EXPERIMENTAL — แยกไปอีก lane)        │
│  └── neural net เล็กๆ ปรับ params ก่อนเข้า WORLD           │
│      ⚠️ ยังไม่ทำตอนนี้ — เก็บเป็นแนวคิดไว้                  │
└────────────────────────────────────────────────────────────┘
```

---

## Current decision (2026-09-12)

| เรื่อง | สรุป |
|---|---|
| **Classic + Worldline** | รวมเป็น libworldline.so ตัวเดียว (classic = feeder mode) |
| **Worldline v2** | main path ของ voicebank-based — WORLD features + neural vocoder |
| **Enhancer (neural param adjuster)** | **experimental — แยกไปอีก lane** ไม่ทำตอนนี้ |
| **Main engine** | ทำใหม่เกือบหมด (Rust core) — plugin เหลือแค่ pure renderer |
| **Model-based** | ทำทีหลัง (DiffSinger เริ่มก่อน) |

สถานะ implementation ตอนนี้: native R1/R2 และ DiffSinger tensor→WAV ใช้งานได้,
pure-Rust `worldline-rs`/Classic ใช้บน WASM ได้, MoE อยู่ experimental lane,
Vogen ยังไม่เริ่ม และ browser editor ยังเป็น vertical slice. รายละเอียด acceptance
อยู่ที่ [Roadmap](../planning/roadmap/ROADMAP.md).

---

## 1. Voicebank-Based (libworldline.so)

### Modes

| Mode | Pipeline | ใช้เมื่อ |
|---|---|---|
| `classic` | ResamplerItem → Resample() per-phone → concat | voicebank เก่าที่ต้อง compat |
| `worldline` | PhraseSynth (WORLD analysis + synthesis) | default |
| `worldline2` | PhraseSynth → mel (ONNX) → NSF-HiFiGAN → audio | **คุณภาพสูงสุด** |

### v2 — implementation ปัจจุบัน

```
- vocoder package (nsf-hifigan): โหลดผ่าน native R2
- core จัดหา model path ให้ plugin (ผ่าน capability needs_vocoder)
- mel model (world features → mel) + HiFi-GAN — golden v2 ผ่าน
```

---

## 2. Model-Based

| Engine | Models | หมายเหตุ |
|---|---|---|
| DiffSinger | acoustic + vocoder + (pitch/variance optional) | tensor→WAV ผ่าน; timing/device gate เหลือ |
| Vogen | f0_man + singer model | ยังไม่เริ่ม |
| Enunu | EnunuOnnx (ONNX) | ยังเป็น future lane |

---

## 3. Experimental: Hybrid Enhancer (เลื่อน)

**แนวคิด:** neural net เล็กๆ ปรับ f0/dynamics/breathiness ก่อนเข้า WORLD renderer — ยังใช้ voicebank เดิม

**สถานะ:** ไม่ทำตอนนี้ — main engine ใหม่ยังไม่เสร็จ เก็บไว้เป็น future lane

**ถ้าทำทีหลัง:**
```
plugin kind: PLUGIN_KIND_ENHANCER
pipeline:    RenderInput → enhancer.predict() → adjusted → worldline.render()
```

---

## Roadmap / historical ordering

```
Phase 1: libworldline.so v1 (WORLD synthesis)     ← พิสูจน์ core + FFI
Phase 2: mode classic (compat)
Phase 3: mode worldline2 (neural vocoder)          ← main target
Phase 4: model-based (DiffSinger → Vogen → Enunu)
Phase 5: (future) hybrid enhancer — แยก lane
```
