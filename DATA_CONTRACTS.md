# Data Contracts — Engine v3

> Draft: สัญญาระหว่าง layer — define โครงสร้าง data ที่ไหลระหว่าง ProjectController ↔ Runtime ↔ Engine

---

## 1. Domain Data (ProjectController ↔ Runtime)

### 1.1 Project

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

### 1.2 Timing

```kotlin
data class Tempo(
    val position: Int,      // ticks
    val bpm: Double
)

data class TimeSignature(
    val barPosition: Int,   // ticks
    val beatPerBar: Int,    // 4
    val beatUnit: Int       // 4
)
```

### 1.3 Track

```kotlin
data class Track(
    val id: String,
    val name: String,
    val singerId: String,
    val phonemizerId: String,
    val rendererId: String,     // "classic" | "worldline" | "diffsinger"
    val resamplerId: String,    // resampler plugin name
    val wavtoolId: String,      // wavtool plugin name
    val voiceColor: String,     // voice color name
    val volume: Double,         // dB
    val pan: Double,            // -100..100
    val muted: Boolean,
    val solo: Boolean,
    val mixFx: MixFx?
)
```

### 1.4 Part

```kotlin
data class VoicePart(
    val id: String,
    val trackId: String,
    val position: Int,          // ticks
    val duration: Int,          // ticks
    val name: String,
    val notes: List<Note>,
    val curves: List<Curve>
)

data class WavePart(
    val id: String,
    val trackId: String,
    val position: Int,
    val duration: Int,
    val filePath: String,
    val skip: Int,
    val trim: Int,
    val fadeIn: Int,
    val fadeOut: Int
)

// Union type
sealed class Part {
    abstract val id: String
    abstract val trackId: String
    abstract val position: Int
    abstract val duration: Int
}
```

### 1.5 Note

```kotlin
data class Note(
    val id: String,
    val position: Int,          // ticks (relative to part)
    val duration: Int,          // ticks
    val tone: Int,              // MIDI note number (C4 = 60)
    val tuning: Int,            // cents (±100 = ±1 semitone)
    val lyric: String,          // "あ" or "read[r iy d]"
    val pitch: Pitch,
    val vibrato: Vibrato,
    val phonemizerOverride: String?,    // override per-note
    val phonemeExpressions: List<PhonemeExpression>,
    val phonemeOverrides: List<PhonemeOverride>
)

data class Pitch(
    val points: List<PitchPoint>
)

data class PitchPoint(
    val xMs: Double,            // ms offset from note start
    val yCent: Double,          // cent value
    val shape: PitchPointShape  // "io", "li", "si", "sp", "hsin", "csin"
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
    val abbr: String,           // "dyn", "vel", "vol", "clr", "gen", "bre", ...
    val value: Float,
    val index: Int              // phoneme index in note
)

data class PhonemeOverride(
    val index: Int,
    val override: String        // override phoneme alias
)
```

### 1.6 Curve

```kotlin
data class Curve(
    val abbr: String,           // "dyn", "pitd", "genc", ...
    val points: List<CurvePoint>
)

data class CurvePoint(
    val tick: Int,
    val value: Float
)
```

### 1.7 Expression

```kotlin
data class ExpressionDescriptor(
    val name: String,
    val abbr: String,
    val type: ExpressionType,       // Numerical | Options | Curve
    val min: Float,
    val max: Float,
    val defaultValue: Float,
    val isFlag: Boolean,
    val flag: String,               // "g", "B", "H", "P", ...
    val options: List<String>,      // for Options type
    val skipOutputIfDefault: Boolean
)

enum class ExpressionType { Numerical, Options, Curve }
```

### 1.8 Voicebank

```kotlin
data class Singer(
    val id: String,
    val name: String,
    val basePath: String,
    val singerType: SingerType,     // Classic, DiffSinger, Enunu, Vogen, Voicevox
    val image: String?,
    val author: String?,
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
    val alias: String,              // "あ" or "あC5"
    val phonetic: String,           // phonetic part (after tone suffix)
    val wav: String,                // "あ.wav"
    val offset: Double,             // ms
    val consonant: Double,          // ms
    val cutoff: Double,             // ms (negative = from end)
    val preutter: Double,           // ms
    val overlap: Double,            // ms
    val frq: OtoFrq?
)

data class Subbank(
    val color: String,              // "power", "whisper", ...
    val prefix: String,
    val suffix: String,
    val toneRanges: List<String>    // "C1-C4", "C4", "C4-C6"
)

data class OtoFrq(
    val loaded: Boolean,
    val hopSize: Int,
    val averageF0: Double,
    val toneDiffFix: DoubleArray,
    val toneDiffStretch: DoubleArray
)
```

---

## 2. Runtime Data (Runtime ↔ Engine)

### 2.1 ผลลัพธ์จาก Phonemizer → ส่งให้ Chunker

```kotlin
data class PhonemeOutput(
    val phoneme: String,            // "あ" (mapped alias)
    val position: Int,              // ticks (absolute, ไม่ relative note)
    val duration: Int,              // ticks
    val tone: Int,                  // MIDI note number
    val parentNoteId: String,
    val index: Int                  // phoneme index in note
)
```

### 2.2 ผลลัพธ์จาก Chunker → ส่งให้ Engine

```kotlin
data class PhoneRenderInput(
    // Identity
    val phoneme: String,            // alias ที่ map แล้ว
    val singerId: String,
    
    // Source
    val sourceWavPath: String,      // oto.File (absolute path)
    
    // OTO parameters
    val oto: OtoParams,
    
    // Note parameters
    val tone: Int,                  // MIDI note number
    val tempo: Double,              // adjusted BPM
    
    // Timing (ms)
    val positionMs: Double,         // absolute position ใน timeline
    val durationMs: Double,
    val leadingMs: Double,          // preutter
    val preutterMs: Double,
    val overlapMs: Double,
    val durCorrectionMs: Double,
    
    // Expressions
    val volume: Float,              // 0-200 (normalized)
    val velocity: Float,            // 0-200
    val modulation: Float,          // 0-100
    val direct: Boolean,            // bypass resampler
    
    // Flags
    val flags: List<Flag>,
    
    // Subbank
    val suffix: String,             // voice color suffix
    
    // Envelope
    val envelope: List<Vector2>,    // 5 points (ms, gain 0-100)
    
    // Pitch
    val pitches: IntArray,          // cents deviation (every 5 ticks)
    
    // Cache key (computed)
    val hash: Long                  // xxhash64
)

data class OtoParams(
    val offset: Double,             // ms
    val consonant: Double,          // ms
    val cutoff: Double,             // ms
    val preutter: Double,           // ms
    val overlap: Double             // ms
)

data class Flag(
    val abbr: String,               // "g", "B", "H", "P"
    val value: Int?                 // null = flag only
)

data class Vector2(
    val x: Float,
    val y: Float
)
```

### 2.3 Engine output → Runtime

```kotlin
data class AudioChunk(
    val samples: FloatArray,        // mono, 44100Hz
    val leadingMs: Double,
    val positionMs: Double,
    val hash: Long                  // cache key
)
```

### 2.4 Runtime → AudioService

```kotlin
data class FinalAudio(
    val samples: FloatArray,        // mono or stereo
    val sampleRate: Int,            // 44100
    val durationMs: Double
)
```

### 2.5 Chunk definition

```kotlin
data class RenderChunk(
    val id: String,
    val phones: List<PhoneRenderInput>,
    val dependency: String?         // chunk ID that must complete first
)
```

### 2.6 Job

```kotlin
data class RenderJob(
    val id: String,
    val parts: List<RenderPart>,
    val startTick: Int,
    val endTick: Int,
    val status: JobStatus
)

data class RenderPart(
    val partId: String,
    val trackId: String,
    val singerId: String,
    val phonemizerId: String
)

enum class JobStatus {
    Queued, Preparing, Rendering, Mixing, Done, Cancelled, Failed
}
```

---

## 3. Commands (UI → ProjectController)

### 3.1 Command base

```kotlin
interface Command {
    fun execute()
    fun unexecute()
    val validateOptions: ValidateOptions
}

data class ValidateOptions(
    val skipTiming: Boolean = false,
    val partId: String? = null,
    val skipPhonemizer: Boolean = false,
    val skipPhoneme: Boolean = false
)
```

### 3.2 Note commands

```kotlin
data class AddNoteCommand(
    val partId: String,
    val note: Note
) : Command

data class RemoveNoteCommand(
    val partId: String,
    val noteId: String
) : Command

data class MoveNoteCommand(
    val partId: String,
    val noteId: String,
    val deltaPos: Int,          // ticks
    val deltaTone: Int          // semitones
) : Command

data class ResizeNoteCommand(
    val partId: String,
    val noteId: String,
    val deltaDur: Int           // ticks
) : Command

data class ChangeNoteLyricCommand(
    val partId: String,
    val noteId: String,
    val newLyric: String
) : Command

data class ChangeNoteToneCommand(
    val partId: String,
    val noteId: String,
    val newTone: Int
) : Command
```

### 3.3 Part commands

```kotlin
data class AddPartCommand(
    val part: Part
) : Command

data class RemovePartCommand(
    val partId: String
) : Command

data class MovePartCommand(
    val partId: String,
    val newPos: Int,
    val newTrackId: String
) : Command

data class ResizePartCommand(
    val partId: String,
    val deltaDur: Int,
    val fromStart: Boolean
) : Command
```

### 3.4 Track commands

```kotlin
data class AddTrackCommand(
    val track: Track
) : Command

data class RemoveTrackCommand(
    val trackId: String
) : Command

data class RenameTrackCommand(
    val trackId: String,
    val newName: String
) : Command

data class ChangeSingerCommand(
    val trackId: String,
    val newSingerId: String
) : Command

data class ChangePhonemizerCommand(
    val trackId: String,
    val newPhonemizerId: String
) : Command

data class ChangeRendererCommand(
    val trackId: String,
    val newRendererId: String,
    val newResamplerId: String?,
    val newWavtoolId: String?
) : Command
```

### 3.5 Expression commands

```kotlin
data class SetNoteExpressionCommand(
    val partId: String,
    val noteId: String,
    val abbr: String,
    val values: List<Float?>     // per-phoneme index
) : Command

data class SetPhonemeExpressionCommand(
    val partId: String,
    val noteId: String,
    val phonemeIndex: Int,
    val abbr: String,
    val value: Float?
) : Command

data class SetCurveCommand(
    val partId: String,
    val abbr: String,
    val points: List<CurvePoint>
) : Command
```

### 3.6 Project commands

```kotlin
data class SetBpmCommand(
    val newBpm: Double
) : Command

data class AddTempoChangeCommand(
    val tick: Int,
    val bpm: Double
) : Command

data class AddTimeSigCommand(
    val bar: Int,
    val beatPerBar: Int,
    val beatUnit: Int
) : Command
```

### 3.7 Playback commands (notifications)

```kotlin
data class SeekCommand(
    val tick: Int,
    val pause: Boolean
) : Command

data class SetVolumeCommand(
    val trackId: String,
    val volume: Double          // dB
) : Command

data class SetPanCommand(
    val trackId: String,
    val pan: Double
) : Command
```

---

## 4. Events (cross-layer notifications)

```kotlin
sealed class ProjectEvent {
    data class ProjectLoaded(val project: Project) : ProjectEvent()
    data class ProjectChanged(val affectedPartIds: List<String>) : ProjectEvent()
    data class ValidateNeeded(val partId: String?) : ProjectEvent()
    data class Error(val message: String, val exception: Exception?) : ProjectEvent()
}

sealed class RenderEvent {
    data class JobStarted(val jobId: String) : RenderEvent()
    data class ChunkCompleted(val jobId: String, val chunkId: String, val progress: Float) : RenderEvent()
    data class JobCompleted(val jobId: String, val audio: FinalAudio) : RenderEvent()
    data class JobFailed(val jobId: String, val error: String) : RenderEvent()
    data class JobCancelled(val jobId: String) : RenderEvent()
}

sealed class PlaybackEvent {
    data class PositionChanged(val tick: Int) : PlaybackEvent()
    data class PlaybackStarted(val tick: Int) : PlaybackEvent()
    data class PlaybackStopped() : PlaybackEvent()
    data class VolumeChanged(val trackId: String, val volume: Double) : PlaybackEvent()
}
```

---

## 5. Layer Summary

```
UI Layer
    │ Command (NoteCommand, PartCommand, ...)
    │ ProjectEvent, RenderEvent, PlaybackEvent
    ▼
ProjectController
    │ Project (full state)
    │ RenderJob (request)
    ▼
Runtime
    │ RenderChunk[] (phones grouped)
    ▼
Engine
    │ AudioChunk[] (rendered audio)
    ▼
Runtime
    │ FinalAudio (mixed)
    ▼
AudioService
    │ output
    ▼
speaker
```
