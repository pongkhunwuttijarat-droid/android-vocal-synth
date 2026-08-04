# Architecture Decision — Native .so Engine Libraries

**วันที่:** 2026-08-02
**สถานะ:** ตัดสินใจแล้ว (Decided)

---

## Decision

ทุก renderer engine ถูก build เป็น **native shared library (.so)** ด้วยภาษา **C++** เดียว
Kotlin ทำหน้าที่เป็น bridge (JNI) + engine core (project model, phonemizer, scheduler) เท่านั้น

## เหตุผล

| ปัจจัย | ผล |
|---|---|
| แต่ละ engine มาจากคนละภาษา (C++, C#, Python) | ถ้า port แยกภาษา = maintain หลายภาษา |
| Python runtime (Enunu/Voicevox) = APK +300MB | ไม่ต้องใช้ — EnunuOnnx ใช้ ONNX แทน |
| ONNX Runtime เป็น .so อยู่แล้ว | ใช้ libonnxruntime.so shared ตัวเดียว |
| WORLD code ใช้ร่วมได้ 3 engine | worldline, vogen, classic ใช้ lib เดียวกัน |
| JNI interface เดียว | ทุก .so implement `renderer_native.h` เหมือนกัน |
| Performance | native ล้วน ไม่มี Python overhead |

## สถาปัตยกรรม

```
┌────────────────────────────────────────────────────────────┐
│  APK (Android App)                                          │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Kotlin (Engine Core + JNI bridge)                    │  │
│  │  ├─ Project Model + Commands + Undo/Redo             │  │
│  │  ├─ Phonemizer (shared plugin)                       │  │
│  │  ├─ Global RenderContext (curves, metadata, timing)  │  │
│  │  └─ Scheduler + Chunker + Mixer                      │  │
│  └──────────────────────┬───────────────────────────────┘  │
│                         │ JNI (unified interface)          │
│  ┌──────────────────────▼───────────────────────────────┐  │
│  │  Native Renderer Libraries (.so — C++ ทั้งหมด)        │  │
│  │                                                       │  │
│  │  ┌────────────────┐ ┌────────────────┐               │  │
│  │  │ libworldline.so│ │ libdiffsinger.so│              │  │
│  │  │ (C++ WORLD)    │ │ (C++ + ONNX)   │              │  │
│  │  └────────────────┘ └────────────────┘               │  │
│  │  ┌────────────────┐ ┌────────────────┐               │  │
│  │  │ libvogen.so    │ │ libenunu.so    │              │  │
│  │  │ (ONNX+WORLD)   │ │ (EnunuOnnx)    │              │  │
│  │  └────────────────┘ └────────────────┘               │  │
│  │  ┌────────────────┐ ┌────────────────┐               │  │
│  │  │ libclassic.so  │ │ libvoicevox.so │              │  │
│  │  │ (SharpWavtool) │ │ (voicevox_core)│              │  │
│  │  └────────────────┘ └────────────────┘               │  │
│  │                                                       │  │
│  │  shared: libonnxruntime.so (ใช้ร่วมกันทุกตัว)          │  │
│  └──────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────┘
```

## .so Components

| Library | Source | Dependencies |
|---|---|---|
| `libworldline.so` | ref C++ worldline/ + WORLD | WORLD, libpyin, libgvps, spline, xxhash |
| `libdiffsinger.so` | port DiffSingerRenderer.cs → C++ | onnxruntime |
| `libvogen.so` | port VogenRenderer.cs + VSVocoder → C++ | onnxruntime, WORLD |
| `libenunu.so` | port EnunuOnnxPhonemizer.cs → C++ | onnxruntime |
| `libclassic.so` | port SharpWavtool.cs → C++ | WORLD (resampler backend) |
| `libvoicevox.so` | voicevox_core (official C++) | onnxruntime |

## Unified JNI Interface

```cpp
// renderer_native.h — ทุก .so implement ตัวนี้
extern "C" {

void* renderer_create(const char* config_path);
void  renderer_destroy(void* handle);

typedef struct {
    const char*  singer_path;
    const float* pitches;       // pitch curve (cents)
    int          pitch_length;
    const float* dynamics;      // DYN curve
    const float* gender;        // GENC
    const float* breathiness;   // BREC
    const float* tension;       // TENC
    const float* voicing;       // VOIC
    const PhonemeInput* phonemes;
    int          phoneme_count;
    int          sample_rate;
    double       tempo;
} RenderContext;

typedef struct {
    const char* phoneme;        // "あ"
    double      position_ms;
    double      duration_ms;
    double      leading_ms;     // preutter
    // ... per-phoneme data
} PhonemeInput;

int renderer_render(void* handle, const RenderContext* ctx,
                    float** out_samples, int* out_length);
}
```

## Build

```
- Build system: CMake + Android NDK (แทน Bazel)
- ทุก .so ใน repo เดียว: native/ directory
- libonnxruntime.so: download จาก official (Maven AAR หรือ GitHub release)
- ภาษา: C++17 อย่างเดียว
```

## ผลกระทบ

### ทำได้ง่ายขึ้น
- JNI bridge เขียนครั้งเดียว (interface เดียว)
- Debug ทุก engine ด้วย toolchain เดียว
- Unit test native ได้ (gtest)

### ต้องยอมรับ
- Kotlin port ของ renderer logic = ยกเลิก (ไป C++ แทน)
- ต้องเรียนรู้ C++ (ถ้ายังไม่ถนัด) — แต่ source จาก ref เป็น C++ อยู่แล้ว
- CMake/NDK cross-compile ทุก .so

## Roadmap

```
Phase 1: libworldline.so (C++ ref เดิม — แค่ NDK build)
Phase 2: libdiffsinger.so (port tensor logic C# → C++)
Phase 3: libclassic.so + libvogen.so (reuse WORLD)
Phase 4: libenunu.so (port EnunuOnnx) + libvoicevox.so (voicevox_core)
```
