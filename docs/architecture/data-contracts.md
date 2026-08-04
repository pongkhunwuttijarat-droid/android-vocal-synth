# Data Contracts

> Data structures shared between components

---

## Domain Data (Engine)

### Project

```kotlin
data class Project(
    val id: String,
    val name: String,
    val tempos: List<Tempo>,
    val timeSignatures: List<TimeSignature>,
    val tracks: List<Track>,
    val parts: List<Part>,
    val expressions: Map<String, ExpressionDescriptor>
)
```

### Timing

```kotlin
data class Tempo(
    val position: Int,      // ticks
    val bpm: Double
)

data class TimeSignature(
    val barPosition: Int,
    val beatPerBar: Int,    // 4
    val beatUnit: Int       // 4
)
```

### Track

```kotlin
data class Track(
    val id: String,
    val name: String,
    val singerId: String,
    val phonemizerId: String,
    val rendererId: String,     // "world", "diffsinger", "classic"
    val volume: Double,         // dB
    val pan: Double,            // -100..100
    val muted: Boolean,
    val solo: Boolean
)
```

### Note

```kotlin
data class Note(
    val id: String,
    val position: Int,          // ticks (relative to part)
    val duration: Int,          // ticks
    val tone: Int,              // MIDI note (C4 = 60)
    val tuning: Int,            // cents
    val lyric: String,          // "あ" or "read[r iy d]"
    val pitch: Pitch,
    val vibrato: Vibrato,
    val phonemeExpressions: List<PhonemeExpression>
)

data class Pitch(
    val points: List<PitchPoint>
)

data class PitchPoint(
    val xMs: Double,            // ms offset from note start
    val yCent: Double,          // cent value
    val shape: String           // "io", "li", "si", "sp"
)

data class Vibrato(
    val length: Float,          // 0-100 (% of note duration)
    val period: Float,          // ms
    val depth: Float,           // 0-200 (cent)
    val fadeIn: Float,          // 0-100 (%)
    val fadeOut: Float,         // 0-100 (%)
    val phase: Float,           // 0-100 (%)
    val drift: Float,           // 0-100 (ms)
    val volLink: Float          // 0-100 (%)
)

data class PhonemeExpression(
    val abbr: String,           // "dyn", "vel", "vol", ...
    val value: Float,
    val index: Int              // phoneme index in note
)
```

### Voicebank

```kotlin
data class Singer(
    val id: String,
    val name: String,
    val basePath: String,
    val singerType: SingerType,
    val otoSets: List<OtoSet>,
    val subbanks: List<Subbank>
)

enum class SingerType { Classic, DiffSinger, Enunu, Vogen, Voicevox }

data class OtoSet(
    val name: String,
    val filePath: String,
    val entries: List<Oto>
)

data class Oto(
    val alias: String,          // "あ" or "あC5"
    val wav: String,            // "あ.wav"
    val offset: Double,         // ms
    val consonant: Double,      // ms
    val cutoff: Double,         // ms
    val preutter: Double,       // ms
    val overlap: Double,        // ms
    val frq: OtoFrq?
)

data class OtoFrq(
    val loaded: Boolean,
    val hopSize: Int,
    val averageF0: Double,
    val toneDiffFix: DoubleArray,
    val toneDiffStretch: DoubleArray
)

data class Subbank(
    val color: String,          // "power", "whisper"
    val prefix: String,
    val suffix: String,
    val toneRanges: List<String>
)
```

---

## Engine → Renderer Data

### PhonemeOutput (from phonemizer)

```kotlin
data class PhonemeOutput(
    val phoneme: String,        // "あ"
    val position: Int,          // ticks (absolute)
    val duration: Int,          // ticks
    val tone: Int,              // MIDI note
    val parentNoteId: String,
    val index: Int
)
```

### PhoneRenderInput (from feed → renderer)

```kotlin
data class PhoneRenderInput(
    // Identity
    val phoneme: String,            // mapped alias
    val singerId: String,
    
    // Source
    val sourceWavPath: String,      // oto.File
    
    // OTO parameters
    val oto: OtoParams,
    
    // Note parameters
    val tone: Int,                  // MIDI note
    val tempo: Double,              // BPM
    
    // Timing (ms)
    val positionMs: Double,
    val durationMs: Double,
    val leadingMs: Double,          // preutter
    val preutterMs: Double,
    val overlapMs: Double,
    val durCorrectionMs: Double,
    
    // Expressions
    val volume: Float,              // 0-200
    val velocity: Float,            // 0-200
    val modulation: Float,          // 0-100
    val direct: Boolean,
    
    // Flags
    val flags: List<Flag>,
    
    // Subbank
    val suffix: String,
    
    // Envelope
    val envelope: List<Vector2>,    // 5 points
    
    // Pitch
    val pitches: IntArray,          // cents
    
    // Cache key
    val hash: Long
)

data class OtoParams(
    val offset: Double,
    val consonant: Double,
    val cutoff: Double,
    val preutter: Double,
    val overlap: Double
)

data class Flag(
    val abbr: String,
    val value: Int?
)

data class Vector2(
    val x: Float,
    val y: Float
)
```

---

## Renderer → Runtime Data

### AudioChunk

```kotlin
data class AudioChunk(
    val samples: FloatArray,    // mono, 44100Hz
    val leadingMs: Double,
    val positionMs: Double,
    val hash: Long
)
```

---

## Runtime → AudioService Data

### FinalAudio

```kotlin
data class FinalAudio(
    val samples: FloatArray,    // mono or stereo
    val sampleRate: Int,        // 44100
    val durationMs: Double
)
```

---

## Commands (UI → Engine)

```kotlin
interface Command {
    fun execute()
    fun unexecute()
}

// Note commands
data class AddNoteCommand(val partId: String, val note: Note) : Command
data class RemoveNoteCommand(val partId: String, val noteId: String) : Command
data class MoveNoteCommand(val partId: String, val noteId: String,
                           val deltaPos: Int, val deltaTone: Int) : Command

// Part commands
data class AddPartCommand(val part: Part) : Command
data class RemovePartCommand(val partId: String) : Command

// Track commands
data class AddTrackCommand(val track: Track) : Command
data class RemoveTrackCommand(val trackId: String) : Command

// Expression commands
data class SetNoteExpressionCommand(val partId: String, val noteId: String,
                                     val abbr: String, val values: List<Float?>) : Command
```

---

## Events (cross-layer)

```kotlin
sealed class ProjectEvent {
    data class ProjectLoaded(val project: Project) : ProjectEvent()
    data class ProjectChanged(val affectedPartIds: List<String>) : ProjectEvent()
    data class Error(val message: String, val exception: Exception?) : ProjectEvent()
}

sealed class RenderEvent {
    data class JobStarted(val jobId: String) : RenderEvent()
    data class Progress(val jobId: String, val progress: Float) : RenderEvent()
    data class JobCompleted(val jobId: String, val audio: FinalAudio) : RenderEvent()
    data class JobFailed(val jobId: String, val error: String) : RenderEvent()
    data class JobCancelled(val jobId: String) : RenderEvent()
}

sealed class PlaybackEvent {
    data class PositionChanged(val tick: Int) : PlaybackEvent()
    data class PlaybackStarted(val tick: Int) : PlaybackEvent()
    data class PlaybackStopped() : PlaybackEvent()
}
```
