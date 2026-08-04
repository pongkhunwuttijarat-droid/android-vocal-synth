# Engine Package Diagram (อ้างอิง OpenUtau)

> ออกแบบสำหรับ Android Voice Synth — focus ที่ core engine ไม่รวม UI

---

## 📦 Package Hierarchy

```
┌─────────────────────────────────────────────────────────────────────┐
│                        APPLICATION LAYER                             │
│  (Android Activity, Service, UI ไม่รวมใน diagram นี้)                │
└──────────────────────┬──────────────────────────────────────────────┘
                       │ depends on
┌──────────────────────▼──────────────────────────────────────────────┐
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                   playback                                   │   │
│  │  PlaybackManager, ToneGenerator, SineGenerator                │   │
│  │  AudioOutput interface + implementations                     │   │
│  │  (Android AudioTrack, AAudio, etc.)                          │   │
│  └──────────┬───────────────────────────────────────────────────┘   │
│             │ depends on                                             │
│  ┌──────────▼───────────────────────────────────────────────────┐   │
│  │              render.engine                                   │   │
│  │  RenderEngine — orchestrator หลัก                             │   │
│  │  RenderPartRequest, Progress, RenderResult                   │   │
│  │  source cache mgmt (VoicebankFiles)                          │   │
│  └──┬────────┬──────────┬───────────────────────────────────────┘   │
│     │        │          │                                            │
│     │  depends on      │                                            │
│     │        │          │                                            │
│  ┌──▼────────▼──────────▼──┐  ┌──────────────────────────────┐     │
│  │     render.api          │  │     signal                   │     │
│  │  <<interface>>          │  │  WaveMix, Fader, ISignalSrc  │     │
│  │  IRenderer              │  │  MasterAdapter, ExportAdapter│     │
│  │  IResampler             │  │  Effects (Reverb, EQ, ...)   │     │
│  │  IWavtool               │  └──────────┬───────────────────┘     │
│  └──────────┬──────────────┘             │                          │
│             │                             │                          │
│  ┌──────────▼─────────────────────────────▼───────────────────┐     │
│  │                  render.impl                                │     │
│  │  ┌──────────┬──────────┬──────────┬──────────┬─────────┐  │     │
│  │  │ classic  │worldline │ diffsinger│  enunu  │ vogen   │  │     │
│  │  │Renderer  │Renderer  │ Renderer  │ Renderer│ Renderer│  │     │
│  │  │Resampler │Resampler │           │         │         │  │     │
│  │  │Wavtool   │          │           │         │         │  │     │
│  │  │ResampItem│          │           │         │         │  │     │
│  │  └──────────┴──────────┴──────────┴──────────┴─────────┘  │     │
│  └──────────────────────┬─────────────────────────────────────┘     │
│                         │ depends on                                │
│  ┌──────────────────────▼─────────────────────────────────────┐     │
│  │                  voicebank                                  │     │
│  │  VoicebankLoader, VoicebankConfig, Voicebank               │     │
│  │  Oto, OtoSet, Subbank, UOto, Frq, prefix.map reader        │     │
│  │  SingerManager, USinger (singer registry)                  │     │
│  └──────────┬─────────────────────────────────────────────────┘     │
│             │ depends on                                             │
│  ┌──────────▼─────────────────────────────────────────────────┐     │
│  │               phonemizer                                    │     │
│  │  ┌──────────────┐  ┌─────────────────────────────┐         │     │
│  │  │ phonemizer    │  │ phonemizer.impl              │         │     │
│  │  │ <<interface>> │  │ JapaneseVCV, EnglishVCCV,    │         │     │
│  │  │ Phonemizer    │  │ ThaiVCCV, ChineseCVVC, ...   │         │     │
│  │  │ IG2p          │  │ G2p implementations          │         │     │
│  │  │ PhonemizerFtry │ └─────────────────────────────┘         │     │
│  │  └──────────────┘                                           │     │
│  └──────────┬─────────────────────────────────────────────────┘     │
│             │ depends on                                             │
│  ┌──────────▼─────────────────────────────────────────────────┐     │
│  │                   domain                                    │     │
│  │  UProject, UTrack, UPart, UVoicePart,                      │     │
│  │  UNote, UPhoneme, UCurve, UExpression                      │     │
│  │  UTempo, UTimeSignature, UOto, UPitch, UVibrato            │     │
│  │  URenderSettings, UExpressionDescriptor                    │     │
│  │  TimeAxis                                                  │     │
│  │  <<pure data model — zero dependencies>>                   │     │
│  └──────────┬─────────────────────────────────────────────────┘     │
│             │ depends on                                             │
│  ┌──────────▼─────────────────────────────────────────────────┐     │
│  │                   format                                    │     │
│  │  Ustx (.ustx serialization/deserialization)                │     │
│  │  VSQx / MusicXML / Wave / Ufdata importers                 │     │
│  │  MidiWriter                                                │     │
│  └────────────────────────────────────────────────────────────┘     │
│                                                                     │
│  ┌────────────────────────────┐  ┌─────────────────────────────┐   │
│  │      cache                 │  │       util                  │   │
│  │  RenderCache (LRU)         │  │  MusicMath, Preferences     │   │
│  │  File cache (hash-based)   │  │  PathManager, Base64, Yaml  │   │
│  │                            │  │  LibraryLoader, OS, Zip     │   │
│  └────────────────────────────┘  │  TimeAxis, NotePresets      │   │
│                                  │  SingletonBase              │   │
│                                  └─────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 📋 Responsibility ของแต่ละ Package

### `domain` — Data Model
**ไม่มี dependency กับ package อื่นใน project**

| Component | Responsibility |
|---|---|
| `UProject` | Project root: tempos, time signatures, tracks, parts, expressions |
| `UTrack` | Track: singer assignment, renderer settings, phonemizer config |
| `UNote` | Note: position, duration, tone, lyric, pitch, vibrato |
| `UVoicePart` | Voice part: sorted notes, curves, phonemes |
| `UPhoneme` | Phoneme: envelope, oto ref, preutter/overlap, timing |
| `UCurve` | Pitch/expression curve: points + interpolation |
| `UExpression` | Expression value: abbr + value + descriptor ref |
| `UOto` | Oto mapping: alias, wav, offset, consonant, preutter, overlap |
| `TimeAxis` | Tick↔ms conversion, tempo/time sig segments |

---

### `format` — File I/O
**depends on:** `domain`

| Component | Responsibility |
|---|---|
| `.ustx` serializer | YAML serialize/deserialize UProject (project file) |
| VSQx importer | Vocaloid VSQx format import |
| MusicXML importer | MusicXML score import |
| Wave reader | WAV/Opus/Ogg audio file reading |
| MIDI writer | MIDI file export |
| Ufdata | UTAU `.ust` / `.tmp` import |

---

### `voicebank` — Voicebank Management
**depends on:** `domain`

| Component | Responsibility |
|---|---|
| `VoicebankLoader` | ค้นหา voicebank directory, โหลด config + oto |
| `VoicebankConfig` | `character.txt`, `character.yaml`, `config.yaml` schema |
| `VoiceBank` | Data model: BasePath, OtoSets, Subbanks |
| `Oto` / `OtoSet` | oto.ini structure + resolution |
| `SingerManager` | Registry singers ทั้งหมด, reload, memory management |
| `USinger` | Singer instance: loaded voicebank, oto mapping, subbanks |
| `Frq` | Frequency file (.frq) reader |

---

### `phonemizer.api` — Phonemizer Framework
**depends on:** `domain`, `voicebank`

| Component | Responsibility |
|---|---|
| `Phonemizer` (abstract) | `SetSinger()`, `Process()` → phoneme output |
| `PhonemizerFactory` | Plugin discovery + instantiation |
| `IG2p` | Grapheme-to-phoneme interface |
| `G2pDictionary` | Dictionary-based G2p lookup |
| `PhonemizerRunner` | Orchestrate phonemizer execution for a part |

---

### `phonemizer.impl` — Phonemizer Implementations
**depends on:** `phonemizer.api`, `domain`

40+ phonemizers:
- `EnglishVCCV`, `JapaneseVCV`, `KoreanCVVC`, `ChineseCVVC`
- `ThaiVCCV`, `FrenchVCCV`, `SpanishVCCV`, `GermanDiphone`, etc.
- `DiffSinger*Phonemizer` per language (Chinese, English, Japanese, etc.)
- G2p implementations per language

---

### `render.api` — Rendering Interfaces
**depends on:** `domain`, `voicebank`

| Interface | Method | Responsibility |
|---|---|---|
| `IRenderer` | `Layout()`, `Render()`, `LoadRenderedPitch()` | Phrase-level renderer |
| `IResampler` | `DoResampler(item)`, `SupportsFlag()` | Sample-level resampling |
| `IWavtool` | `Concatenate(items)` | Concatenate resampled files |
| `RenderResult` | `float[] samples`, `leadingMs`, `positionMs` | Output data |

---

### `render.impl` — Renderer Implementations
**depends on:** `render.api`, `voicebank`, `domain`

| Renderer | Engine | Notes |
|---|---|---|
| **classic** | ClassicRenderer + ResamplerItem | UTAU-compatible, external or in-process |
| **worldline** | WorldlineRenderer + WorldlineResampler | WORLD vocoder (C++ in-process) |
| **diffsinger** | DiffSingerRenderer | AI-based (ONNX model, neural net) |
| **enunu** | EnunuRenderer | Japanese TTS-based (HTS-style) |
| **vogen** | VogenRenderer | Mandarin/Cantonese neural |
| **voicevox** | VoicevoxRenderer | Voicevox TTS integration |

---

### `render.engine` — Render Orchestrator
**depends on:** `render.api`, `render.impl`, `domain`

| Component | Responsibility |
|---|---|
| `RenderEngine` | Prepare requests → dispatch → mixdown |
| `RenderPartRequest` | Container: part + phrases + sources + mix |
| `Progress` | Progress tracking + notification |
| `VoicebankFiles` (moved) | Source file caching, hash-based temp management |

---

### `signal` — Audio DSP / Mixing
**depends on:** none (pure signal processing)

| Component | Responsibility |
|---|---|
| `ISignalSource` | Audio source interface: `Mix(position, buffer, ...)` |
| `WaveMix` | Mix multiple ISignalSources |
| `Fader` | Volume + pan per track |
| `MasterAdapter` | Master output adapter (for audio driver) |
| `ExportAdapter` | Export adapter (to .wav file) |
| `MixFxSource` | Track FX wrapper (reverb/EQ/compressor) |
| Effects | `Freeverb`, `BiquadEQ`, `SimpleCompressor` |

---

### `playback` — Audio Playback
**depends on:** `render.engine`, `signal`

| Component | Responsibility |
|---|---|
| `PlaybackManager` | Play/pause/stop, loop, subscribe to commands |
| `IAudioOutput` | Audio output interface |
| `AudioTrackOutput` (Android) | Android AudioTrack implementation |
| `SineGenerator` / `ToneGenerator` | Test tone generation |

---

### `cache` — Caching
**depends on:** none

| Component | Responsibility |
|---|---|
| `RenderCache` | LRU in-memory cache (hash → byte[]) |
| File cache | hash-based `.wav` file caching on disk |

---

### `util` — Utilities
**depends on:** none

| Component | Responsibility |
|---|---|
| `MusicMath` | Math helpers: decibel↔linear, tempo conversions, interpolation |
| `Preferences` | User preferences singleton |
| `PathManager` | Path resolution: singers, cache, backup, plugins |
| `TimeAxis` | Tick/ms conversion (shared with domain) |
| `Yaml` | YAML serializer/deserializer wrapper |
| `SingletonBase` | Singleton pattern base class |

---

## 🔗 Dependency Graph (Simplified)

```
                   ┌──────────┐
                   │  util    │
                   └────┬─────┘
                        │ (no dep)
                   ┌────▼─────┐
            ┌──────┤  domain  ├──────────┐
            │      └────┬─────┘          │
            │           │                │
       ┌────▼────┐ ┌────▼─────┐   ┌─────▼──────┐
       │ format  │ │voicebank │   │   cache    │
       └─────────┘ └────┬─────┘   └────────────┘
                        │
              ┌─────────┼─────────┐
         ┌────▼────┐ ┌──▼───┐ ┌──▼──────────┐
         │phoneme  │ │render│ │phoneme.impl  │
         │.api     │ │.api  │ └──────────────┘
         └────┬────┘ └──┬───┘
              │         │
         ┌────▼────┐ ┌──▼───────────────┐
         │phoneme  │ │ render.impl      │
         │.impl    │ │(classic,worldline│
         └─────────┘ │ diffsinger, etc.)│
                     └──┬───────────────┘
                        │
                   ┌────▼──────┐
                   │render     │
                   │.engine    │
                   └────┬──────┘
                        │
              ┌─────────┼─────────┐
         ┌────▼────┐ ┌──▼────┐    │
         │ signal  │ │playback│    │
         └─────────┘ └───────┘    │
                                  │
                     ┌────────────▼─────────┐
                     │  Application Layer    │
                     │ (Android Service/UI)  │
                     └──────────────────────┘
```

---

## 🎯 สรุป Key Design Decisions

| Decision | Rationale |
|---|---|
| `domain` ไม่มี dep | Pure data model — porting ง่ายที่สุด, ทดสอบแยกได้ |
| `render.api` แยกจาก `render.impl` | Interface-based — สามารถสลับ renderer backend ได้ |
| `voicebank` แยกจาก `render` | Voicebank format ต้อง support หลายเวอร์ชัน |
| `signal` ไม่ dep กับ domain | Pure DSP — reuse ได้กับ project อื่น |
| `phonemizer.api` แยก `phonemizer.impl` | Plugin architecture — เพิ่มภาษาได้ไม่ต้องแก้ core |
| Hash-based file caching | ไม่ต้อง re-render ถ้า parameter ไม่เปลี่ยน |
| LRU in-memory cache | สำหรับ pitch data ที่คำนวณแล้ว |
