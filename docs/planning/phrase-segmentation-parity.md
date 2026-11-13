# การเชื่อม/แบ่ง phrase + phonemizer: ของเรา vs OpenUtau (T11)

> วันที่ 2026-09-23 · โจทย์ผู้ใช้: "การเชื่อม phrase ด้วยไหม"
> หลักฐาน: (ก) อ่านโค้ด OpenUtau ตัวจริงใน `ref(openutau+openutau mobile)/core`
> (ข) รัน `synth-cli render` ของเราทั้ง `--phonemizer japanese` และ `universal` กับโปรเจกต์จริง
> (ค) parse `voice/oto.ini` ของ bank เอง (ไม่เชื่อเอกสาร) · (ง) เทสต์ `tests/phrase_segmentation_parity.rs`
> ขอบเขต: ไม่แก้ behavior ของ phonemizer/feed — งานนี้คือ "อธิบายให้ชัด + เทสต์ + ข้อเสนอ"

## 0. สรุป (อ่านอันนี้ก่อน)

**กฎการแบ่ง phrase ของเราเหมือน OpenUtau เป๊ะ** — `native/crates/phrase/src/grouping.rs` เป็น port ตรงของ
`OpenUtau.Core/Render/RenderPhrase.cs` `FromPart` (แยกเมื่อ `phonemes[i-1].End != phonemes[i].position`)

**ที่ทำให้ผลลัพธ์ต่างจาก reference มี 2 อย่าง — ไม่ใช่กติกาการแบ่ง:**

| # | สาเหตุ | OpenUtau | ของเรา |
|---|---|---|---|
| 1 | **นโยบาย error ต่อ phoneme** | ตัด phoneme ที่หา oto ไม่ได้ออก (`Error = true` → `Where(!phoneme.Error)`) **ก่อน** แบ่ง phrase ⇒ เกิดช่องว่าง ⇒ ได้ phrase เพิ่ม | ไม่มี error flag ต่อ phoneme — แบ่ง phrase จาก phoneme ทั้งหมด แล้วค่อยกรองที่ระดับ **phrase** ⇒ ถ้ามี phoneme เดียวหา alias ไม่ได้ = **skip ทั้ง phrase** |
| 2 | **phonemizer ที่ถูกเลือก** | track มี `phonemizer: ''` ⇒ `UTrack.AfterLoad` fallback เป็น **DefaultPhonemizer** (lyric = phoneme, ถ้า map oto ได้ก็ใช้ alias นั้น) | default = **Universal** (IPA adapter + nearest-phoneme) · `--phonemizer japanese` = VCV candidate pairs |

**สาเหตุราก (root cause) ที่ 'ya' หาย**: bank นี้ (`test/golden/teto-english/library`) **ไม่มี alias `ya`**
— parse `voice/oto.ini` (2681 บรรทัด, 1701 alias, รูปแบบ `wavfile=alias,params`) แล้วเช็ค:

| lyric | มี alias ตรงตัว? |
|---|---|
| sa, ku, no, ma, mo, to, ni | ✅ มี |
| **ya** | ❌ **ไม่มี** (ไม่มี variant ใด ๆ ที่ลงท้าย/ขึ้นต้นด้วย ya) |

⇒ OpenUtau: 'ya' → `TryGetMappedOto` fail → `UPhoneme.Error = true` ("Oto not found") → `FromPart` ทิ้งโน้ตนั้น
⇒ เหลือ phoneme ที่ `no.end(2000) != ma.position(2250)` ⇒ **3 phrase** (0/1250 · 1500/500 · 2250/1250) ตรงกับที่ Lead วัด
⇒ ของเรา (`--phonemizer japanese`): phoneme ของ 'ya' ก็ไม่มี alias เหมือนกัน แต่เราไปตายที่ gate ระดับ phrase
⇒ **1 phrase + 1 skipped**

**ชื่อ alias ที่หายจริง ๆ = `y` ไม่ใช่ `ya`** (ดูตาราง per-phoneme ใน §2): phonemizer ญี่ปุ่นของเราแยก mora 'ya'
เป็น onset `y` + vowel `a`; alias ที่ลอง (`o y`, `* y`, `y`, `- y`) ไม่มีใน bank → fallback เป็น raw `y` → ไม่มี oto

## 1. OpenUtau เลือก phonemizer อะไรให้ bank นี้ และแมป lyric → alias อย่างไร

1. **bank ไม่มี `character.yaml`** ⇒ ไม่มี phonemizer ประกาศมาที่ตัวนักร้อง; โปรเจกต์ระบุ `track.phonemizer: ''`
2. `UTrack.AfterLoad` (`Ustx/UTrack.cs:211-223`): `PhonemizerFactory.Get(phonemizer)` กับค่าว่างไม่ได้ factory
   ⇒ บรรทัด 222: `Phonemizer = PhonemizerFactory.Get(typeof(DefaultPhonemizer)).Create()`
3. **DefaultPhonemizer** (`OpenUtau.Core/DefaultPhonemizer.cs:14-33`):
   `string alias = notes[0].lyric;` แล้ว `if (singer.TryGetMappedOto(lyric + alt, tone + shift, color, out var oto)) alias = oto.Alias;`
   ⇒ **1 โน้ต = 1 phoneme** และ phoneme คือ **alias ที่แมปได้จริง** (ถ้าแมปไม่ได้ก็เป็น lyric ดิบ)
4. **การแมป alias** อยู่ที่ `Classic/ClassicSinger.cs:171-186` → `Voicebank` alias map
5. **phoneme ที่หา oto ไม่ได้กลายเป็น error**: `Ustx/UPhoneme.cs:98-103`
   (`Error = true; ErrorException ??= new Exception($"Oto not found for \"{phoneme}\".")`)
6. **phrase ถูกสร้างหลังกรอง error**: `Render/RenderPhrase.cs:522-543`
   `part.phonemes.Where(phoneme => !phoneme.Error)` แล้วค่อยตัดเมื่อ `phonemes[i-1].End != phonemes[i].position`

`presamp.ini` มีอยู่ใน bank แต่ **ไม่ถูกใช้โดยเส้น Classic/DefaultPhonemizer** — alias ถูกใช้ตามที่อยู่ใน `oto.ini` ตรง ๆ
(ในเครื่องนี้ alias ตรงกับ lyric พอดีสำหรับ sa/ku/no/ma/mo/to/ni)

## 2. ทำไมเส้นทาง japanese ของเรา "ข้าม 1 phrase"

หลักฐานคำสั่งจริง — รัน `synth-cli` 2 ครั้ง: binary เดิม (mtime 10:50) และ binary ที่ rebuild ล่าสุด
(mtime 15:02 หลัง task-9) ได้ผลเหมือนกันทุกตัวอักษร (ดูท้าย §4):

```bash
native/target/release/synth-cli render --engine r1-rs --phonemizer japanese \
  --so native/build/build-linux/libworldline.so \
  --project test/golden/sakura-test.ustx --voicebank test/golden/teto-english/library \
  --out /tmp/t11-japanese.wav --verbose
```

ผลจริง:

```
project: Sakura | track 0 ('T') | phonemizer: Japanese
  phrase @0 ms chunk 0-10 (own 0-10): 55157 samples (1251 ms) (cached)
skipped: part 'P' phrase @1500 ms: render failed — partial oto mapping (15 of 16 phonemes) (k u u n - o y - a m - a m - o s - a t - o n i)
rendered 1 phrase(s), 1 skipped
samples: 55157 (1250.7 ms @ 44100 Hz), peak: 1.0047
```

กลไกในโค้ดของเรา:
- `native/crates/phonemizer/src/japanese.rs:305-321` — head ของแต่ละ mora สร้าง candidate alias เป็น
  `{tail vowel} {symbol}`, `* {symbol}`, `{symbol}`, `- {symbol}` แล้ว `resolve()` เก็บ **candidate แรกที่มีใน oto**
  (ถ้าไม่มีเลยจะ fallback เป็น raw symbol)
- `native/tools/synth-cli/src/pipeline.rs:380-427` — หลังแบ่ง phrase แล้ว มี gate:
  ถ้า `sample_based.is_none()` (ไม่แมปเลย) หรือ `sample_based.oto.len() != phonemes.len()` (แมปไม่ครบ)
  ⇒ `skipped.push("... partial oto mapping (N of M) ...")` และ **ไม่ render phrase นั้น**

per-phoneme table (จากเทสต์ `phrase_segmentation_parity`, `--nocapture`):

ตารางเต็มอยู่ใน output ของเทสต์ (26 phoneme); แถวที่สำคัญคือจุดที่พัง:

| idx | note | lyric | pos (ms) | dur | raw | resolved alias | มี oto? |
|---|---|---|---|---|---|---|---|
| 0 | 0 | sa | 0.0 | 125 | s | `s` | ✅ |
| 1 | 0 | sa | 125.0 | 125 | a | `- a` | ✅ |
| 2 | 1 | ku | 250.0 | 125 | k | `k` | ✅ |
| 4 | 2 | sa | 500.0 | 125 | s | `u s` | ✅ |
| 10 | 5 | ku | 1500.0 | 125 | k | `k` | ✅ |
| 12 | 6 | no | 1750.0 | 125 | n | `u n` | ✅ |
| 13 | 6 | no | 1875.0 | 125 | o | `- o` | ✅ |
| **14** | **7** | **ya** | **2000.0** | **125** | **y** | **`y` (fallback)** | ❌ **ไม่มี oto** |
| 15 | 7 | ya | 2125.0 | 125 | a | `- a` | ✅ |
| 16 | 8 | ma | 2250.0 | 125 | m | `m` | ✅ |
| 25 | 12 | ni | 3375.0 | 125 | i | `i` | ✅ |

**alias ที่หาไม่เจอ = `y`** (ไม่ใช่ `ya`): `JapaneseVcvPhonemizer` แยก mora 'ya' เป็น onset `y` + vowel `a`
(`japanese.rs:300-325`) แล้วลอง candidate ตามลำดับ `{tail} {symbol}` → `* {symbol}` → `{symbol}` → `- {symbol}`
ซึ่งก็คือ `o y`, `* y`, `y`, `- y` — **ไม่มีตัวไหนอยู่ใน oto.ini** ⇒ `resolve()` fallback เป็น raw `y`
⇒ phoneme #14 ไม่มี oto ⇒ phrase ที่ 2 (@1500 ms) แมปได้ 15/16 ⇒ gate ที่ `pipeline.rs:417-427` **skip ทั้ง phrase**

เทียบกับเส้น universal (phoneme เดียวต่อโน้ต): 'ya' ถูกแมปเป็น **`A`** (nearest-phoneme) จึงเรนเดอร์ได้ —
แต่เป็น *คนละ phone* กับ reference ที่ **ทิ้ง** โน้ตนั้น (ไม่ใช่คุณภาพเท่ากัน แค่ไม่ถูก skip)

## 3. ตารางเทียบการตัด phrase ทีละ phrase (sakura-test.ustx, bpm 120, 240 ticks = 250 ms)

| # | OpenUtau (reference) position/dur/phones | ของเรา Universal | ของเรา Japanese |
|---|---|---|---|
| 1 | @0 ms · dur 1250 · sa ku sa ku sa | @0 ms · dur ~1382 ms (chunk 0-5, 60,929 samples) | @0 ms · dur ~1251 ms (55,157 samples) |
| 2 | @1500 ms · dur 500 · ku no | รวมอยู่ใน phrase 2 | **SKIPPED** (15/16 phonemes) |
| 3 | @2250 ms · dur 1250 · ma mo sa to ni | @1500 ms · dur ~2067 ms (chunk 0-8, 91,137 samples) | — (อยู่ใน phrase ที่ถูก skip) |
| รวม | 3 phrase · ~3000 ms span (มีช่อง 250 ms ที่ 'ya') | **2 phrase** · 151,059 samples (3425 ms) | **1 phrase + 1 skipped** · 55,157 samples (1251 ms) |

**ข้อสรุปของตาราง**: จำนวน phrase ต่างกันเพราะ (1) OpenUtau ทิ้งโน้ต 'ya' (ไม่มี alias) ทำให้เกิดช่องว่าง
(2) เราในเส้น Universal แมป 'ya' ได้ (nearest-phoneme) จึงไม่เกิดช่องว่าง → ได้ 2 phrase
⇒ ถ้าเอา phoneme ของเส้น japanese มาตัดตัวที่ไม่มี oto ออกก่อน แล้วจัดกลุ่มใหม่ จะได้ 3 phrase ที่ตำแหน่ง/ความยาว
**ตรงกับ reference เป๊ะ** — เทสต์ `openutau_segmentation_is_our_rule_plus_error_dropping` ยืนยันข้อนี้

## 4. เทสต์กัน regression

ไฟล์ใหม่: `native/tools/synth-cli/tests/phrase_segmentation_parity.rs` (ไม่แตะ pipeline.rs ของ task-9)

```bash
cargo test -p synth-cli --test phrase_segmentation_parity -- --nocapture
```

assert 3 ข้อ:
1. `universal_is_the_recommended_path_and_skips_nothing` — เส้น universal: **0 phrase ถูก skip**, ทุก phoneme มี oto, ได้ 2 phrase
2. `japanese_path_pins_the_missing_oto_alias` — เส้น japanese: มี phoneme ที่ไม่มี oto **1 ตัวเท่านั้น** และนั่นคือเหตุที่ทำให้ 1 phrase ถูก skip (ถ้ามีการแก้ alias ทีหลัง เทสต์นี้จะ fail ให้อัปเดตเอกสาร)
3. `openutau_segmentation_is_our_rule_plus_error_dropping` — ตัด phoneme ที่ไม่มี oto ออกแล้วจัดกลุ่มใหม่ = 3 phrase ที่ (0,1250) (1500,500) (2250,1250) ตรงกับ reference

ผลรันจริง (`cargo test -p synth-cli --test phrase_segmentation_parity -- --nocapture`, 2026-09-23):

```
test universal_is_the_recommended_path_and_skips_nothing ... ok
test japanese_path_pins_the_missing_oto_alias ... ok
test openutau_segmentation_is_our_rule_plus_error_dropping ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

เทสต์ 3 พิมพ์ผลเทียบ reference:

```
reference model: 1 note(s) without any oto alias -> 2 phonemes dropped, 3 phrases left
  @0.0 ms leading 0.7 (reference 135.7) dur 1250.0 ms (10 phonemes)
  @1500.0 ms leading 78.1 (reference 70.6) dur 500.0 ms (4 phonemes)
  @2250.0 ms leading 0.3 (reference 85.5) dur 1250.0 ms (10 phonemes)
```

⇒ **ตำแหน่ง/ความยาว phrase ตรงกับ OpenUtau ทั้ง 3 phrase** ⇒ กติกาการแบ่งของเรา = ของ reference จริง ๆ;
ที่เหลือต่างคือ `leading` ซึ่งมาจาก **preutter ของ alias ตัวแรกของ phrase** (ไม่ใช่กติกาการแบ่ง):
เส้น universal ที่แมป 'sa' → alias `sa` ให้ leading **135.7** และ phrase 2 ให้ **70.6** ซึ่ง **ตรงกับ reference เป๊ะ**
ส่วนเส้น japanese ได้ alias ตัวแรกเป็น `s`/`k` (คนละ oto) จึงได้ leading คนละค่า

**ยืนยันซ้ำด้วย binary ที่ rebuild ล่าสุด (mtime 15:02 หลัง task-9)**: ได้ skip line เดิมทุกตัวอักษร
(`partial oto mapping (15 of 16 ...)`) และ 55,157 samples เท่าเดิม ⇒ behavior นี้ไม่ใช่ของ binary เก่า

## 5. ข้อเสนอ (เลือกได้ — ยังไม่แก้ในงานนี้)

**(ก) ใช้ Universal กับ bank แบบนี้ (คำแนะนำหลัก)** — มันแมปทุก lyric และไม่ skip phrase ใดเลย;
bank นี้เป็น English VCV ที่ alias ไม่ตรงกับ VCV kana ของเส้น japanese ⇒ ในเอกสาร/UX ควรบอกว่า
"bank ที่ไม่มี alias ญี่ปุ่น ให้ใช้ universal" (ซึ่งตรงกับ default ของ `PhonemizerKind::from_track` อยู่แล้ว)

**(ข) ปรับนโยบาย error ให้ตรง reference (เปลี่ยน behavior — ต้องคุยกันก่อน)**
ทำ per-phoneme error แบบ OpenUtau: ก่อน `PhraseGrouping::group` ให้ตัด phoneme ที่ `voicebank.lookup(...)` ไม่เจอ
(พร้อมเก็บเหตุผลไว้รายงาน) ⇒ ผลลัพธ์จะเท่า reference ทุกกรณี (โน้ตเดียวเสีย ไม่ลากทั้ง phrase)
ข้อแลกเปลี่ยน: เราเสีย "เห็น skip ชัด ๆ ทั้ง phrase" ไป ต้องรายงานเป็น per-phoneme แทน

**(ค) เพิ่ม alias coverage ของเส้น japanese** — ตอนนี้ candidate เป็น VCV kana patterns เท่านั้น;
ถ้าจะรองรับ bank อังกฤษที่มี alias ตรงกับ *lyric เปล่า ๆ* (sa/ku/no) ควรลอง candidate `{symbol}` (raw) ก่อนด้วย
ซึ่งมีอยู่แล้วในลิสต์ — แต่ของ 'ya' ไม่มีทั้ง `ya` และ VCV pair ⇒ ต้องพึ่ง (ก) หรือ (ข) ไม่ใช่ (ค)

**(ง) เพิ่ม "alias audit" ก่อน render** — รายงานล่วงหน้าว่า lyric ตัวไหนในโปรเจกต์จะหา alias ไม่เจอ
(มี phoneme table ในเทสต์เป็นแม่แบบแล้ว) เพื่อไม่ให้ผู้ใช้เจอ skip กลางทาง

## 6. ข้อจำกัด

- หลักฐานฝั่ง OpenUtau มาจาก **การอ่านโค้ด** + ตัวเลข phrase ที่ Lead รัน golden renderer ไว้ (ไม่ได้รัน .NET เองในงานนี้)
- ตัวเลขตัวอย่างเสียง (samples/ms) มาจาก binary ที่ build ไว้ 10:50; หลัง r2-wasm rebuild ควรวัดซ้ำ (ผมรายงานผลรอบใหม่ไว้ด้านบนถ้ามี)
- ยังไม่ได้ทดสอบเส้น `english` (CVVC+G2P) กับโปรเจกต์นี้ — อาจได้ผลต่างอีกแบบ

## 7. ไฟล์ที่แตะ

`docs/planning/phrase-segmentation-parity.md` (เอกสารนี้) · `native/tools/synth-cli/tests/phrase_segmentation_parity.rs` (เทสต์ใหม่)
ไม่แตะ: `web/**`, `models/**`, `ref/**`, `ref-openutau-clean/**`, `native/crates/phonemizer/**`, `native/crates/feed/**`,
`native/tools/synth-cli/src/**` (pipeline อยู่ในมือ r2-wasm/task-9)
