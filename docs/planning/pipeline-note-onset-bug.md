# pipeline-note-onset-bug — root cause "โน้ตช่วงต้นผิด" ในเส้นทาง pipeline (T9)

> task: `task-9` (r2-wasm) · วันที่: 2026-09-23 · สถานะ: **เจอ root cause + แก้ + verify แล้ว**
> อาการที่ผู้ใช้เจอ: โน้ตช่วงต้นเงียบ/ผิดจังหวะ — เสียงโน้ตเดียว "sa" เงียบ ~750 ms แล้วค่อยดัง
> ไฟล์อ้างอิง: `test/golden/one-note.ustx` (lyric sa, tone 69, dur 1440 ticks) · bank `test/golden/teto-english/library`

## 0. สรุป

**เจอ 2 root cause ที่เป็นอิสระต่อกัน** (ทั้งคู่เกิดเฉพาะเส้นทาง pipeline ไม่ใช่ phrase renderer):

| # | Root cause | ที่ไหน | อาการที่ตามมา |
|---|---|---|---|
| 1 | **japanese phonemizer แบ่งโน้ตครึ่งต่อครึ่ง** — แยก 1 โน้ตเป็น `[s, "- a"]` แล้ว `split_duration` แบ่ง duration เท่ากัน | `native/crates/phonemizer/src/japanese.rs:297` | โน้ต 1500 ms ถือ consonant 750 ms → **สระเริ่มที่ 750 ms** (ตรงกับที่ผู้ใช้/Lead เห็น 66,049 samples เงียบ 750 ms) |
| 2 | **mixer หัก leading ซ้ำ (double-subtract)** — `trim_chunk_audio` คืน `position_ms` เป็น *เวลาเริ่มของ buffer* แล้ว แต่ `mix()` หัก `leading_ms` อีกครั้ง | `native/tools/synth-cli/src/pipeline.rs:342` + `native/crates/runtime/src/mixer.rs:219,230` | ทุก chunk ถูกวางก่อนเวลาจริง 1 leading; ที่ t=0 ติดลบ → ถูก clamp เป็น 0 → **pre-roll 135.7 ms หลุดเข้า mixdown** (1636 ms แทน 1501 ms) |

**ซึ่ง phrase renderer เองถูกต้อง** — พิสูจน์แล้วว่า output ของ pipeline (ก่อนแก้) เท่ากับ golden phrase ที่ **corr@0 = 1.000000**:

```text
our-english.wav (72132 samples, pre-fix pipeline) vs g-note-r1.wav (72193, OpenUtau phrase):
  corr@0 = 1.000000   best lag 0   -> renderer + oto offset ถูกต้องทั้งหมด
```

## 1. Reproduce (ก่อนแก้)

```bash
cd /media/DATA/project/android-voice-synth
SO=native/build/build-linux/libworldline.so
./native/target/release/synth-cli render --project test/golden/one-note.ustx \
  --voicebank test/golden/teto-english/library --so $SO --engine r1-rs --phonemizer japanese --out /tmp/before-jp.wav
```

| render | samples | duration | onset (>-30 dBFS, ab_metrics) |
|---|---|---|---|
| golden demo (OpenUtau) | 66,211 | 1501.4 ms | 50 ms |
| ของเรา `--phonemizer japanese` (ก่อนแก้) | 66,049 | 1497.7 ms | **150 ms** + เงียบระดับ ~0.024 RMS ถึง 750 ms |
| ของเรา default/universal (ก่อนแก้) | 72,132 | 1635.6 ms | 150 ms |

## 2. Root cause #1 — japanese phonemizer แบ่งโน้ตครึ่งต่อครึ่ง

## 2.1 หลักฐานจาก debug dump (env-gated ที่ใส่ชั่วคราวเพื่อหา root cause — ถอดออกแล้ว)

```text
[chunk-dbg] ctx=0..2 own=0..2 start_ms=0.000 end_ms=1500.000
[chunk-dbg] phoneme[0] s   pos=0.000   lead=0.731  dur=750.000
[chunk-dbg] phoneme[1] - a pos=750.000 lead=1.994  dur=750.000
[chunk-dbg] oto[0] offset=742.069 preutter=0.731 cutoff=-122.467   (_s3_s3_s-.wav=s)
[chunk-dbg] oto[1] offset=100.000 preutter=1.994 cutoff=-275.001
```

⇒ โน้ต 1500 ms ถูกแบ่งเป็น 2 phoneme เท่า ๆ กัน (750/750) สระจึงเริ่มที่ 750 ms

## 2.2 ต้นเหตุในโค้ด

`native/crates/phonemizer/src/japanese.rs:297`:

```rust
let durations = split_duration(note.duration, symbols.len());   // 1440 ticks / 2 = 720/720
let positions = positions_from_durations(&durations);
```

`split_duration` (`phonemizer/src/phonemizer.rs:65`) หารเท่ากันทุก phoneme ⇒ phoneme แรก (consonant) กินครึ่งโน้ต

## 2.3 Reference (OpenUtau) ทำอะไร

`ref-openutau-clean/core/OpenUtau.Plugin.Builtin/JapaneseVCVPhonemizer.cs:72-88` (และ `DefaultPhonemizer.cs:26-32`,
`Api/Phonemizer.cs:115-137`):

```csharp
if (CheckOtoUntilHit(tests, note, out var oto)) {
    return new Result { phonemes = new Phoneme[] { new Phoneme { phoneme = oto.Alias } } };
}
```

⇒ คืน **1 phoneme ต่อ 1 โน้ต** (alias เดียวคุมทั้งโน้ต; `Phoneme` ใน OpenUtau มีแค่ `position` ไม่มี duration)
และ tests ของ alias คือ `"{prev vowel} {lyric}"`, `"* {lyric}"`, `"{lyric}"`, `"- {lyric}"`
⇒ **ไม่มีการแบ่ง duration ของโน้ตเลย**

## 3. Root cause #2 — mixer หัก leading ซ้ำ

`native/tools/synth-cli/src/pipeline.rs::trim_chunk_audio` คืน `(samples, position_ms, leading_ms)` โดย `position_ms`
คือ **เวลาโปรเจกต์ของ samples[0]** (= `chunk.start_ms - first_lead`) แต่ `runtime::mixer::mix` ใช้

```rust
let start = ms_to_samples(input.chunk.position_ms - input.chunk.leading_ms, SAMPLE_RATE);
```

⇒ หัก leading **ครั้งที่สอง** ⇒ chunk ถูกวางเร็วเกิน 1 leading ทุก chunk
และเมื่อค่าติดลบ (โน้ตที่ t=0) `ms_to_samples` cast เป็น usize → 0 ⇒ ทั้ง pre-roll ถูกเขียนที่ sample 0
(ไม่ได้ถูกตัด) ⇒ mixdown ยาว 72,132 samples แทนที่จะเป็น 66,211 เท่า golden

หลักฐานยืนยันว่า convention ที่ mixer ใช้ถูกต้อง (และ `trim_chunk_audio` เป็นฝ่ายผิด):
`runtime::mixer::tests::alignment_uses_position_minus_leading` เขียนคาดหวังไว้แบบนั้นอยู่แล้ว
และ comment ของ `AudioChunk` (`pipeline.rs`) ก็เขียนว่า "samples[0] starts at (position_ms - leading_ms)"

## 4. สิ่งที่แก้

| ไฟล์ | แก้อะไร |
|---|---|
| `native/crates/phonemizer/src/japanese.rs` | เพิ่มเส้นทาง **1 phoneme ต่อโน้ต** ตาม reference: ลอง alias ทั้งคำ (`"{tail} {lyric}"`, `"* {lyric}"`, `"{lyric}"`, `"- {lyric}"`) ก่อน ⇒ เจอ → emit phoneme เดียวที่ `position=0`, `duration = note.duration`; **ถ้าไม่เจอ alias ทั้งคำ ค่อย fallback ไป mora-split เดิม** (แบงก์ที่ไม่มี alias ทั้งคำไม่พัง) · เพิ่ม `resolve_any()` คืน bool ว่าเจอ alias ไหม |
| `native/tools/synth-cli/src/pipeline.rs` | `trim_chunk_audio` คืน `chunk.start_ms` (แทน `chunk.start_ms - first_lead`) ให้ตรง convention ของ mixer |
| `native/crates/runtime/src/mixer.rs` | เพิ่ม `ms_to_samples_signed`; `mix()` คำนวณ start แบบมีเครื่องหมาย และ **ข้าม sample ที่ target < 0** (ตัด pre-roll) แทนการ clamp ตำแหน่ง |

หมายเหตุ: ไฟล์ `pipeline.rs` มี 4 hunks ของ **task-12** (PhonemizerKind::Default) แปะเพิ่มตามที่ Lead อนุมัติ — อยู่คนละส่วนกับงานนี้

## 5. ผลหลังแก้ (วัดด้วย `test/golden/ab_metrics.py` + cross-correlation)

| render | samples | duration | onset | waveform corr@0 vs golden demo |
|---|---|---|---|---|
| golden demo (OpenUtau) | 66,211 | 1501.4 ms | 50 ms | — |
| ของเรา japanese **ก่อนแก้** | 66,049 | 1497.7 ms | **150 ms** | +0.185 (บังเอิญ: มี silence 750 ms) |
| ของเรา japanese **หลังแก้** | **66,150** | **1500.0 ms** | **50 ms** | -0.108 (ใช้ alias `- sa` ตาม reference จึงคนละเนื้อกับ golden demo — ดู §6) |
| ของเรา default/universal **ก่อนแก้** | 72,132 | 1635.6 ms | 150 ms | **-0.0179** |
| ของเรา default/universal **หลังแก้** | **66,150** | **1500.0 ms** | **50 ms** | **+0.3797** |

- **onset 750 ms → 0 ms** (นิยาม 50 ms window แรกที่ RMS > -30 dBFS: 150 ms → 50 ms เท่ากับ golden)
- **ความยาว 1636 ms → 1500.0 ms** (golden 1501.4 ms, ต่าง 1.4 ms เท่ากับผลต่างระดับ phrase ที่มีอยู่ก่อนแล้ว)
- corr กับ golden demo ของเส้นทาง default: **-0.0179 → +0.3797**

## 6. ข้อจำกัด / สิ่งที่ยังไม่ตรง (รายงานตรงไปตรงมา)

1. **envelope ของ golden demo ยังไม่ตรง** — golden demo มี amplitude envelope ที่แกว่ง 0.17–0.30 (50 ms RMS)
   ขณะที่ของเราแบน ~0.21–0.23 ⇒ envelope Pearson corr@lag0 = **-0.17** (waveform best-lag corr ~0.52)
   เป็นความต่างปลายน้ำของเส้นทาง mixdown/demo ของ OpenUtau **ไม่ใช่อาการ onset ของ T9** และงานนี้ไม่ได้แก้
   (คำเตือน: ถ้าวัดแบบ cosine ไม่ลบ mean จะได้ ~0.96 ซึ่งทำให้เข้าใจผิด — เอกสารนี้ใช้ Pearson)
2. **japanese path เลือก alias ต่างจาก golden demo** — ตาม reference `JapaneseVCVPhonemizer` จะลอง `- sa` ก่อน
   (offset 80 ms) จึงได้เนื้อเสียงต่างจาก golden demo ซึ่งตรงกับ alias `sa` (offset 736.336 ms);
   เส้นทาง **universal/default** ของเราตรงกับ golden phrase ที่ corr 1.000000 ⇒ alias `sa`
3. **ไม่เกี่ยวกับงานนี้แต่พบระหว่างทาง:** `cargo test -p phonemizer` มี 2 เทสต์พังอยู่ก่อนแล้ว
   (`tests/rusted_utau_dict.rs:21` hardcode path `/home/seal/project/android-voice-synth/...` แต่ repo อยู่ที่ `/media/DATA/...`)
4. **บทเรียนการ build:** binary `synth-cli` **ต้อง build ด้วย `--features cli`** (`[[bin]] required-features = ["cli"]`)
   — `cargo build -p synth-cli` เฉย ๆ จะไม่ relink binary ทำให้การ reproduce ก่อนหน้านี้ใช้ binary เก่า (10:50)
   ⇒ ใช้ `cargo build --release -p synth-cli --features cli`

## 7. Regression tests ที่เพิ่ม

| test | ไฟล์ | รันปกติ? | กันอะไร |
|---|---|---|---|
| `negative_start_trims_the_preroll_instead_of_shifting` | `native/crates/runtime/src/mixer.rs` | ✅ รันเสมอ | กัน mixer clamp แทนการตัด pre-roll |
| `one_phoneme_per_note_like_openutau` | `native/crates/phonemizer/src/japanese.rs` | ✅ รันเสมอ | กันการแบ่งโน้ตครึ่ง (1 โน้ต = 1 phoneme, duration = ทั้งโน้ต) |
| `falls_back_to_the_mora_split_without_a_whole_alias` | `native/crates/phonemizer/src/japanese.rs` | ✅ รันเสมอ | กัน fallback พังสำหรับแบงก์ที่ไม่มี alias ทั้งคำ |
| `japanese_one_note_starts_at_zero_and_lasts_one_note` | `native/tools/synth-cli/tests/note_onset.rs` | ⏭️ `--ignored` | onset ≤ 100 ms + ความยาว 1500 ms ± 25 |
| `universal_one_note_trims_the_phrase_leading` | `native/tools/synth-cli/tests/note_onset.rs` | ⏭️ `--ignored` | mixdown ต้องตัด pre-roll |
| `universal_one_note_matches_the_golden_demo_onset_and_length` | `native/tools/synth-cli/tests/note_onset.rs` | ⏭️ `--ignored` | เทียบ duration/onset กับ golden demo (`GOLDEN_DEMO=...`) |

คำสั่งรัน (ทดสอบจริงแล้วผ่านทั้ง 3 ตัว):

```bash
cargo test -p runtime -p phonemizer                     # 56 + 27 passed
cargo test -p synth-cli --test note_onset -- --ignored   # 3 passed
```

## 8. ไฟล์ที่แตะ

| ไฟล์ | สถานะ |
|---|---|
| `native/crates/phonemizer/src/japanese.rs` | fix 1 phoneme/โน้ต + fallback + 2 unit tests |
| `native/crates/runtime/src/mixer.rs` | `ms_to_samples_signed` + ตัด pre-roll + 1 unit test |
| `native/tools/synth-cli/src/pipeline.rs` | `trim_chunk_audio` fix + 4 hunks ของ task-12 (ตามที่ Lead อนุมัติ) |
| `native/tools/synth-cli/tests/note_onset.rs` | integration test ใหม่ (ignored by default) |
| `docs/planning/pipeline-note-onset-bug.md` | เอกสารนี้ |

ไม่ได้แตะ: `web/**`, `models/**`, `ref/**`, `ref-openutau-clean/**`, เอกสารของ T1/T2/T4/T5/T6/T8
