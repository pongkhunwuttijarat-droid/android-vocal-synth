# Renderer Architecture

> Pluggable synthesis backends — owns voicebank + synthesis

---

## Interface

```kotlin
interface Renderer {
    val id: String
    val name: String
    
    fun initialize(config: RendererConfig)
    fun cleanup()
    
    fun render(
        phonemes: List<PhonemeOutput>,
        project: Project,
        parts: List<Part>,
        range: TickRange
    ): List<AudioChunk>
    
    fun supportsExpression(abbr: String): Boolean
}
```

---

## Implementations

### 1. WORLD Renderer (v1 — from OpenUtau)

```
Voicebank: oto.ini + wav + prefix.map
Synthesis: WORLD vocoder (C++ via JNI)
Output: AudioChunk (mono, 44100Hz)
```

**Data Flow:**
```
PhoneRenderInput[]
    │
    ▼
SynthRequest (per-phone)
    │
    ▼
PhraseSynth.AddRequest()
    │
    ▼
PhraseSynth.SetCurves(f0, gender, tension, breathiness, voicing)
    │
    ▼
PhraseSynth.Synth() → float[]
    │
    ▼
AudioChunk { samples, leadingMs, positionMs, hash }
```

**SynthRequest Fields:**
```cpp
struct SynthRequest {
    // Audio
    int32_t sample_fs;          // 44100
    int32_t sample_length;      // samples count
    double* sample;             // audio data
    
    // FRQ (optional)
    int32_t frq_length;
    char* frq;
    
    // Note
    int32_t tone;               // MIDI note
    double con_vel;             // velocity (0-200)
    
    // Timing (ms)
    double offset;              // OTO offset
    double required_length;     // duration
    double consonant;           // OTO consonant
    double cut_off;             // OTO cutoff
    
    // Expression
    double volume;              // 0-200
    double modulation;          // 0-100
    double tempo;               // BPM
    
    // Pitch
    int32_t pitch_bend_length;
    int32_t* pitch_bend;        // cents array
    
    // Flags
    int flag_g;                 // gender (-100..100)
    int flag_P;                 // peak compression (0..100)
    int flag_Mt;                // tension (-100..100)
    int flag_Mb;                // breathiness (-100..100)
    int flag_Mv;                // voicing (0..100)
};
```

### 2. DiffSinger Renderer (future)

```
Voicebank: dsconfig.yaml + ONNX model
Synthesis: ONNX inference
Output: AudioChunk
```

### 3. Classic Renderer (future)

```
Voicebank: oto.ini + wav
Synthesis: External resampler + wavtool
Output: AudioChunk
```

---

## WORLD Renderer Internal

```
┌──────────────────────────────────────────────────────────────┐
│  WORLD Renderer                                              │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  Voicebank Loader                                      │   │
│  │  ├─ parse oto.ini → Oto[]                             │   │
│  │  ├─ parse character.txt → metadata                    │   │
│  │  └─ parse prefix.map → tone mapping                   │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  Feed (internal)                                       │   │
│  │  ├─ phoneme + tone → oto lookup                       │   │
│  │  ├─ compute pitches (vibrato + PITD + mod+)           │   │
│  │  ├─ compute flags (g, B, H, P, ...)                   │   │
│  │  └─ build SynthRequest                                │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  WORLD Synthesis (JNI)                                 │   │
│  │  ├─ libworldline.so                                   │   │
│  │  ├─ PhraseSynth: AddRequest + SetCurves + Synth       │   │
│  │  └─ Output: float[] (mono, 44100Hz)                   │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  Output: AudioChunk[]                                        │
└──────────────────────────────────────────────────────────────┘
```

---

## JNI Bridge (Kotlin → C++)

```kotlin
class WorldlineNative {
    companion object {
        init {
            System.loadLibrary("worldline")
        }
    }
    
    external fun create(): Long
    external fun destroy(handle: Long)
    
    external fun addRequest(
        handle: Long,
        samples: FloatArray,
        sampleFs: Int,
        tone: Int,
        conVel: Double,
        offset: Double,
        requiredLength: Double,
        consonant: Double,
        cutOff: Double,
        volume: Double,
        modulation: Double,
        tempo: Double,
        pitchBend: IntArray,
        posMs: Double,
        skipMs: Double,
        lengthMs: Double,
        fadeInMs: Double,
        fadeOutMs: Double
    )
    
    external fun setCurves(
        handle: Long,
        f0: DoubleArray,
        gender: DoubleArray,
        tension: DoubleArray,
        breathiness: DoubleArray,
        voicing: DoubleArray
    )
    
    external fun synth(handle: Long): FloatArray
}
```

---

## Output

```kotlin
data class AudioChunk(
    val samples: FloatArray,    // mono, 44100Hz
    val leadingMs: Double,      // preutter
    val positionMs: Double,     // position in timeline
    val hash: Long              // cache key
)
```
