# Runtime Engine Plugin Possibilities

> วิเคราะห์ความเป็นไปได้ในการ port แต่ละ engine มาลง Android app
> วันที่: 2026-08-02

---

## สรุปโดยรวม

| Engine | ชนิด | Approach ที่แนะนำ | Port ยาก | Mobile เหมาะ? |
|---|---|---|---|---|
| **Worldline** | WORLD C++ | NDK build + JNI | 🔴 สูง | ✅ |
| **Classic** | Resampler | in-process (WorldlineResampler/SharpWavtool) | 🟡 กลาง | ✅ |
| **DiffSinger** | AI ONNX | onnxruntime-android | 🟢 ต่ำ | ✅ |
| **Vogen** | AI ONNX | onnxruntime-android + VSVocoder | 🟢 ต่ำ | ✅ |
| **Enunu** | Python + PyTorch | subprocess / Chaquopy | 🔴 สูง | ⚠️ |
| **Voicevox** | HTTP server | subprocess | 🔴 สูง | ⚠️ |

---

## 1. Worldline Renderer (WORLD vocoder)

**รายละเอียด:**
- C++ WORLD vocoder + OpenUtau worldline wrapper
- Source: `ref/desktop-ref/cpp/worldline/` + https://github.com/mmorise/World

**Dependencies:**
- WORLD (mmorise/World) — pure C
- libpyin, libgvps, spline, libnpy, xxhash — pure C/C++
- miniaudio, absl — desktop only, **ต้องตัดทิ้ง**

**Approach:**
```
1. Bazel → CMakeLists.txt
2. ตัด audio_output.h/cc, worldline_main.cpp, audio_debug.cc
3. เขียน JNI bridge (worldline_jni.cpp)
4. Android NDK cross-compile → libworldline.so
```

**ความเสี่ยง:**
- C++ build บน NDK (ต้องจัดการ toolchain)
- 16KB page size บน Android 15+ (ต้อง rebuild ด้วย NDK r27+)
- ยังไม่รองรับ DiffSinger-style expression curves ครบ (มี curves แล้ว)

**ขนาด:** ~1-3MB .so + voicebank wav files

**Feasibility: 🟢 เป็นไปได้ (งานเยอะแต่เส้นทางชัดเจน)**

---

## 2. Classic Renderer (UTAU resampler)

**รายละเอียด:**
- UTAU-compatible: IResampler + IWavtool
- External exe บน desktop — **บน Android ใช้ไม่ได้** ต้อง in-process

**Approach:**
```
Option A: ใช้ WorldlineResampler (C++ WORLD) เป็น resampler + SharpWavtool (C#→Kotlin port)
Option B: port resampler interface เป็น Kotlin + ใช้ใน-process WORLD
```

**Key files:**
- `ClassicRenderer.cs` → ClassicRenderer.kt
- `ResamplerItem.cs` → ResamplerItem.kt
- `SharpWavtool.cs` → SharpWavtool.kt (crossfade + envelope — pure DSP)
- `VoicebankLoader.cs` → VoicebankLoader.kt

**Feasibility: 🟡 กลาง — ต้อง port SharpWavtool (DSP) แต่ถ้าใช้ WORLD เป็น backend ก็ง่ายขึ้น**

---

## 3. DiffSinger Renderer (AI Neural)

**รายละเอียด:**
- ONNX models: acoustic + vocoder + optional (pitch/variance/linguistic)
- Input: tensors (tokens, durations, f0, + optional features)
- Output: waveform

**Approach:**
```
1. ใช้ onnxruntime-android (official AAR จาก Maven)
2. Port tensor building logic จาก DiffSingerRenderer.cs → Kotlin
3. Port DiffSingerUtils.cs (durations, sampling, padding)
4. Port DiffSingerSinger.cs (config load, tokenization)
5. Port DiffSingerVocoder.cs / Variance / Pitch
```

**Dependencies:**
- `com.microsoft.onnxruntime:onnxruntime-android` — official
- YAML parser (kotlinx.serialization / SnakeYAML)
- ไม่ต้อง compile C++ เลย

**ความเสี่ยง:**
- Model ขนาดใหญ่ (50-300MB) — ต้องโหลดจาก storage
- Inference บน mobile CPU (Diffusion steps 4-100) — ช้าอาจต้องปรับ
- ONNX opset compatibility กับ onnxruntime-android
- Vocoder (nsf-hifigan) ต้อง download แยก

**ขนาด:** onnxruntime-android ~20-50MB + models 50-300MB

**Feasibility: 🟢 ง่ายที่สุดในบรรดา AI engines — official runtime, แค่ port logic**

---

## 4. Vogen Renderer (ONNX Neural)

**รายละเอียด:**
- ONNX model (Mandarin/Cantonese)
- VSVocoder (WORLD-based vocoder, C# in-process)

**Approach:**
```
1. onnxruntime-android สำหรับ model inference
2. Port VSVocoder จาก VocalShaper/ (C# → Kotlin หรือ C++ NDK)
   - VSVocoder = WORLD analysis + synthesis wrapper
```

**ความเสี่ยง:**
- จำกัดแค่ 2 ภาษา (Mandarin, Cantonese)
- VSVocoder เป็น WORLD-based — อาจ reuse libworldline.so ที่ build ไว้แล้ว

**Feasibility: 🟢 ง่าย ถ้า reuse WORLD .so — ไม่งั้นต้อง port VSVocoder**

---

## 5. Enunu Renderer (Python + PyTorch)

**รายละเอียด:**
- Python server (ENUNUServer) — external project (oto-oto/ENUNU)
- Protocol: **ZeroMQ REQ/REP** ผ่าน `tcp://localhost:15555` (ไม่ใช่ HTTP!)
- Request: `["acoustic", ustPath, "", vbHash, "600"]` / `["synthe", ustPath, wavPath, vbHash, "600"]`
- ใช้ PyTorch models (acoustic + synthesizer)

**Key finding — Chaquopy + torch:**
```
❌ Chaquopy รองรับ torch แค่ ~1.8.1 (issue chaquo/chaquopy#606, #1215)
❌ PyTorch ยังไม่มี official Android wheel สำหรับ Python (pytorch#140090)
❌ ENUNU ปัจจุบันต้องการ torch ใหม่กว่า 1.8.1 (ส่วนใหญ่)
✅ PEP 738 (2025) ทำให้ CPython รองรับ Android อย่างเป็นทางการ
   แต่ PyTorch ยังไม่ได้ publish android wheels
```

**Approach:**
```
Option A: Subprocess + Python runtime เต็ม (p4a / proot + Ubuntu)
   - bundle Python + torch + deps → fork/exec ENUNUServer
   - ใช้ ZMQ เดิม ไม่แตะโค้ด Python
   - ขนาด APK: Python ~50MB + torch ~200MB+ = ใหญ่

Option B: Chaquopy in-process (internal API)
   - ⚠️ torch จำกัดเวอร์ชัน 1.8.1 — ENUNU ส่วนใหญ่ใช้ไม่ได้
   - ถ้า voicebank เก่าที่ใช้ torch 1.8.x พอได้

Option C: Port ENUNU model เป็น ONNX
   - ต้อง export model ใหม่ + ตรวจ opset compat
   - งานเยอะ แต่ได้ผลลัพธ์เหมือน DiffSinger (runtime เล็ก)
```

**ความเสี่ยง:**
- torch บน Android = ปัญหาใหญ่สุด (ไม่มี official wheel)
- Python runtime + torch = APK +300MB
- RAM สูง (Python + PyTorch ~500MB-1GB)
- Startup ช้า (Python boot + model load)

**Feasibility: ⚠️ พอได้แต่แพง — ต้อง bundle Python ทั้งระบบ หรือ port เป็น ONNX**

---

## 6. Voicevox Renderer (HTTP TTS)

**รายละเอียด:**
- VOICEVOX engine (Python FastAPI server + C++ core)
- Protocol: HTTP (audio_query + synthesis)
- ใช้ได้แค่ Japanese

**Approach:**
```
Option A: subprocess + VOICEVOX engine เต็ม
   - Python + FastAPI + C++ core — ใหญ่และช้า
   
Option B: Port core เป็น onnxruntime-android
   - VOICEVOX core ใช้ ONNX models อยู่แล้ว
   - แต่ pipeline (phoneme → prosody → synthesis) ซับซ้อน
   - งานเยอะเทียบกับคุณค่าที่ได้ (แค่ภาษา JP)
```

**ความเสี่ยง:**
- Server-based → lifecycle management
- ภาษาญี่ปุ่นอย่างเดียว — ROI ต่ำ

**Feasibility: 🔴 ต่ำ — ไม่คุ้มค่า ควรใช้ DiffSinger แทน**

---

## 7. ข้อสรุปสำคัญ: Chaquopy / Python บน Android

### Chaquopy สถานะ (2026)
```
✅ รองรับ: Python 3.8-3.13, numpy, scipy, pandas, pillow
✅ pip install จาก PyPI (PEP 738 android wheels)
⚠️ torch: จำกัด 1.8.1 — ล้าสมัย
❌ ไม่มี official PyTorch Android wheel
```

### ทางเลือก Python runtime บน Android
| วิธี | ขนาด | torch | เหมาะกับ |
|---|---|---|---|
| **Chaquopy** | +30-50MB (runtime) | ⚠️ 1.8.1 เท่านั้น | numpy/scipy งานเบา |
| **p4a (python-for-android)** | +50MB runtime | ⚠️ ต้อง build เอง | Kivy apps |
| **proot + distro** | +500MB rootfs | ✅ เต็ม (pip install) | งานหนัก (seal_ws มี infra) |
| **termux-packages** | +50MB | ✅ ถ้ามี package | manual install |

---

## 8. แผนที่แนะนำ (Roadmap)

```
Phase 1 (พิสูจน์ concept):
  ┌─────────────────────────────────────────────┐
  │ Worldline (NDK) — voicebank UTAU เดิมใช้ได้  │
  │ DiffSinger (ONNX) — AI คุณภาพสูง            │
  └─────────────────────────────────────────────┘

Phase 2 (ขยาย):
  ┌─────────────────────────────────────────────┐
  │ Classic (SharpWavtool port)                 │
  │ Vogen (reuse WORLD .so + ONNX)              │
  └─────────────────────────────────────────────┘

Phase 3 (สำรวจ):
  ┌─────────────────────────────────────────────┐
  │ Enunu (ต้องตัดสินใจ: bundle Python หรือ      │
  │        port เป็น ONNX)                      │
  │ Voicevox (low priority — JP เท่านั้น)        │
  └─────────────────────────────────────────────┘
```

---

## 9. Decision Matrix

| คำถาม | คำตอบ |
|---|---|
| เริ่มจาก engine ไหนก่อน | **Worldline + DiffSinger** (ครอบคลุม UTAU + AI) |
| ใช้ Chaquopy ไหม | เฉพาะงานเบา — ไม่ใช่สำหรับ torch/ENUNU |
| Enunu ยังไงดี | subprocess + proot (มี infra) หรือ ONNX port — รอ Phase 3 |
| Voicevox จำเป็นไหม | ไม่ — DiffSinger ครอบคลุมกว่า |
| APK size budget | Worldline+DiffSinger ≈ 100-350MB ต่อ ABI |

---

## References

- Chaquopy: https://chaquo.com/chaquopy/
- torch issue: https://github.com/chaquo/chaquopy/issues/606, #1215
- PyTorch Android wheel request: https://github.com/pytorch/pytorch/issues/140090
- ENUNU: https://github.com/oto-oto/ENUNU (external)
- OpenUtau ENUNU wiki: https://github.com/openutau/OpenUtau/wiki
