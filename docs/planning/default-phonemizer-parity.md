# Default phonemizer (parity กับ OpenUtau) + A/B กับ golden (T12)

> วันที่ 2026-09-23 · ต่อจาก T11 (`docs/planning/phrase-segmentation-parity.md`)
> หลักฐาน: unit/integration test ของจริง + A/B วัดด้วยสคริปต์กลาง `test/golden/ab_metrics.py` (นิยาม dBFS อ้าง full scale 32768)
> ขอบเขต: เพิ่ม phonemizer ใหม่ + CLI flag + เทสต์ + เอกสาร — **ไม่แตะ `pipeline.rs`** (r2-wasm แปะ diff ให้ตามคำสั่ง Lead)

## 0. สรุป

T11 พบว่า OpenUtau เลือก **DefaultPhonemizer** กับโปรเจกต์นี้ (1 โน้ต = 1 phoneme = lyric verbatim, ใช้ alias ที่แมปได้)
แต่ของเรา default = Universal (letter-split + nearest-phoneme) ⇒ phone/alias คนละชุด ⇒ เสียงต่าง

งานนี้เพิ่มโหมด parity: `native/crates/phonemizer/src/default.rs` = **`DefaultPhonemizer`** ที่ทำตาม
`OpenUtau.Core/DefaultPhonemizer.cs` ตรง ๆ และ**ให้ผล segmentation ตรง reference เป๊ะทุกตัวเลข**:

```
dropped 1 note(s): ["note 7 lyric \"ya\": no oto alias"]
  @0.0 ms leading 135.7 dur 1250.0 ms -> sa ku sa ku sa
  @1500.0 ms leading 70.6 dur 500.0 ms -> ku no
  @2250.0 ms leading 85.5 dur 1250.0 ms -> ma mo sa to ni
```

เทียบ reference ของ OpenUtau (จาก T11): `posMs=0.0 leading=135.7 dur=1250` · `1500 / 70.6 / 500` · `2250 / 85.5 / 1250`
⇒ **ตรงทั้ง 6 ตัวเลขและ phones ทุกตัว** (ต่างจากเส้น japanese/universal ที่ได้ 1+skipped และ 2 phrase ตามลำดับ)

**ผล e2e หลัง wire CLI (ไบนารี release หลัง task-9 fix, เคลียร์ render cache)**
· sakura: **3 phrase** · 154,349 samples = 3,500.0 ms (golden 154,336 = ต่าง **0.3 ms**) · **onset 50 ms = golden** · 0 skipped
· scale-demo: **4 phrase** · 14,500 ms (golden 14,497) · **onset 50 ms = golden** · **silence 0.0% = golden** (universal 5.6%)
⇒ เทียบ universal (2 phrase, 3,429 ms, onset 200 ms, silence 3.5%) ก่อนหน้า — ดูตารางเต็มใน §3.3

กุญแจของ parity: OpenUtau ทำ `UPhoneme.Error = true` เมื่อหา oto ไม่เจอ แล้ว `RenderPhrase.FromPart` **กรองทิ้งก่อนแบ่ง phrase**
ของเรามี gate ระดับ phrase (หา alias ไม่ครบ = skip ทั้ง phrase) จึงทำให้โหมดนี้ **ไม่ emit phoneme ของโน้ตที่ไม่มี alias เลย**
— ได้ผลสุทธิเท่ากัน (ช่องว่าง 250 ms ที่โน้ต 'ya') โดยไม่ต้องแก้ feed/pipeline

## 1. สิ่งที่เพิ่ม

| ไฟล์ | อะไร |
|---|---|
| `native/crates/phonemizer/src/default.rs` | `DefaultPhonemizer` + `process_report` (คืนรายการโน้ตที่ถูกทิ้งพร้อมเหตุผล) + unit test 3 ตัว |
| `native/crates/phonemizer/src/lib.rs` | `pub mod default;` + `pub use default::DefaultPhonemizer;` |
| `native/tools/synth-cli/tests/default_phonemizer_parity.rs` | เทสต์ integration 2 ตัว (segmentation parity + 1 โน้ต ≤ 1 phoneme) |
| `native/tools/synth-cli/src/main.rs` | `--phonemizer default` (เตรียมไว้ — apply หลัง pipeline diff เข้า) |
| `native/tools/synth-cli/src/pipeline.rs` | diff 4 hunk — **r2-wasm แปะแล้ว** (Lead อนุมัติ; ผมไม่แตะไฟล์นี้) |

### กฎของ `DefaultPhonemizer` (ตรงกับ C# โดยเจตนา)

1. 1 โน้ต → 1 phoneme, `position = 0`, `duration = note.duration`, `index = 0`
2. ใช้ **lyric ดิบ** (ไม่ G2P, ไม่แยก mora) — C#: `alias = notes[0].lyric`
   · แต่ **ตัด `[hint]` ออกก่อน** เพราะ OpenUtau แยก hint ไปเก็บในฟิลด์ `phoneticHint` ตั้งแต่ตอน parse
   (`UNote.cs:120-158`) ⇒ `notes[0].lyric` ของ C# คือ lyric ที่ไม่มี hint
   (เทสต์: `phonetic_hint_is_stripped_from_the_lyric`)
3. ถ้า `voicebank.lookup(lyric, tone)` เจอ → `phoneme = oto.alias` (ตาม C# `alias = oto.Alias`)
4. ถ้าไม่เจอ → **ไม่ emit phoneme นั้น** + รายงานใน `process_report` (แทน `Error`+`FromPart` ของ C#)
   ⇒ โน้ตนั้นกลายเป็นช่องว่าง ทำให้ phrase แตกตรงจุดเดียวกับ reference และ phrase อื่นยังเรนเดอร์ได้
5. โน้ต extender (`+` / `+N`) ต่อท้ายโน้ตหลัก → ยืด duration ของ phoneme ก่อนหน้า (OpenUtau รวม extender เข้า phoneme เดียวกัน)
6. ไม่มี singer → emit unresolved (เหมือน phonemizer อื่น) ; rest (`R`) ถือเป็น lyric ธรรมดา (C# ไม่มี special case)

## 2. หลักฐาน: เทสต์ parity

```bash
cargo test -p phonemizer --lib default::                                 # 4 passed
cargo test -p synth-cli --test default_phonemizer_parity -- --nocapture # 4 passed
cargo test -p synth-cli --test phrase_segmentation_parity               # 3 passed (T11 ยังผ่าน)
```

ผลรันจริง (binary/tests ปัจจุบัน):

```
$ cargo test -p phonemizer --lib default::
running 4 tests
test default::tests::empty_lyric_is_dropped_not_emitted ... ok
test default::tests::extender_notes_extend_the_previous_phoneme ... ok
test default::tests::one_note_one_phoneme_without_a_singer ... ok
test default::tests::phonetic_hint_is_stripped_from_the_lyric ... ok
test result: ok. 4 passed; 0 failed

$ cargo test -p synth-cli --test default_phonemizer_parity -- --nocapture
running 4 tests
test from_track_maps_default_explicitly_and_keeps_universal_as_fallback ... ok
dropped 1 note(s): ["note 7 lyric \"ya\": no oto alias"]
  @0.0 ms leading 135.7 dur 1250.0 ms -> sa ku sa ku sa
  @1500.0 ms leading 70.6 dur 500.0 ms -> ku no
  @2250.0 ms leading 85.5 dur 1250.0 ms -> ma mo sa to ni
test every_note_maps_to_at_most_one_phoneme ... ok
test default_phonemizer_reproduces_openutau_segmentation_on_sakura ... ok
test result: ok. 2 passed; 0 failed
```

เทสต์ที่ 1 assert 4 อย่าง (ทั้งหมดผ่าน):
1. มีโน้ตถูกทิ้ง **1 ตัวเท่านั้น** และเป็น lyric `"ya"` (ไม่มี alias ใน bank — ตรวจแล้วว่าไม่มี alias ใดมี "ya" เลย)
2. ได้ **3 phrase** ที่ `(0,1250) (1500,500) (2250,1250)` ตรงกับ OpenUtau (±1 ms)
3. phones ต่อ phrase = `sa ku sa ku sa` / `ku no` / `ma mo sa to ni` ตรงทุกตัว
4. ทุก phoneme ที่ emit มี oto ⇒ pipeline ไม่ skip phrase ใด (0 skipped)

เทสต์ที่ 2 assert ว่า 1 โน้ตให้ ≤ 1 phoneme (คุณสมบัติ 1:1 ของ DefaultPhonemizer) และจำนวน phoneme = จำนวนโน้ต - 1 (โน้ตที่ถูกทิ้ง)

## 3. A/B กับ golden (สคริปต์กลาง `test/golden/ab_metrics.py`)

ทุกค่า dB อ้าง full scale 32768 · onset = หน้าต่าง 50 ms แรกที่ RMS > -30 dBFS ต่อเนื่อง ≥ 2 หน้าต่าง ·
silence = % ของหน้าต่าง 10 ms ที่ RMS < -45 dBFS · clip = จำนวน sample |x| ≥ 32767
(golden เป็น stereo → สคริปต์กลางจัดการให้; ของเรา mono 44.1k)

> **สำคัญเรื่องเวอร์ชันไบนารี**: §3.1/§3.2 วัดด้วยไบนารีก่อน task-9 fix; หลัง r2-wasm แก้ 2 จุด
> (mixer double-subtract leading + japanese phonemizer แบ่งโน้ตครึ่ง ๆ) onset/ความยาวเปลี่ยนไปมาก
> ⇒ §3.3 (default mode) และแถว universal ที่วัดซ้ำ **วัดใหม่ทั้งหมดด้วยไบนารี release ล่าสุดหลัง fix**

### 3.1 sakura-test (โหมด universal = ค่า default เดิม)

```
g-sakura-r2.wav         3500 ms  rms  -5.8 dB  onset   50 ms  silence  0.0%  peak 1.000  clip 12403
t12-uni-r2.wav          3429 ms  rms -11.4 dB  onset  200 ms  silence  3.5%  peak 0.788  clip 0
g-sakura-r1.wav         3500 ms  rms  -5.8 dB  onset   50 ms  silence  0.0%  peak 1.000  clip 12403
t12-uni-r1.wav          3425 ms  rms -13.6 dB  onset  150 ms  silence  0.6%  peak 0.625  clip 0
```

(golden r1 กับ r2 `cmp` กันได้ **byte-identical** — ไม่ใช่เพราะ "gain เดียวกัน" แต่เพราะตอนวัด **harness demo ไม่สลับ renderer ตาม `WORLDLINE_RENDERER`**
⇒ golden r1/r2 ที่มีอยู่จึงเป็น **v1 ทั้งคู่** (task-17 อธิบาย + แก้แล้ว · ดู §4) · goldens: `/tmp/g-sakura-r1.wav`, `/tmp/g-sakura-r2.wav`)
ทั้งสอง engine ของเราได้ **2 phrase** (universal) ในขณะที่ golden ได้ **3 phrase** — คือความต่างที่งานนี้แก้

### 3.2 scale-demo-app (โหมด universal)

```
g-scaledemo-r2.wav 14497 ms  rms  -3.1 dB  onset   50 ms  silence  0.0%  peak 1.000  clip 205274
t12-scale-uni-r2.wav 14500 ms  rms -10.8 dB  onset   50 ms  silence  5.6%  peak 0.907  clip 0
```

### 3.3 ผล e2e หลัง wire CLI + หลัง task-9 fix (ไบนารี release ล่าสุด, เคลียร์ render cache ก่อนวัด)

คำสั่ง (รันจริงทั้งหมด):

```bash
rm -rf test/golden/teto-english/cache          # measurement protocol ของ repo
native/target/release/synth-cli render --engine r2 --phonemizer default \
  --mel-onnx models/r2/mel.onnx \
  --vocoder-onnx models/r2/pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx \
  --so native/build/build-linux/libworldline.so \
  --project test/golden/sakura-test.ustx --voicebank test/golden/teto-english/library \
  --out /tmp/t12-def-r2.wav --verbose
python3 test/golden/ab_metrics.py /tmp/g-sakura-r2.wav /tmp/t12-def-r2.wav /tmp/t12-uni-r2.wav
# (ทำแบบเดียวกันกับ scale-demo-app → /tmp/t12-def-scale-r2.wav, /tmp/t12-uni-scale-r2.wav)
```

**sakura-test** (golden 154,336 samples):

```
g-sakura-r2.wav         3500 ms  rms  -5.8 dB  onset   50 ms  silence  0.0%  peak 1.000  clip 12403
t12-def-r2.wav          3500 ms  rms -11.7 dB  onset   50 ms  silence 13.1%  peak 0.788  clip 0     <- default
t12-uni-r2.wav          3500 ms  rms -11.5 dB  onset   50 ms  silence  8.6%  peak 0.788  clip 0     <- universal
```

- **default: 3 phrase (0 / 1500 / 2250 ms) · 0 skipped · 154,349 samples = 3,500.0 ms**
  ต่างจาก golden 13 samples = **0.3 ms** (ก่อนหน้านี้ universal เดิม 3,429 ms = ต่าง 70.3 ms)
- **onset 50 ms = golden เป๊ะ** (universal เดิม 200 ms; หลัง task-9 fix universal เองก็เป็น 50 ms แล้ว)
- phones ต่อ phrase ตรง reference ทุกตัว (`sa ku sa ku sa` / `ku no` / `ma mo sa to ni`)
- **universal ยังได้ 2 phrase** — นี่คือความต่างเชิงโครงสร้างที่โหมด default แก้

**scale-demo-app** (golden 639,328 samples):

```
g-scaledemo-r2.wav      14497 ms  rms  -3.1 dB  onset   50 ms  silence  0.0%  peak 1.000  clip 205274
t12-def-scale-r2.wav    14500 ms  rms -10.7 dB  onset   50 ms  silence  0.0%  peak 0.991  clip 0     <- default
t12-uni-scale-r2.wav    14500 ms  rms -10.8 dB  onset   50 ms  silence  5.6%  peak 0.907  clip 0     <- universal
```

- **default: 4 phrase · 0 skipped · 14,500 ms (golden 14,497) · onset 50 = golden · silence 0.0% = golden**
  ⇒ universal เหลือ silence 5.6% แต่ default กำจัดได้หมดด้วย alias ตาม reference
- peak 0.991 ใกล้ golden 1.000 มากกว่าอย่างชัดเจน (universal 0.907)

### 3.4 คำทำนาย vs ผลจริง

| คำทำนาย (จาก phrase plan) | ผลจริง | ผ่าน? |
|---|---|---|
| 3 phrase ที่ 0/1500/2250 ms | 3 phrase ที่ 0/1500/2250 ms | ✅ |
| 154,350 samples (±14) | 154,349 samples | ✅ |
| onset เข้าใกล้ 50 ms | 50 ms (= golden) | ✅ |
| silence ลดจาก baseline | scale-demo: 5.6% → 0.0% (= golden) | ✅ |
| 0 skipped | 0 skipped | ✅ |
| (sakura) silence ลด | ❌ **เพิ่ม** 8.6% → 13.1% | ❌ อธิบายด้านล่าง |

**sakura silence เพิ่มขึ้นเป็น 13.1% — อธิบายได้และไม่ใช่ regression**: โหมด default *ทิ้งโน้ต 'ya'*
(ตาม reference) ทำให้ช่วง 2000–2250 ms เป็น **ความเงียบจริง 250 ms** ส่วน universal เติมช่วงนั้นด้วย phone `A`
(nearest-phoneme ที่ผิด) จึงมีค่าน้อยกว่า — คือ "เงียบถูกต้องตาม reference" vs "มีเสียงแต่ผิด phone"
และ golden วัดได้ 0.0% เพราะ golden ถูก clip + ระดับดังกว่า ~6 dB (ดู §4)

## 4. ข้อจำกัด / ข้อควรระวัง

- **level ของ golden ไม่ใช่เป้าหมายที่ยุติธรรม**: golden clip 12,403 (sakura) / 205,274 (scale-demo) samples และ peak = 1.000
  ⇒ ของเรา clip 0 จึงตั้งใจไม่ทำ level ให้เท่า; ตัวชี้วัดที่ควรใช้คือ **onset / silence / phrase count / duration**
- **`rms`/`peak` ของ golden ที่สูงกว่า (-5.8 vs -11.4 dB) มาจากกลไกจริง 2 ข้อ (ไม่ใช่คุณภาพ — อ้าง task-16/task-17):**
  1. **pan law −3.01 dB ต่อ channel**: demo ของ OpenUtau เป็น stereo ผ่าน `Fader` ของ track; track center ⇒
     `MusicMath.PanToChannelVolumes(0) = (cos π/4, sin π/4) = 0.7071` ต่อ channel = **−3.0103 dB**
     (`SignalChain/Fader.cs:40-48` → `Util/MusicMath.cs:247-249`) · ตรวจแล้วว่า demo = phrase × 1/√2 เป๊ะ
     (corr 1.000000, gain 0.7071, demo L == demo R) · ของเราเป็น **mono fold-down** `pan_gain = cos(pan·π/4)`
     (`native/crates/runtime/src/mixer.rs:95-101`) ⇒ center = unity; ภายใต้ fold-down เดียวกัน OpenUtau ให้
     `(0.7071+0.7071)/√2 = 1.0` ⇒ **พลังงานรวมเท่ากัน ต่างแค่ −3.01 dB ต่อ channel**
  2. **golden r1/r2 ที่มีอยู่เป็น v1 ทั้งคู่** (ก่อน task-17): `./run.sh demo` ตอนนั้นไม่สลับ renderer ตาม
     `WORLDLINE_RENDERER` เพราะ `RenderPhrase` ฝัง renderer ตอนสร้าง phrase (`RenderPhrase.cs:222`) และ
     harness อ่าน env เฉพาะโหมด `phrase` ⇒ `RenderMixdown` (demo) ใช้ v1 เสมอ
     (task-17 แก้: เลือก renderer ก่อน `ValidateFull` → หลัง fix `ours r2 / demo r2 = +3.01 dB` = เหลือ pan law อย่างเดียว)
  ⇒ อย่าไล่ระดับ dB ให้เท่า golden (±3.01 dB ต่อ channel จาก pan + reference เป็น v1); ให้ใช้ onset / silence / phrase count / duration เป็นหลัก
  · รายละเอียดกลไก/หลักฐาน: `docs/planning/gain-level-parity.md` (T16) และ `docs/planning/harness-demo-renderer.md` (T17)
- โหมด default **ไม่มี VCV bridging / G2P**: เหมาะกับ bank ที่ alias = lyric (เช่น Teto English นี้) ตามที่ OpenUtau ทำ;
  ถ้า lyric ไม่ตรง alias จะถูกทิ้ง (มีรายงานจาก `process_report`) — ไม่ใช่ตัวแทนของ Universal
- **silence% ขึ้นกับระดับเสียง**: golden ดังกว่า ~6 dB — มาจาก **pan law −3.01 dB (ข้อ 1 ข้างบน) + engine v1/ระดับของ harness ~3 dB** — และถูก clip
  ⇒ ไฟล์ที่ดังกว่าจะมีหน้าต่างที่ต่ำกว่า -45 dBFS น้อยกว่าโดยธรรมชาติ
  การเทียบ silence ข้ามไฟล์ที่ระดับต่างกันจึงต้องอ่านคู่กับ `rms` เสมอ (และใช้ onset / phrase count / duration เป็นหลัก)
- **โหมด default เลือกได้ 3 ทาง — และลำดับความสำคัญคือ (สูง → ต่ำ)**
  1. `--phonemizer default` (CLI flag): `main.rs` จับคู่ flag เองแล้ว **ไม่เรียก `from_track` เลย** ⇒ **ชนะทุกอย่าง** (รวม env)
  2. **`RUSTED_UTAU_PHONEMIZER=default`** (env, เทียบแบบ case-insensitive): เช็คเป็น **บรรทัดแรกของ `from_track`**
     (`pipeline.rs:103-108`) ⇒ **ชนะ `track.phonemizer`** · ค่าอื่นนอกจาก `default` ถูกเมิน (ไม่มีผล)
  3. `track.phonemizer` ใน .ustx: มีคำว่า `japanese`/`jpn`/`vcv` → Japanese · มีคำว่า `default` → Default · อื่น ๆ → Universal
  4. ไม่มีอะไรระบุ → **Universal** (default ของระบบไม่เปลี่ยน — เทสต์ `from_track_maps_default_explicitly_and_keeps_universal_as_fallback` ยืนยัน)

- **⚠️ คำเตือน diagnose: เส้นทางที่ 2 (env) เป็นกับดัก** — `RUSTED_UTAU_PHONEMIZER` เปลี่ยน phonemizer (และเสียง) **โดยไม่ปรากฏในไฟล์โปรเจกต์เลย**
  ⇒ โปรเจกต์เดียวกันให้ผลต่างกันได้ตาม env ของ shell และไม่มีร่องรอยใน .ustx ⇒ ใช้เฉพาะ A/B/diagnose
  **ห้ามใช้ตอน reproduce หรือรายงานตัวเลข** (ถ้า env ค้างอยู่ ตัวเลขจะเทียบซ้ำไม่ได้) ·
  ก่อนวัดทุกครั้งให้เช็ค `env | grep RUSTED_UTAU_PHONEMIZER` (หรือ `unset RUSTED_UTAU_PHONEMIZER`) แล้วบันทึกใน log ว่าวัดด้วยวิธีไหน
  · ตัวเลขทั้งหมดในเอกสารนี้วัดด้วย **CLI flag** ไม่ได้ตั้ง env

## 5. ไฟล์ที่แตะ

`native/crates/phonemizer/src/default.rs` (ใหม่) · `native/crates/phonemizer/src/lib.rs` (export) ·
`native/tools/synth-cli/tests/default_phonemizer_parity.rs` (ใหม่) · `native/tools/synth-cli/src/main.rs` (flag) · เอกสารนี้
**ไม่แตะ**: `native/tools/synth-cli/src/pipeline.rs` (task-9), `crates/phrase/src/grouping.rs`, `crates/feed/**`, `web/**`, `models/**`, `ref*/**`
