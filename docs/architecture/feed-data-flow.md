# Feed Data Flow Tree

> แสดงการแปลงข้อมูลแต่ละขั้น (derivation chains) — core สร้างให้ครบทุก stage
> plugin แต่ละตัวเลือก "tap" ตาม capability
> อัปเดต: 2026-08-02

---

## 1. LYRIC → PHONEME → TOKEN

```
lyric ("あ" หรือ "read[r iy d]")
  │
  ├─ Phonemizer.process(notes, singer)
  │    ├─ lyric → phoneme sequence (rule-based / G2p)
  │    └─ phonetic hint [r iy d] → ใช้ตรงๆ
  │
  ▼
phoneme ("あ")
  │
  ├─→ [worldline/classic] OtoMapper
  │     phoneme + tone + voiceColor + subbank
  │     ├─ prefix.map lookup → "あC5"
  │     └─ oto.ini lookup → Oto { wav, offset, consonant,
  │                              cutoff, preutter, overlap }
  │
  ├─→ [diffsinger] Tokenizer
  │     phoneme → phonemes.txt index → token (Int64)
  │     "SP" + tokens[] + "SP"  (padding 8 frames head/tail)
  │
  ├─→ [vogen] phs[] (string ตรงๆ — ไม่มี token)
  │
  └─→ [enunu] .ust writer (phoneme → .ust note)
        └─→ [voicevox] dic lookup (lyric → kana → phoneme)
```

## 2. TIMING → DURATION → FRAMES

```
note.position/duration (ticks)
  │
  ├─ TimeAxis.TickPosToMsPos
  │
  ▼
phoneme.positionMs / durationMs
  │
  ├─→ [worldline/classic] ใช้ ms ตรงๆ
  │     + preutter/overlap/durCorrection → SynthRequest
  │
  └─→ [neural] DurationsMsToFrames
        durationMs / frameMs (hop_size/rate*1000)
        → durations[] (Int64 frames)
        ├─ [diffsinger] SP 8 + durations + SP 8
        ├─ [vogen] head(50) + durations + tail(50)
        └─ [enunu] .ust duration (ms → ticks @tempo)
```

## 3. PITCH → PITCHES → F0 → SHIFTED F0

```
note.tone + tuning (MIDI + cents)
  │
  ├─ + vibrato (period/depth/fadeIn/fadeOut/phase)
  ├─ + pitch points (PitchPoint: xMs, yCent, shape)
  ├─ + PITD curve
  └─ + mod+ (frq-based, optional)
  │
  ▼
pitches[] (cents, every 5 ticks)
  │
  ├─→ [worldline/classic] pitch_bend[] (int32 cents)
  │     └─→ SynthRequest.pitch_bend
  │
  └─→ [neural] ToneToFreq(cents/100) → f0[] (Hz/frame)
        │
        ├─→ [diffsinger] f0 + toneShift×2^(d/1200) → shiftedF0
        ├─→ [vogen] f0 + toneShift → f0Shifted
        └─→ [enunu] editorF0 (Hz) → .ust pitch bend / .npy
```

## 3.5. CURVE REPRESENTATION (points + equation → dense sampling)

### Project เก็บ curve เป็น sparse (points + equation)

```
UCurve {
  xs[]:    [0, 480, 960]        ← ตำแหน่ง (ticks)
  ys[]:    [0, -50, 30]         ← ค่า
  shape[]: ["io", "li", "sp"]   ← สมการ interpolation ระหว่างจุด
}

PitchPoint { xMs, yCent, shape }  ← pitch ก็เหมือนกัน
  shape = io (linear) / li (linear) / si / sp (spline) / hsin / csin
```

ข้อดี: compact, แก้ไขง่าย (ลากจุด), serialize ง่าย, undo/redo ง่าย

### การแปลง 3 ขั้น (ก่อนเข้า plugin)

```
PROJECT (sparse)
  UCurve { xs[], ys[], shape[] }     ← จุด + สมการ
        │
        │  STEP 1: SampleCurve() — core
        │  ใช้ shape interpolate ระหว่างจุด, interval = 5 ticks
        ▼
CORE FEED (dense per-tick)
  float[] per 5 ticks                 ← [0, -12, -25, ...]
        │
        │  STEP 2: per-frame sampling — core
        │  (ตาม frame size ของ renderer)
        ▼
PER-FRAME (dense per frame)
  float[] per frameMs                 ← ตาม frame size
        │
        │  STEP 3: normalization — plugin ทำเอง
        ▼
PLUGIN INPUT
  worldline:  gender = 0.5 + 0.005×x
  diffsinger: embed scale / delta
```

### Step 1: points+eqn → per-5-tick (core)

```rust
// core: curve.rs
fn sample_curve(curve: &Curve, start: i32, length: usize) -> Vec<f32> {
    // สำหรับแต่ละตำแหน่ง: หาจุดรอบข้าง + interpolate ด้วย shape
    // io/li: linear interpolation
    // sp: cubic spline (MusicMath.InterpolateShape)
}
```

### Step 2: per-tick → per-frame (core — ตาม frame size)

```rust
// core: feed.rs
fn sample_per_frame(curve: &[f32], frame_ms: f64, time_axis: &TimeAxis) -> Vec<f32> {
    // index = ticks / interval (จาก positionMs → tick)
}
```

### Step 3: normalization (plugin ทำเอง)

```rust
// plugin: worldline
gender = 0.5 + 0.005 * raw_gender[i]

// plugin: diffsinger
velocity = 2f32.powf((raw_velc[i] - 100.0) / 100.0)
```

### สัญญา

```
Project เก็บ:   points + equation (sparse)     ← แก้ไขง่าย
Core feed สร้าง: per-5-tick float[] (dense)    ← ครั้งเดียว ทุก renderer
Core สร้างต่อ:   per-frame float[] (dense)     ← ตาม frame size
Plugin รับ:      per-frame raw values          ← normalize เอง

Cache point:    sample ครั้งเดียว → cache (hash จาก points+eqn)
                → แก้ point → hash เปลี่ยน → re-sample
```

---

## 4. CURVES → SAMPLED CURVES (per frame / per tick)

```
part.curves[] { abbr: dyn/pitd/genc/brec/tenc/voic/... }
  │
  ├─ SampleCurve(5-tick interval)
  │
  ▼
curve[] per 5 ticks
  │
  ├─→ [worldline/classic] flags จาก expressions
  │     ├─ gen → flag_g (-100..100)
  │     ├─ bre → flag_Mb
  │     ├─ tenc → flag_Mt
  │     ├─ voic → flag_Mv
  │     └─ norm → flag_P (peak compression)
  │     + frame-based gender/tension/breathiness/voicing
  │       (0.5 + 0.005×value normalization)
  │
  ├─→ [diffsinger] sample ต่อ frame (frameMs)
  │     + variance predictor → energy/bre/voic/tenc
  │       (ถ้า acoustic ต้องการ + use*Embed)
  │     + gender → key shift embed scale
  │     + velocity → speed embed (2^((x-100)/100))
  │
  └─→ [vogen] gender/tension → 0.5+0.005×x per frame
```

## 5. ENVELOPE + FLAGS (เฉพาะ sample-based)

```
phoneme.envelope (p1..p5: xMs, y%)
  │
  ├─ EnvelopeMsToSamples (×44100/1000, + skipOver)
  │
  ▼
envelope[] (samples space)
  │
  └─ ApplyEnvelope(samples) — gain multiply

flags (จาก expressions)
  ├─ gen → "g5", bre → "B2", tenc → "Mt10", voic → "Mv90"
  ├─ norm → "P86"
  └─ GetFlagsString → "g5B2H10P86"
```

## 6. NOTES → VOGEN INPUTS (เฉพาะ vogen)

```
notes[]
  ├─ tone → notePitches[] (float, 0 + notes + 0)
  ├─ durationMs → noteDurs[] (frames, head + notes + tail)
  └─ index → noteToCharIndex[]
phonemes[]
  ├─ phoneme → phs[] (string, "" + phones + "")
  └─ durationMs → phDurs[] (frames)
  │
  ▼
f0_man.onnx(notePitches, noteDurs, noteToCharIndex, phs, phDurs)
  → f0 (predicted)
  → singer.onnx(phs, phDurs, f0, breAmp) → mgc + bap
  → DecodeMgc/DecodeBap → sp/ap → VSVocoder → audio
```

## 7. PHRASE → .UST (เฉพาะ enunu)

```
phonemes + notes + tempo
  │
  ├─ EnunuUtils.WriteUst()
  │    ├─ phoneme + duration + tone per note
  │    ├─ flags/expressions
  │    └─ tempo (BPM)
  │
  ▼
.ust file → ZMQ ["acoustic", ustPath, "", vbHash, "600"]
  → f0/sp/ap หรือ mel (.npy files)
  → ZMQ ["synthe", ustPath, wavPath, vbHash, "600"]
  → wav → samples
```

## 8. PHRASE → VOICEVOX PARAMS (เฉพาะ voicevox)

```
notes/lyric
  ├─ dic lookup (lyric → kana) + phoneme classification
  ├─ NoteGroupsToVQuery → query
  │
  ▼
VoicevoxSynthParams {
  phonemes[] { phoneme, frame_length },
  f0[] (per frame),
  volume[] (per frame),
  speedScale, pitchScale, intonationScale
}
  │
  └─ POST /frame_synthesis?speaker={id} → wav
```

---

## สรุป: สายการแปลงหลัก (derivation chains)

```
1. lyric ──phonemizer──► phoneme ──oto mapper──► mapped alias + oto
                                 ──tokenizer──► token[]
                                 ──dic lookup──► vv phoneme

2. ticks ──timeAxis──► ms ──/frameMs──► frames (durations[])

3. tone+vibrato+PITD ──► pitches[](cents) ──ToneToFreq──► f0[](Hz)
                                          ──+toneShift──► shiftedF0

4. curves ──sample──► per-tick/per-frame ──normalize──► flags / embeds

5. phoneme ──► envelope + flags (sample-based เท่านั้น)

6. notes ──► notePitches/noteDurs/noteToCharIndex (vogen)

7. phrase ──► .ust file (enunu)

8. phrase ──► VoicevoxSynthParams (voicevox)
```

---

## Core Transformers (12 ตัว — implement ครบ = support ทุก plugin)

| # | Transformer | Output | ใช้กับ |
|---|---|---|---|
| 1 | Phonemizer | phoneme[] | ทุกตัว |
| 2 | OtoMapper | mapped alias + oto | worldline/classic |
| 3 | Tokenizer | token[] | diffsinger/enunu |
| 4 | TimeAxis | ms | ทุกตัว |
| 5 | DurationsToFrames | frames | neural 3 ตัว |
| 6 | PitchComputer | pitches[] | ทุกตัว |
| 7 | ToneToFreq | f0[] | neural 3 ตัว |
| 8 | CurveSampler | per-frame curves | ทุกตัว |
| 9 | EnvelopeBuilder | envelope | worldline/classic |
| 10 | FlagBuilder | flags | worldline/classic |
| 11 | UstWriter | .ust | enunu |
| 12 | VvParamsBuilder | VV params | voicevox |

---

## Pipeline: Core สร้างตาม capability

```
1. Phonemizer → phonemes
2. Pitch computation (vibrato + PITD + toneShift) → pitches[]
3. Curve sampling → dyn/genc/brec/tenc/voic
4. [ถ้ามี plugin needs_oto]    → oto lookup + envelope + flags
5. [ถ้ามี plugin needs_wav]    → wav path + frq
6. [ถ้ามี plugin needs_neural] → tokens + durations + f0 + (vogen extras)
7. [ถ้ามี plugin needs_variance] → variance predictor → curves
8. [enunu]                    → .ust builder
9. [voicevox]                 → VV params builder
```
