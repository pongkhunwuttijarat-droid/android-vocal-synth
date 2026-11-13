# phrase-join-gap — "รูทะลุ 150 ms ระหว่าง phrase" (T15)

> task: `task-15` (r2-wasm) · วันที่: 2026-09-23 · สถานะ: **หาสาเหตุได้แล้ว — รูของเรา "ถูกต้อง" และ golden demo ที่ใช้อ้างอิงมีบั๊กของ harness**
> อาการ: sakura (default phonemizer) ช่วง 1250–1429 ms และ 2000–2164 ms ของเราเป็น digital silence (−180 dB) แต่ "golden" ร้องต่อเนื่อง

## 0. สรุป

| คำถาม | คำตอบ | หลักฐาน |
|---|---|---|
| reference วาง phrase ยังไงให้ไม่มีรู? | **ไม่มี** — OpenUtau เองก็ปล่อยช่วง rest ว่าง เหมือนเรา (`RenderMixdown` วาง source ที่ `positionMs - leadingMs`, ยาว `estimatedLengthMs`) | §2 |
| แล้วทำไมไฟล์ "golden demo" ร้องต่อเนื่อง? | **บั๊กของ harness** `test/golden/golden-renderer/Program.cs` (demo mode): มันเรียก `mix.Mix()` ตรง ๆ ด้วย buffer ที่ allocate ครั้งเดียวและ **ไม่เคย clear** ⇒ ช่วงที่เป็นรูได้ audio เก่าค้าง (stale) + สะสมซ้ำทุก chunk | §3 |
| รูของเรามาจากไหน? | จาก **rest 250 ms ในโน้ตจริง** (note 4 จบ 1250 ms, note 5 เริ่ม 1500 ms) หักด้วย leading ของ phrase ถัดไป ⇒ ช่วง 1245.9–1429.4 ไม่มี source ไหนคลุม = เงียบโดยชอบ | §1 |
| ต้องแก้โค้ดเราไหม? | **ไม่** — พฤติกรรมตรงกับ reference ที่ใช้งานจริง (`MasterAdapter.Read` clear buffer ทุกครั้ง ⇒ รูเงียบ) | §3.2 |

## 1. วัดจริง: รูของเราอยู่ตรงไหน และ reference phrase ครอบอะไร

คะแนน sakura (`test/golden/sakura-test.ustx`): 13 โน้ต โน้ต 0–4 = 0–1250 ms, **rest 250 ms**, โน้ต 5–6 = 1500–2000 ms,
โน้ต 7 (`ya`) ถูก drop (ไม่มี oto alias), โน้ต 8–12 = 2250–3500 ms

**golden phrase mode** (`WORLDLINE_RENDERER=1 ./run.sh phrase`) รายงานต่อ phrase:

```text
phrase 0: samples=60929 (1381.6 ms) posMs=0.0    leadingMs=135.7 estLen=1385.7  -> [ -135.7, 1245.9]
phrase 1: samples=25089 ( 568.9 ms) posMs=1500.0 leadingMs= 70.6 estLen= 570.6  -> [ 1429.4, 1998.3]
phrase 2: samples=58881 (1335.2 ms) posMs=2250.0 leadingMs= 85.5 estLen=1335.5  -> [ 2164.5, 3499.7]
```

⇒ **reference ปล่อยช่วง 1245.9–1429.4 (183 ms) และ 1998.3–2164.5 (166 ms) ว่าง** — ตรงกับที่เราเงียบ (วัดได้ 150 ms ที่ -180 dB หลังปัดหน้าต่าง 50 ms)

วัดฝั่งเรา (`--phonemizer default`, engine r1-rs, segment เดียวกับ reference):

```text
g-sakura-demo.wav       3500 ms  rms  -5.8 dB  silence  0.0%  peak 1.000  clip 12403
our-sakura-default.wav  3500 ms  rms -14.0 dB  silence 10.3%  peak 0.627  clip     0

  1250-1500 ms ของเรา: ... -12.4 -180.0 -180.0 -180.0 -47.3 -31.3 -15.1 ...
  2000-2164 ms ของเรา: ... -13.8 -180.0 -180.0 -180.0 -33.7 -26.8 -12.7 ...
```

## 2. reference วาง phrase อย่างไร (อ่านจากโค้ด OpenUtau)

`ref-openutau-clean/core/OpenUtau.Core/Render/RenderEngine.cs:218-227`:

```csharp
var layout = phrase.renderer.Layout(phrase);
double posMs = layout.positionMs - layout.leadingMs;
double durMs = layout.estimatedLengthMs;
request.sources[i] = new WaveSource(posMs, durMs, 0, 1);
```

และ `Classic/WorldlineRenderer.cs:58-64`: `estimatedLengthMs = phrase.durationMs + phrase.leadingMs`
⇒ source ของแต่ละ phrase คลุม `[positionMs - leadingMs, positionMs - leadingMs + durationMs + leadingMs]` = `[positionMs - leadingMs, positionMs + durationMs]`
**เท่ากับสิ่งที่نا mixer ทำเป๊ะ** (และเท่ากับที่ T9 แก้ double-subtract ไปแล้ว) ⇒ ช่วง rest ต้องว่างทั้งสองฝั่ง

## 3. ทำไม "golden demo" ถึงร้องต่อเนื่อง — บั๊กของ harness

### 3.1 กลไก

`SignalChain/WaveMix.cs` — ไม่ clear buffer, แค่เรียก `sources.Max(s => s.Mix(...))`:

```csharp
public int Mix(int position, float[] buffer, int index, int count) {
    if (sources.Count == 0) return 0;
    return sources.Max(source => source.Mix(position, buffer, index, count));
}
```

`SignalChain/WaveSource.cs` — เขียนแบบ `+=` เฉพาะช่วงที่ source คลุม:

```csharp
for (int i = start; i < end; ++i) buffer[index + i - position] += data[i / copies - offset];
```

`test/golden/golden-renderer/Program.cs` (demo mode, บรรทัด ~269-301) — allocate buffer **ครั้งเดียว** แล้ววน `mix.Mix` โดยไม่ clear:

```csharp
var buffer = new float[chunkSize];          // 4096 floats = 2048 เฟรม = 46.4 ms
while (true) { int nextPos = mix.Mix(total, buffer, 0, chunkSize); ... }
```

⇒ (ก) ช่วงที่ไม่มี source คลุม = ค่าเก่าใน buffer ยังอยู่ → ได้ audio จาก chunk ก่อนหน้า **y้อนกลับมาเป็น "เสียง"**
⇒ (ข) ช่วงที่ source คลุม = `+=` ทับค่าเก่า → **สะสมซ้ำ** (นี่คือที่มาของ peak 2.47 → clip 12,403 และ rms ที่สูงกว่าจริง ~12 dB)

### 3.2 ฝั่งแอพจริงไม่เป็นแบบนี้

`SignalChain/MasterAdapter.cs:20-24` (เส้นทางเล่นจริงผ่าน NAudio) **clear buffer ก่อนทุกครั้ง**:

```csharp
public int Read(float[] buffer, int offset, int count) {
    for (int i = offset; i < offset + count; ++i) buffer[i] = 0;
    if (!source.IsReady(position, count)) { ... }
    int pos = source.Mix(position, buffer, offset, count);
```

⇒ ในแอพจริง ช่วง rest = **เงียบ** เท่ากับของเรา — "ร้องต่อเนื่อง" มีเฉพาะในไฟล์ demo ที่ harness สร้าง

### 3.3 พิสูจน์ด้วยการจำลอง

จำลอง mixdown ตาม harness ตรง ๆ (3 phrase ที่ offset/leading จริง, chunk 4096 floats, เขียน 16-bit clip เหมือน golden-renderer):

| โหมด | frames | peak | rms | clip | ช่วง 1250–1400 ms |
|---|---|---|---|---|---|
| **ไม่ clear buffer** (แบบ harness) | 154,335 | 1.0000 | −4.0 dB | 31,599 | −4.0 −4.0 −3.9 (ต่อเนื่อง) |
| **clear buffer** (แบบ MasterAdapter) | 154,335 | 0.6251 | −15.2 dB | 0 | **−180 −180 −180** (เงียบ) |
| golden demo (ไฟล์จริงของ Lead) | 154,336 | 1.0000 | −5.8 dB | 12,403 | −5.9 −5.8 −5.7 (ต่อเนื่อง) |
| ของเรา (pipeline) | 154,336 | 0.627 | −14.0 dB | 0 | −180 −180 −180 (เงียบ) |

⇒ โหมด "ไม่ clear" reproduce ลักษณะของ golden demo ได้ (ต่อเนื่อง + clip + peak 1.0) และโหมด "clear" reproduce ลักษณะของเราได้เป๊ะ
⇒ **ยืนยันกลไก** (ส่วนต่างระดับ ~1 dB ระหว่างการจำลองกับไฟล์จริงมาจากการที่ harness ใช้ `estimatedLength` กับ source ไม่ใช่ `data.Length` และจาก cache/การปัดเศษ)

## 4. ผลพลอยได้: อธิบายข้อค้างของ task-13 ได้ด้วย

อาการ "envelope ของ demo แกว่งคาบ ~325 ms / ไม่ใช่ pure gain" (§3.2 ของ `envelope-note-stretch-check.md`)
**คือผลของ buffer สะสมตัวเดียวกัน**: ทุก chunk ถูก `+=` ทับข้อมูล chunk ก่อนหน้า (shift 46.4 ms ต่อ chunk) ⇒ เกิด comb/accumulation
ไม่ใช่ gain envelope และไม่ใช่ "note stretch" ของเรา ⇒ ข้อค้าง T13 ปิดได้

## 5. สิ่งที่ทำต่อ / ข้อเสนอ

1. **[Lead ทำแล้ว/กำลังทำ]** แก้ harness `test/golden/golden-renderer/Program.cs` demo loop: clear buffer ก่อน `mix.Mix` ทุกครั้ง
   (mirror `MasterAdapter.Read`) —
   ```csharp
   int nextPos = mix.Mix(total, buffer, 0, chunkSize);   // เดิม
   // เพิ่มก่อนบรรทัดนี้:  Array.Clear(buffer, 0, chunkSize);
   ```
   หรือเปลี่ยนไปใช้ `MasterAdapter` + `ISampleProvider.Read` ตรง ๆ จะตรงกับแอพจริงที่สุด
2. **ไฟล์ golden demo เก่าทั้งหมดถูกยกเลิกการใช้** (Lead แจ้ง): `out/ab/golden-openutau-*-sakura.wav`, `golden-openutau-*-scaledemo.wav`,
   `/tmp/g-demo*.wav`, `/tmp/g-sakura*` มี stale audio + clipping ⇒ **อย่าใช้เทียบ RMS/envelope/silence%**
   (ไฟล์ **phrase** mono ยังใช้ได้ — พวกนั้นผ่าน `RenderPhrasesDirect` ไม่มี buffer ปัญหา)
3. **[ทำแล้วในงานนี้]** regression test `runtime::mixer::tests::gap_between_chunks_is_digital_silence`
   (Lead อนุมัติหลัง task-10 เสร็จ): chunk สองก้อนที่มีช่องว่างระหว่างกัน ต้องให้ **ศูนย์สนิท** ในช่องว่าง
   — กัน regression ถ้ามีคนทำ buffer reuse/smearing ในอนาคต · รันผ่านแล้ว (`cargo test -p runtime` = 57 passed)
4. **อย่าแก้ behavior ของเรา** เพื่อ "เติมรู": รูคือ rest ของเพลง และ reference จริงก็เงียบ

## 6. ไฟล์ที่แตะ

| ไฟล์ | สถานะ |
|---|---|
| `docs/planning/phrase-join-gap.md` | เอกสารนี้ (ใหม่) |
| `native/crates/runtime/src/mixer.rs` | เพิ่ม **เทสต์เท่านั้น** (Lead อนุมัติ): `gap_between_chunks_is_digital_silence` — ไม่เปลี่ยน behavior |

ไม่ได้แตะ `tools/synth-cli/src/pipeline.rs` หรือ `phonemizer/**` ในงานนี้

ไฟล์ชั่วคราว: `/tmp/g-sakura-phrase.wav`, `/tmp/g-sakura-demo.wav`, `/tmp/g-sakura-demo-cold.wav`, `/tmp/our-sakura*.wav`, `/tmp/r2w-*.py`
