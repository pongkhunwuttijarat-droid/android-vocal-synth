# Android Voice Synth — Architecture v2

> Revised architecture — Global Scheduler + Chunker

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
        │  │ Job queue                            │  │
        │  │ Progress tracking                    │  │
        │  │ Cancellation                         │  │
        │  │ Retry logic                          │  │
        │  └─────────────────────────────────────┘  │
        └───────────────────────────────────────────┘
                        │
                        ▼
        ┌───────────────────────────────────────────┐
        │              Chunker                       │
        │  ┌─────────────────────────────────────┐  │
        │  │ Split phones → batches               │  │
        │  │ Group by N-phones                    │  │
        │  │ Handle dependencies                  │  │
        │  └─────────────────────────────────────┘  │
        └───────────────────────────────────────────┘
                        │
                        ▼


════════════════════════════════════════════════════
RENDERER PLUGIN
════════════════════════════════════════════════════

 World Renderer Plugin
 ┌────────────────────────────────────┐
 │ Voicebank Loader                   │
 │ Builder (→ SynthRequest)           │
 │ PhraseSynth / libworldline.so      │
 └────────────────────────────────────┘

 Neural Renderer Plugin
 ┌────────────────────────────────────┐
 │ Model Loader                       │
 │ Builder (→ Tensor Input)           │
 │ ONNX Runtime                       │
 └────────────────────────────────────┘

 Custom Renderer Plugin
 ┌────────────────────────────────────┐
 │ ...                                │
 └────────────────────────────────────┘


════════════════════════════════════════════════════
SHARED PLUGINS
════════════════════════════════════════════════════

Phonemizer
 ├── Japanese
 ├── English
 ├── Thai
 └── ...


════════════════════════════════════════════════════
OUTPUT
════════════════════════════════════════════════════

PCM Chunks
      │
      ▼
Mixer
      │
Playback / Export
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

Each plugin implements:

```kotlin
interface RendererPlugin {
    val id: String
    val name: String
    
    fun load(config: RendererConfig)
    fun unload()
    
    fun build(context: RenderContext): List<SynthRequest>
    fun synthesize(requests: List<SynthRequest>): List<PCMChunk>
}
```

### 4. Shared Plugins

**Phonemizer:**
- Shared across all renderers
- Convert lyric → phoneme
- Multiple implementations (Japanese, English, Thai, etc.)

### 5. Output

**PCM Chunks → Mixer → Playback / Export**

---

## Data Flow

```
Project / Timeline
    │
    ▼
Render Planner
    │
    ├─ Determine what to render (parts, range)
    ├─ Run Phonemizer (shared plugin)
    │   └─ lyric → phoneme
    │
    ▼
Global RenderContext
    │
    ├─ Curve Engine: compute expressions
    ├─ Metadata: tempo, time sig, key
    └─ Render Settings: which renderer
    │
    ▼
Scheduler
    │
    ├─ Queue job
    ├─ Track progress
    │
    ▼
Chunker
    │
    ├─ Split phones → batches
    │
    ▼
Renderer Plugin (per batch)
    │
    ├─ Voicebank/Model Loader
    ├─ Builder: context → synth request
    └─ Synthesis: request → PCM chunks
    │
    ▼
PCM Chunks
    │
    ▼
Mixer
    │
    ▼
Playback / Export
```

---

## Scheduler

```kotlin
interface Scheduler {
    fun submitJob(
        context: RenderContext,
        onProgress: (Float) -> Unit,
        onComplete: (List<PCMChunk>) -> Unit,
        onError: (Exception) -> Unit
    ): JobHandle
    
    fun cancelJob(handle: JobHandle)
    fun getProgress(handle: JobHandle): Float
}

data class JobHandle(
    val id: String
)
```

---

## Chunker

```kotlin
interface Chunker {
    fun split(phones: List<PhoneInput>): List<RenderBatch>
}

data class RenderBatch(
    val id: String,
    val phones: List<PhoneInput>,
    val dependency: String? = null
)

// Simple implementation: split by N-phones per batch
class SimpleChunker(private val batchSize: Int = 8) : Chunker {
    override fun split(phones: List<PhoneInput>): List<RenderBatch> {
        return phones.chunked(batchSize).mapIndexed { index, batch ->
            RenderBatch(
                id = "batch-$index",
                phones = batch
            )
        }
    }
}
```

---

## RenderContext (Global)

```kotlin
data class RenderContext(
    // Project metadata
    val projectId: String,
    val projectName: String,
    val tempos: List<Tempo>,
    val timeSignatures: List<TimeSignature>,
    
    // Track/Part info
    val trackId: String,
    val partId: String,
    val singerId: String,
    
    // Notes (phonemized)
    val notes: List<Note>,
    val phonemes: List<Phoneme>,
    
    // Curves (computed)
    val curves: Map<String, CurveData>,
    
    // Render settings
    val rendererId: String,
    val rendererSettings: Map<String, Any>,
    
    // Range
    val startTick: Int,
    val endTick: Int
)
```

---

## Key Differences from v1

| Aspect | v1 | v2 |
|---|---|---|
| **Curve Engine** | Per-renderer | Global (in Engine) |
| **Metadata** | Per-renderer | Global (in Engine) |
| **Render Settings** | Per-renderer | Global (in Engine) |
| **Scheduler** | In Runtime | Global Orchestration |
| **Chunker** | In Runtime | Global Orchestration |
| **Phonemizer** | In Engine | Shared Plugin |
| **Renderer Input** | PhoneRenderInput | RenderContext → SynthRequest |
| **Renderer Output** | AudioChunk | PCMChunk |

---

## Benefits of v2

1. **Global Context** — Curves, metadata, settings computed once
2. **Global Orchestration** — Scheduler + Chunker manage complexity
3. **Plugin Isolation** — Each renderer owns its own loader + builder
4. **Shared Phonemizer** — Reusable across renderers
5. **Cleaner Interface** — Renderer just needs batches from Chunker
6. **Extensible** — Easy to add new renderer plugins
