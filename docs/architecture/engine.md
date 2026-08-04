# Engine Architecture

> Core domain logic — no IO, no threads, no audio

---

## Modules

```
Engine
├── Project Module
│   ├─ UProject (state)
│   ├─ Commands (execute/unexecute)
│   ├─ Undo/Redo stack
│   └─ Validation
│
├── Phonemizer Module
│   ├─ Phonemizer interface
│   ├─ ClassicPhonemizer (VCV/CV rule-based)
│   ├─ DiffSingerPhonemizer (G2p-based)
│   └─ G2p framework
│
└── Feed Module
    ├─ prepareRenderInputs()
    ├─ Pitch computation (vibrato + PITD + mod+)
    ├─ Curve sampling (DYN, GENC, BREC, ...)
    ├─ Flag computation (g, B, H, P, ...)
    └─ Hash computation
```

---

## Project Module

### UProject

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

### Commands

```kotlin
interface Command {
    fun execute()
    fun unexecute()
    val validateOptions: ValidateOptions
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

## Phonemizer Module

### Interface

```kotlin
interface Phonemizer {
    fun process(
        notes: List<NoteInput>,
        singer: Singer,
        prev: NoteInput?,
        next: NoteInput?
    ): List<PhonemeOutput>
    
    fun setSinger(singer: Singer)
}
```

### Implementations

| Phonemizer | Use Case |
|---|---|
| ClassicPhonemizer | VCV/CV rule-based (Japanese, Korean, etc.) |
| DiffSingerPhonemizer | G2p-based (any language) |

---

## Feed Module

### prepareRenderInputs()

```kotlin
fun prepareRenderInputs(
    phonemes: List<PhonemeOutput>,
    project: Project,
    trackId: String,
    partId: String
): List<PhoneRenderInput>
```

### Computation Steps

1. **Pitch Computation**
   - Base pitch from note.tone
   - Apply vibrato
   - Apply PITD curve
   - Apply mod+ (FRQ-based)

2. **Curve Sampling**
   - DYN → dynamics (linear gain)
   - GENC → gender
   - BREC → breathiness
   - TENC → tension
   - VOIC → voicing

3. **Flag Computation**
   - g → gender flag
   - B → breath flag
   - H → lowpass flag
   - P → normalize flag

4. **Hash Computation**
   - XXH64 of all render parameters
   - Used for cache key

---

## Data Contracts

### Input (from UI)

```kotlin
// Note
data class Note(
    val id: String,
    val position: Int,      // ticks
    val duration: Int,      // ticks
    val tone: Int,          // MIDI note (C4 = 60)
    val tuning: Int,        // cents
    val lyric: String,      // "あ" or "read[r iy d]"
    val pitch: Pitch,
    val vibrato: Vibrato,
    val phonemeExpressions: List<PhonemeExpression>
)
```

### Output (to Renderer)

```kotlin
data class PhoneRenderInput(
    val phoneme: String,            // mapped alias
    val sourceWavPath: String,      // oto.File
    val oto: OtoParams,
    val tone: Int,
    val tempo: Double,
    val positionMs: Double,
    val durationMs: Double,
    val leadingMs: Double,
    val preutterMs: Double,
    val overlapMs: Double,
    val volume: Float,
    val velocity: Float,
    val modulation: Float,
    val flags: List<Flag>,
    val pitches: IntArray,          // cents
    val hash: Long
)
```
