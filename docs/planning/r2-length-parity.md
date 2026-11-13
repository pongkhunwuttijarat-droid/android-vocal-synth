# เส้นทาง R2 ยาวกว่า reference 511 samples — tail หายไปไหน (T20)

> วันที่ 2026-09-23 · task-20 · ตรวจด้วยของจริง: golden harness **phrase mode** (mono, เลือก renderer ได้) + mel dump + render ของเรา
> · scope: `native/crates/worldline-plugin/**` + เอกสารนี้ ⇒ **งานนี้ไม่แก้โค้ด** (จุดที่ต้องแก้คือ `synth-cli/src/pipeline.rs` ซึ่งอยู่นอก scope)
> · คำสั่ง golden: `cd test/golden/golden-renderer && TETO_DIR=$ROOT/test/golden/teto-english WORLDLINE_RENDERER=<1|2> ./run.sh phrase <out.wav> test/golden/one-note.ustx`

## 0. คำตอบ (สั้นและมีตัวเลขครบ)

| คำถาม | คำตอบ |
|---|---|
| **511 samples มาจากไหน** | **hop − 1 = 512 − 1** = ผลต่างระหว่าง R2 กับ R1 **ของ reference เอง** |
| สูตรที่อธิบายได้เป๊ะ (F = 142 frames) | R2 = **F · hop** = 142×512 = **72,704** · R1 = **(F−1) · hop + 1** = 141×512+1 = **72,193** ⇒ ต่าง **511** |
| engine R2 ของเราหด/ตัด tail ไหม | **ไม่** — mel dump ให้ F = 142 ⇒ `142×512 = 72,704` = **ตรงกับ reference R2 ทุก sample** |
| แล้ว tail หายที่ไหน | **ตอน chunk trim**: `trim_chunk_audio` ตัดที่ **สิ้นสุด span ของ chunk** (1500 ms) ⇒ chunk = 72,132 ⇒ ทิ้ง tail **572 samples** (r2) / **61** (r1) |
| ทำไมของเรา r2 == r1 (66,150 ทั้งคู่) | เพราะ trim เป็น **plan-derived** (ไม่ขึ้นกับ engine) ⇒ ความต่าง 511 ของ engine ถูกตัดทิ้งทั้งสองข้าง |
| golden demo ต่างจากเราอย่างไร | golden demo = **phrase − leading (5,982)** ⇒ r1 = 66,211 · r2 = 66,722 ⇒ **เราขาด 61 / 572 samples ตามลำดับ** |
| ควรแก้ไหม | **ควร** (แก้จุดเดียว ~1 บรรทัด) แต่ `pipeline.rs` อยู่นอก scope T20 ⇒ §4 มีข้อเสนอ + คาดการณ์ **±0 samples** |

## 1. หลักฐานดิบ

### 1.1 golden **phrase mode** (one-note.ustx, mono, apple-to-apple)

```
WORLDLINE_RENDERER=1  phrase 0: renderer=WORLDLINE-R  samples=72193 (1637.0 ms) posMs=0.0 leadingMs=135.7 estLen=1635.7
WORLDLINE_RENDERER=2  phrase 0: renderer=WORLDLINE-R2 samples=72704 (1648.6 ms) posMs=0.0 leadingMs=135.7 estLen=1635.7
```

⇒ reference **R2 ยาวกว่า R1 = 72,704 − 72,193 = 511 samples** (ตรงกับที่ Lead วัดใน demo: 66,722 − 66,211 = 511)

### 1.2 ของเรา (one-note)

| อะไร | ค่า | ที่มา |
|---|---|---|
| engine R2 (ก่อน trim) | **72,704 samples = 142 frames × 512** | `WORLD_DUMP_MEL_FILE` → `total=142` |
| chunk หลัง trim (ที่ pipeline เก็บ) | **72,132 samples** (1636 ms) | log: `phrase @0 ms chunk 0-1 (own 0-1): 72132 samples` |
| WAV สุดท้ายที่ mix ได้ | **66,150 samples = 1500.0 ms** | `/tmp/t20-note-r2.wav` |
| WAV r1 ของเรา | **66,150 samples** (เท่ากันเป๊ะ) | `/tmp/t20-note-r1.wav` |

### 1.3 เทียบปลายทาง

| คู่ | reference (demo stereo) | ของเรา | ต่าง |
|---|---|---|---|
| one-note r1 | 66,211 | 66,150 | 61 |
| one-note r2 | 66,722 | 66,150 | **572** |

## 2. อธิบาย 511 ให้ครบ (และทำไมไม่ใช่ความผิดของ R2)

โค้ด reference กำหนดจำนวน frame ของ phrase ไว้ที่ `Render/Worldline.cs:660-662`:

```csharp
int totalFrames = segments.Max(s => s.p4) + 1;     // = F
```

ของเรา port มาตรงตัวเดียวกัน (`worldline-plugin/src/r2.rs:233`): `max(p4) + 1` ⇒ **F เท่ากัน**

- **R2** ใช้ feature timeline เต็ม F frames แล้ว vocoder คืน `F × hop` samples
  (C#: `Skip(leftPadding*512).Take(totalFrames*512)`; เรา: `audio.truncate(total_frames * hop)`)
  ⇒ 142 × 512 = **72,704**
- **R1** (`WorldSynthesis` บน F frames) คืน **(F−1) × hop + 1** samples = 141×512 + 1 = **72,193**
  (เป็นพฤติกรรมเดิมของ WORLD/synthesis ที่มี sample เดียวส่วนเกินเมื่อเทียบกับ frame grid)
- ⇒ **R2 − R1 = hop − 1 = 511** เสมอ ไม่ว่า F เท่าไร — ตรวจแล้วว่า **ทั้งสองสูตร reproduce ตัวเลข golden ได้ตรงเป๊ะ**

⇒ 511 ไม่ใช่ "tail ที่ R2 ทำหล่น" แต่เป็น **ส่วนต่างเชิงโครงสร้าง hop ของ R2 เทียบกับ R1**

## 3. ของเราขาดเพราะอะไร (จุดเดียว)

`native/tools/synth-cli/src/pipeline.rs:344-369` (`trim_chunk_audio`) ตัด buffer ที่ **สิ้นสุด span ของ chunk**:

```rust
let end_i = ((chunk.end_ms - audio_start_ms) * sr) as usize;
let samples = rendered[start_i..end_i.min(rendered.len())].to_vec();
```

แทนค่าของ one-note: `audio_start_ms = position_ms − leading_ms = −135.646 ms`, `chunk.end_ms = 1500`

```
end_i = (1500 − (−135.646)) × 44.1 = 72,132        // แต่ renderer คืน 72,704
⇒ tail ที่ถูกทิ้ง = 72,704 − 72,132 = 572 samples = 12.97 ms
```

golden demo ทำตรงข้าม: เก็บ render เต็ม (72,704) แล้วเลื่อนตาม leading ⇒ 72,704 − 5,982 = **66,722**;
ของเรา = chunk (72,132) − 5,982 = **66,150** ⇒ ผลต่าง = 572 = tail ที่ trim ทิ้งนั่นเอง
(ทำนองเดียวกัน r1: ทิ้ง 61 ของ 72,193 → 66,211)

## 4. ผลกระทบ + ข้อเสนอ (ยังไม่แก้ — รอเจ้าของ pipeline.rs)

**tail ที่หายเป็น fade-out ไม่ใช่ความเงียบ**: วัดจาก golden phrase mode

| engine | tail ที่เราทิ้ง | peak | rms | ปลาย |
|---|---|---|---|---|
| r2 | 572 samples (12.97 ms) | −48.7 dBFS | −56.0 dBFS | ลงท้ายที่ 0.0 |
| r1 | 61 samples (1.38 ms) | −41.0 dBFS | −46.3 dBFS | ลงท้ายที่ 0.0 |

ของเราเวลานี้ **จบแบบตัด** (sample สุดท้าย 6e−05 ไม่ใช่ 0) ⇒ เสียงหายไป 13 ms และ envelope ไม่จบที่ศูนย์ตาม reference

**ผลถ้าแก้** (คาดการณ์, ±0 samples):
- one-note r2 → 66,150 + 572 = **66,722 = golden** · r1 → 66,150 + 61 = **66,211 = golden**
- sakura r2 → +tail ของ phrase สุดท้าย (Lead วัดต่างไว้ 498) · scale-demo → +390 (คาดว่าตรงกันด้วยกลไกเดียวกัน)
- r2 จะ **ไม่เท่ากับ r1 อีก** (สะท้อน engine จริง — ตอนนี้ความต่าง 511 ถูกกลบด้วย trim)
- ตัวชี้วัด: duration +13 ms (r2 note) · silence% เพิ่มไม่เกิน 1–2 หน้าต่าง 10 ms (≤0.3% ของ 3,500 ms) · envelope ได้ fade-out ตาม reference

**ข้อเสนอ (จุดเดียว ~1 บรรทัด + เงื่อนไข)** ใน `trim_chunk_audio`:
ให้ **chunk สุดท้ายของ phrase** (`chunk.own_range.end == phrase.input.phonemes.len()`) ไม่ cap ที่ `chunk.end_ms`
แต่ใช้ `rendered.len()` แทน ⇒ เก็บ tail ของ renderer ไว้; chunk อื่นต้อง cap ที่ `chunk.end_ms` เหมือนเดิมเพื่อไม่ให้ซ้อนกับ chunk ถัดไป
พร้อมเทสต์กัน regression ที่เสนอ:
1. one-note: `mix_len == phrase_render_len − leading_samples` (66,722 สำหรับ r2 · 66,211 สำหรับ r1)
2. r2 − r1 = 511 samples สำหรับ one-note (ตอนนี้ = 0)
3. เทสต์เดิมของ chunk boundary (multi-chunk) ต้องยังผ่าน — เฉพาะ chunk สุดท้ายที่เปลี่ยนพฤติกรรม

## 5. ไฟล์ที่แตะ / สถานะ

เอกสารนี้ไฟล์เดียว — **ไม่มีการแก้โค้ดในงานนี้** (จุดแก้จริงคือ `native/tools/synth-cli/src/pipeline.rs` ซึ่ง T20 ไม่มีสิทธิ์แตะ; `phonemizer/**`, `runtime/src/mixer.rs`, `test/**` ก็ห้ามแตะ)
artefacts ที่ใช้ยืนยัน: `/tmp/t20-gp-r1-note.wav` (72,193) · `/tmp/t20-gp-r2-note.wav` (72,704) · `/tmp/t20-note-dump.bin` · `/tmp/t20-note-r1.wav`, `/tmp/t20-note-r2.wav` (66,150)

---

## 6. ผลหลังแก้ (T21 — ทำจริงแล้ว)

> task-21 แก้ตามข้อเสนอ §4 แล้ว: `native/tools/synth-cli/src/pipeline.rs::trim_chunk_audio`
> — chunk สุดท้ายของ phrase ใช้ `rendered.len()` แทนการ cap ที่ `chunk.end_ms`; chunk อื่น cap เหมือนเดิม

### 6.1 one-note — ตรง golden ทั้งสอง engine (ไม่ต้องแก้อะไรเพิ่ม)

| engine | ก่อนแก้ | หลังแก้ | golden | ต่างหลังแก้ |
|---|---|---|---|---|
| r1 (universal) | 66,150 | **66,211** | 66,211 | **0** ✅ |
| r2 (universal) | 66,150 | **66,722** | 66,722 | **0** ✅ |
| r2 − r1 | 0 (ความต่าง 511 ถูกกลบ) | **511** ✅ | 511 | — |

คำสั่ง: `synth-cli render --project test/golden/one-note.ustx --voicebank test/golden/teto-english/library --so ... --engine r1-rs|r2 --phonemizer universal --out ...`

### 6.2 ab_metrics: golden VALID vs ของเราหลังแก้

```text
golden r1 note   1501 ms  rms -15.4 dB  onset 50 ms  silence 0.7%  peak 0.427  clip 0
ours   r1 note   1501 ms  rms -12.4 dB  onset 50 ms  silence 0.7%  peak 0.604  clip 0   (+3.01 dB = pan law)
golden r2 note   1513 ms  rms -12.2 dB  onset 50 ms  silence 1.3%  peak 0.558  clip 0
ours   r2 note   1513 ms  rms  -9.2 dB  onset 50 ms  silence 1.3%  peak 0.789  clip 0   (+3.01 dB = pan law)
```

⇒ duration/onset/silence% **ตรงกันทุกช่อง** · rms/peak ต่าง +3.01 dB ซึ่งเป็น pan law ของ harness (task-16) ไม่ใช่ของเรา

### 6.3 ผลข้างเคียงบนเพลงอื่น

| เพลง (r2, DEFAULT phonemizer) | ก่อนแก้ | หลังแก้ | golden | หมายเหตุ |
|---|---|---|---|---|
| sakura | 154,349 | **154,847** | **154,847** | **ตรง golden เป๊ะ** (+498 = tail ของ phrase สุดท้าย) · silence% 10.1 → 10.0 |
| scale-demo | 639,449 | 640,040 | 639,839 | +201 (ก่อนแก้ −390) · **corr@0 = −0.12 ทั้งก่อนและหลังแก้ ⇒ ความต่างนี้มีอยู่เดิม ไม่ได้เกิดจาก fix นี้** (เปิดเป็นข้อสังเกต) |

sakura: prefix 154,349 samples ต่างจากไฟล์ก่อนแก้ ≤ 4.18e-3 (−47.6 dB) เพราะ tail ของ phrase 1/2 ไปคาบกับช่วงของ phrase ถัดไปแล้วเข้า crossfade — ระดับเท่ากับ fade-out ของ r2 ⇒ ยอมรับได้
scale-demo: prefix ก่อนแก้ **bit-identical** (max|diff| = 0) ⇒ fix นี้มีผลเฉพาะการต่อ tail จริง

### 6.4 เทสต์

| test | ผล |
|---|---|
| `synth-cli/tests/tail_parity.rs::last_chunk_keeps_the_renderer_tail` (ใหม่, รันเสมอ) | ✅ ok |
| `...::one_note_r2_is_511_samples_longer_than_r1` (ใหม่, ignored) | ✅ ok — `r1 66211 · r2 66722 · diff 511` |
| `tests/note_onset.rs` (T9, ignored 3 ตัว) | ✅ 3 passed (ไม่ถดถอย) |
| `cargo test -p synth-cli` | ✅ exit 0 (9+4+3+4+2+1 passed · 3+1 ignored) |
| `cargo test -p runtime` | ✅ 57 passed |

### 6.5 ไฟล์ที่แตะใน T21

| ไฟล์ | สถานะ |
|---|---|
| `native/tools/synth-cli/src/pipeline.rs` | `trim_chunk_audio`: last chunk ใช้ `rendered.len()` (+ comment อธิบาย) |
| `native/tools/synth-cli/tests/tail_parity.rs` | เทสต์ใหม่ 2 ตัว (1 รันเสมอ + 1 ignored E2E) |
| `docs/planning/r2-length-parity.md` | §6 นี้ |

ไม่ได้แตะ `native/crates/phonemizer/**`, `runtime/src/mixer.rs`, `test/golden/golden-renderer/**` (ตามข้อห้าม)

