# Rusted UTAU / Android Voice Synth — Architecture

> Conceptual architecture: Global Scheduler + Chunker · อัปเดต 2026-09-12

เอกสารนี้อธิบาย component model เดิม; ตัวอย่าง `Kotlin` ด้านล่างเป็น pseudocode
เชิง interface ไม่ใช่ implementation ปัจจุบัน. ของจริงอยู่ใน Rust workspace
(`native/crates/*`, `native/tools/*`) และมี host สองทาง: Flutter → LAN synth-server,
กับ static HTML/Canvas → `engine-wasm` in-process.

สถานะ/รายละเอียดล่าสุด:

- [Worldline Rust Port](worldline-rust-port.md) — pure-Rust `r1-rs`
- [Known Limitations](known-limitations.md) — ทะเบียนข้อจำกัดที่รู้แล้วและตั้งใจไม่แก้ตอนนี้
- [WASM Target](wasm-target.md) — engine บน browser
- [Runtime Engine Plugins](runtime-engine-plugins.md) — native renderer matrix
- [Current Roadmap](../planning/roadmap/ROADMAP.md) — สิ่งที่เสร็จและงานค้าง
- [Engine-owned state policy and migration](engine-owned-state.md) — Rust authority และ Flutter migration gap

> **นโยบาย 2026-09-29:** Rust เป็น source of truth ของ project/musical state ทุก host; frontend เป็น UI-only สำหรับ render snapshot, ส่ง commands และจัดการ ephemeral input/layout. `project-session` เป็น data holder กลางที่ `engine-wasm` และ native adapter ใช้ร่วมกัน; WASM ยังต้องเปิด stable-ID session surface ให้ browser Driver. Flutter เป็น host รองและ adapter ใน working tree ยังต้องตรวจยืนยัน ดู policy/migration doc.

---

## System Overview

```
════════════════════════════════════════════════════
MAIN ENGINE
════════════════════════════════════════════════════

                Project / Timeline
                        │
                        ▼
               Render Planner
                        │
                        ▼
               Global RenderContext
                        │
        ┌───────────────┼────────────────────────────┐
        │               │                            │
        ▼               ▼                            ▼
   Curve Engine     Metadata                   Render Settings
   (Global)         (Global)                     (Global)


════════════════════════════════════════════════════
GLOBAL ORCHESTRATION
════════════════════════════════════════════════════

        ┌───────────────────────────────────────────┐
        │              Scheduler                     │
        │  ┌─────────────────────────────────────┐  │
        │  │ Job queue / progress / cancellation  │  │
        │  └─────────────────────────────────────┘  │
        └───────────────────────────────────────────┘
                        │
                        ▼
        ┌───────────────────────────────────────────┐
        │              Chunker                       │
        │  ┌─────────────────────────────────────┐  │
        │  │ Split phones → batches / dependencies│ │
        │  └─────────────────────────────────────┘  │
        └───────────────────────────────────────────┘
                        │
                        ▼

════════════════════════════════════════════════════
RENDERER PLUGIN
════════════════════════════════════════════════════

 World Renderer Backend         Neural Renderer Plugin
 ┌────────────────────────┐     ┌────────────────────────┐
 │ Voicebank Loader       │     │ Model Loader           │
 │ Builder → SynthRequest │     │ Builder → Tensor Input │
 │ .so or worldline-rs    │     │ ONNX Runtime           │
 └────────────────────────┘     └────────────────────────┘

════════════════════════════════════════════════════
SHARED PLUGINS / OUTPUT
════════════════════════════════════════════════════

Phonemizer → PCM Chunks → Mixer → Playback / Export
```

---

## Components

### 1. Main Engine

| Component | Responsibility |
|---|---|
| **Project / Timeline** | Project state, notes, tracks, parts |
| **Render Planner** | Orchestrate render job, determine what to render |
| **Global RenderContext** | Prepared data shared across all renderers |
| **Curve Engine** | Compute expression curves (DYN, PITD, GENC, BREC, etc.) |
| **Metadata** | Project metadata (tempo, time signature, key) |
| **Render Settings** | Which renderer to use, global settings |

### 2. Global Orchestration

| Component | Responsibility |
|---|---|
| **Scheduler** | Job queue, progress tracking, cancellation, retry |
| **Chunker** | Split phones into batches for parallel render |

### 3. Renderer Plugin

Each plugin implements a load/build/synthesize lifecycle. Native and browser adapters select supported implementations; project semantics stay in Rust.

### 4. Shared Plugins

The shared phonemizer converts lyrics to phonemes and supports multiple language/dictionary implementations.

### 5. Output

**PCM Chunks → Mixer → Playback / Export**

---

## Data Flow

```
Rust Project / Timeline
    │
    ▼
Rust Render Planner → Phonemizer → Curve/Metadata preparation
    │
    ▼
Scheduler → Chunker → Renderer Plugin
    │
    ▼
PCM Chunks → Mixer → Playback / Export
```

The frontends translate user input into commands and display snapshots/results; they do not own a competing project document or musical rules.

---

## Scheduler

```kotlin
interface Scheduler {
    fun submitJob(context: RenderContext, onProgress: (Float) -> Unit,
                  onComplete: (List<PCMChunk>) -> Unit,
                  onError: (Exception) -> Unit): JobHandle
    fun cancelJob(handle: JobHandle)
    fun getProgress(handle: JobHandle): Float
}

data class JobHandle(val id: String)
```

## Chunker

```kotlin
interface Chunker {
    fun split(phones: List<PhoneInput>): List<RenderBatch>
}

data class RenderBatch(val id: String, val phones: List<PhoneInput>,
                       val dependency: String? = null)
```

## RenderContext (Global)

```kotlin
data class RenderContext(
    val projectId: String, val projectName: String,
    val tempos: List<Tempo>, val timeSignatures: List<TimeSignature>,
    val trackId: String, val partId: String, val singerId: String,
    val notes: List<Note>, val phonemes: List<Phoneme>,
    val curves: Map<String, CurveData>, val rendererId: String,
    val rendererSettings: Map<String, Any>, val startTick: Int, val endTick: Int
)
```

## Related implementation docs

- [Engine](engine.md) — current Rust domain and engine implementation
- [Runtime](runtime.md) — scheduler, chunking and mixer
- [WASM Target](wasm-target.md) — browser engine
- [Engine-owned state policy and migration](engine-owned-state.md)
- [Roadmap](../planning/roadmap/ROADMAP.md)
