# pool length mismatch — len −6, max|diff| 0.63 (T24)

> วันที่ 2026-09-23 · ต่อจาก T23 (`run-pool.sh` FAIL หลัง wasm refresh) · **ไม่แก้ native ในงานนี้** (นอก scope T24 — รายงาน Lead)
> หลักฐาน: `web/pool-test.html` diag (patch เฉพาะ debug lines, **ไม่ได้คลาย threshold**) · `native/target/pool-result.txt`

## 0. สาเหตุ (พิสูจน์แล้วเป็นตัวเลข)

**`PlannedChunk.position_ms` ในแผนเป็น "buffer origin" (= `chunk.start_ms − leading_ms`) แต่ `runtime::mixer::mix` คาดหวัง "จุดเริ่มของ note/chunk" แล้วหัก leading เอง**
⇒ เส้น pool/worker (ซึ่ง mix ผ่าน `Mixer.add_chunk(position_ms, leading_ms)` จาก `chunk_meta_json`) **หัก leading ซ้ำสองครั้ง**

พิสูจน์แบบหักล้างไม่ได้ (รันจริง, engine เดียวกัน ไม่มี worker):

```
DIAG-LEN  manual-chunk-mix = 639679   sequential = 639685   delta = -6
DIAG-DIFF manual vs sequential max|diff| = 6.299e-1
DIAG-FIX  compensated(+leading) len = 639685  vs sequential = 639685  delta = 0  max|diff| = 0.000e+0   <-- bit-identical
```

⇒ พอเติม `+leading_ms` ก่อนส่งให้ Mixer (ชดเชยการหักซ้ำ) ทั้งสองเส้นทาง **bit-identical ทันที (delta 0, max|diff| 0)**
⇒ ตัวเลขตรงพอดี: leading ของ fixture = **0.126 ms** → หักซ้ำ = −0.126 ms = **−5.56 → −6 samples** (truncation) = `delta = -6` และทุก chunk เลื่อนไป 6 samples ⇒ `max|diff| 0.63`

## 1. อาการ

```
pool 2274 ms | chunks rendered 4 | samples โหลด 2 | per-worker keys 2,2,2,2
plain sequential: 639685 samples
   pool vs sequential: MISMATCH ✘  (len 639679 vs 639685, max|diff| 6.3e-1)
```

plan (main engine และ worker เหมือนกันเป๊ะ — ยืนยันด้วย `chunk_meta_json`):

```
#0{pos=-0.126,lead=0.126,own=0-14,len=154355}
#1{pos=3499.874,lead=0.126,own=14-28,len=154355}
#2{pos=6999.874,lead=0.126,own=28-44,len=176405}
#3{pos=10999.874,lead=0.126,own=44-58,len=154591}
```

คำนวณด้วยมือ: `mix` วาง chunk ที่ `ms_to_samples_signed(position_ms − leading_ms)`
- #3: `(10999.874 − 0.126) × 44.1 = 485,088.9 → 485,088` → end = 485,088 + 154,591 = **639,679** = ความยาวที่ pool ได้
- เส้น sequential: `trim_chunk_audio` คืน `position_ms = chunk.start_ms` ⇒ `(11000.000 − 0.126) × 44.1 = 485,094.5 → 485,094` → end = **639,685** ✓
⇒ ต่างกัน 6 samples ที่ chunk สุดท้าย และทุก chunk เลื่อน 6 samples → diff ทั้งเพลง

## 2. สิ่งที่ตัดออกไปแล้ว (ไม่ต้องทำซ้ำ)

| สมมติฐาน | ผล |
|---|---|
| JS mixer / padding ต่างกัน | ❌ `Mixer` เป็น Rust wrapper ของ `runtime::mixer::mix` และ sequential ก็ใช้ `mix(&chunks)` ไม่มี `mix_min_duration` |
| worker re-plan จาก YAML ได้ plan ต่าง | ❌ `chunk_meta_json` ของ main engine = ของ pool เป๊ะ (pos/lead/len ตรงกันทุก chunk) |
| render engine ต่างกัน | ❌ ทั้งคู่เรียก `render_planned_chunk` ตัวเดียวกัน (chunk len ตรงกันทุก chunk) |
| **convention ของ `position_ms` ไม่ตรงกัน** | ✅ **ใช่** (พิสูจน์ด้วย DIAG-FIX) |

## 3. จุดแก้ (native — 1 บรรทัด, อยู่นอก scope T24)

`native/tools/synth-cli/src/pipeline.rs` ใน `plan_track_chunks`:

```rust
 // เดิม: buffer origin (ทำให้ mixer หัก leading ซ้ำ)
 position_ms: chunk.start_ms - first_lead_of(&chunk),
 // ควรเป็น: จุดเริ่ม note/chunk (ให้ mixer หัก leading เอง ครั้งเดียว)
 position_ms: chunk.start_ms,
```

เหตุผลว่าทำไมต้องแก้ที่จุดนี้: T9 แก้ `trim_chunk_audio` ให้คืน `chunk.start_ms` แล้ว (พร้อมคอมเมนต์ "Returning the buffer origin here made the mixer subtract the leading a SECOND time")
แต่ **`PlannedChunk.position_ms` ยังใช้ convention เก่า** และถูกส่งออกผ่าน `chunk_meta_json` → `Mixer.add_chunk` ⇒ เส้น pool/worker ยังหักซ้ำอยู่

**จุดที่ได้รับผลกระทบ**
- `engine-wasm/src/lib.rs:505` (`render_project_planned`) ส่ง `planned.position_ms` เข้า `Mixer::add_chunk` ⇒ บั๊กเดียวกัน (pool/editor ใช้ทางนี้)
- `web/pool.js` `mix()`/`toWav()` อ่าน `meta.position_ms` จาก `chunk_meta_json` ⇒ ได้ค่าเดียวกัน (จะถูกต้องทันทีเมื่อแก้ต้นทาง)
- render cache hash ไม่ได้รวม `position_ms` ⇒ **ไม่ต้องล้าง cache**

**ขนาดของบั๊กขึ้นกับ leading ของ bank**: fixture นี้ leading = 0.126 ms ⇒ เพี้ยน 6 samples (ฟังไม่ออก แต่ test จับได้)
แต่ bank จริง (leading ~135 ms เช่น `sa` ใน Teto) ⇒ เส้น pool จะเลื่อนไป ~**5,900 samples (~134 ms)** = เพี้ยนแบบได้ยินชัด ⇒ **ควรแก้ก่อนใช้งานจริง**

**เทสต์กัน regression ที่เสนอ** (สำหรับเจ้าของ native)
1. `plan_track_chunks` + `render_chunk` + `Mixer` (หรือ `render_project_planned`) ต้อง **bit-identical** กับ `render_project` (ตอนนี้ต่าง `len −6, max|diff| 0.63`)
2. `chunk_meta_json` ต้องรายงาน `position_ms == chunk.start_ms` (ไม่ใช่ `start_ms − leading_ms`)
3. รัน `./web/run-pool.sh` ต้องได้ `bit-identical ✔` ที่ทุกข้อ (cold/repeat/incremental)

## 4. สถานะงานนี้ (T24)

- วินิจฉัยจบ **เป็นตัวเลข** (chunk สุดท้าย #3, −6 samples, เกิดจาก leading 0.126 ms ถูกหักซ้ำ) + พิสูจน์ด้วย compensated mix (delta 0 / max|diff| 0)
- **ไม่ได้แก้ native** (T24 ห้ามแตะ `native/tools/synth-cli/**` + "ถ้าต้องแตะโค้ด native ให้รายงาน Lead ก่อน") ⇒ **`run-pool.sh` ยัง FAIL ตามจริง** และ **ไม่มีการคลายเกณฑ์/threshold ใด ๆ**
- patch ใน `web/pool-test.html` เป็น **diagnostic lines เท่านั้น** (`DIAG-ENGINE`/`DIAG-LEN`/`DIAG-DIFF`/`DIAG-POOL`/`DIAG-FIX`) — เกณฑ์ `compare()` เดิมไม่ถูกแตะ
- ค่าอื่นในชุด harness ยังผ่านตาม T23 (smoke/editor/vocoder/r2 + cargo test 66/0)

## 5. ไฟล์ที่แตะ

`web/pool-test.html` (เพิ่ม DIAG lines + compensated-mix proof; ไม่แก้เกณฑ์) · `docs/planning/pool-length-mismatch.md` (เอกสารนี้)
**ไม่แตะ**: `native/tools/synth-cli/**`, `native/crates/phonemizer/**`, `native/crates/runtime/src/mixer.rs`, `test/**`
