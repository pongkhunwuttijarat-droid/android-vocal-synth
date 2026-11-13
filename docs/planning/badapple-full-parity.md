# Bad Apple ทั้งเพลง — golden vs เรา (T22)

> 2026-09-23 · เพลง 504.4 วิ (867 notes, 138 BPM) · engine **r2** · ของเราใช้ `--phonemizer default`
> ไฟล์: `out/ab/golden-openutau-r2-badapple-FINAL.wav` (stereo, 89 MB) ↔ `out/ab/ours-r2-badapple-FINAL.wav` (mono, 44.5 MB)
> อ้างอิง harness: golden-renderer มี fix `Array.Clear` (T15) + demo เคารพ `WORLDLINE_RENDERER` (T17) + ไฟล์ `badapple.ustx` ใหม่มี pitch block 867/867 (ไม่มี output_dir/cache_dir)

## 1. ตารางเทียบ

| วัด | golden | ours | ผล |
|---|---|---|---|
| duration | 504,363 ms (22,242,406 fr) | 504,358 ms (22,242,198) | **−208 samples = −4.7 ms** ✅ |
| **onset (−30 dB, ถือ 50 ms)** | **3450 ms** | **3450 ms** | **ตรงกันเป๊ะ** ✅ |
| silence (เกณฑ์ −45 dBFS) | 23.8% | 21.7% | ใกล้กัน (ระดับต่าง ~3 dB ทำให้ threshold ต่าง) |
| rms | −15.0 dB | −11.6 dB | **+3.4 dB** ≈ pan law 3.01 + 0.4 residual |
| peak | 0.688 | **1.000 (clip 4 samples)** | ⚠️ ดู §4 |
| clip | 0 | 4 / 22,242,198 | ⚠️ §4 |
| **phrases** | **123** | **123 distinct positions · 0 skipped** | ✅ ตรง |

## 2. ความตรงของการจังหวะ/พลวัต (หัวใจของการเทียบ)

- **envelope Pearson (50 ms, stride 1 เต็มไฟล์, 10,087 blocks) = 0.928016**
- **best lag = 0 ms** (Pearson 0.928) ⇒ **ไม่มีการเลื่อนสะสมตลอด 504 วินาที**
- per-60s waveform corr = 0.067–0.347 (9 sections) — ต่ำตามคาด: คนละตัวสังเคราะห์ (vocoder/phase ต่าง) ⇒ **ใช้ envelope เป็นเกณฑ์ ไม่ใช่ waveform**
- LS gain ที่ lag 0 = 0.2273 (−12.87 dB) — **ไม่ใช่ค่าระดับเสียง** แต่เป็น regression slope ซึ่งไร้ความหมายเมื่อ corr ต่ำ · ระดับจริงดูจาก rms = +3.4 dB

## 3. phrase count ที่นับใหม่ (รอบแรกนับผิด)

- `grep -c "phrase @"` = 166 → **นับซ้ำ** (บรรทัดละ phrase×chunk: `phrase @N ms chunk x-y`)
- **นับ position ไม่ซ้ำ = 123 = golden 123** ✅
- summary ของเรา: `rendered 166 phrase(s), 0 skipped` ⇒ **0 skipped** ✅ (รอบแรกอ่าน grep -ci "skip" = 1 บรรทัด = บรรทัด summary เอง ไม่ใช่ 1 phrase ถูกทิ้ง)

## 4. ข้อควรสังเกต

1. **ours clip 4 samples (peak 1.000)** — 4 ใน 22M ไม่ได้ยิน แต่เป็นสัญญาณว่า chain r2 แตะ full scale บนเพลงยาว (single-note peak 0.789 ไม่แตะ) → ควรพิจารณา gain trim/limiter ที่ master (นอก scope T22 → ถ้าผู้ใช้ต้องการให้เปิดงาน)
2. duration ขาด 4.7 ms (208 samples) — สัดส่วนเล็กมาก (0.0009%) และเกิดจาก tail/rounding ที่ทราบกลไกแล้ว (T20/T21)
3. waveform corr ต่ำ (0.155) — **ไม่ใช่ regression**: คนละตัวสังเคราะห์ · envelope 0.928 ที่ best-lag 0 คือเกณฑ์ที่ถูกต้อง

## 5. วิธีรันซ้ำ

```
# ours
native/target/release/synth-cli render --engine r2 --mel-onnx models/r2/mel.onnx \
  --vocoder-onnx models/r2/pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx \
  --so native/build/build-linux/libworldline.so --project test/golden/badapple.ustx \
  --voicebank test/golden/teto-english/library --phonemizer default --out ours.wav
# golden (harness แก้แล้ว)
(cd test/golden/golden-renderer && WORLDLINE_RENDERER=2 ./run.sh demo golden.wav \
  ../../badapple-openutau.ustx)
# วัด
python3 test/golden/ab_metrics.py golden.wav ours.wav
python3 /tmp/t22-analyze.py golden.wav ours.wav   # envelope Pearson stride 1 + best-lag + per-60s
```

## 6. บทเรียนสภาพแวดล้อม

- render ยาว (504 วิ x 2 ฝั่ง) โดน kill ตอน session reset **3 ครั้ง** → จบด้วยการรันใน session ของ Lead ด้วย tracked job (job ของเพื่อนร่วมทีมถูก reset ทิ้ง · `setsid nohup` ก็ไม่รอดในที่นี้)
- cache ของ renderer (`test/golden/teto-english/cache`, 51M) **resume ข้ามครั้งได้** — ครั้งสุดท้ายจบ ours ใน 105 วิ + golden ใน 81 วิ

## 7. Residual A/B — chunk ยังปรับให้ใกล้ reference ได้อีก

> บันทึก 2026-09-24 หลังปรับไฟล์เทียบให้เป็น stereo ซ้ำโมโน, ลด gain 3.3 dB และเติมท้าย 208 samples

- หลังปรับแล้ว format/ความยาวตรงกัน, RMS ต่างกันประมาณ 0.05 dB, integrated loudness = −11.1 LUFS และ LRA = 4.4 LU ทั้งคู่
- envelope RMS แบบ 50 ms มี Pearson **0.928** และ best lag **0 ms**; ความต่างอยู่เป็นช่วงสั้น ๆ แถว attack/preutter ของบางโน้ต มากกว่าจะเป็นการเลื่อนจังหวะหรือ drift ทั้งเพลง
- reference เรนเดอร์ 123 phrases; pipeline เราวางแผน 166 chunks เพราะแบ่ง phrase ยาวเป็น chunk ตามขอบเขตโน้ต (target 4 s) พร้อม context/trim/crossfade
- **ข้อสังเกต/งานต่อ:** chunk boundary และบริบทของแต่ละ render อาจเป็นที่มาของ residual รอบ attack/release; ควรเทียบ phrase ที่มีจุดต่างแบบ whole-phrase กับ chunked ก่อนเลือกปรับบริบทหรือวิธี crossfade อย่าใช้ waveform correlation เป็นเกณฑ์หลัก เพราะ phase/vocoder ต่างกันได้
- แก้ native scheduler แล้ว: เดิม Rayon ทำงานเฉพาะ chunks ภายใน phrase ทีละ phrase จึงไม่กระจาย phrase ที่มี chunk เดียวพร้อมกัน; ตอนนี้รวบงานทั้ง track แล้ว dispatch งานของ renderer ที่ thread-safe พร้อมกัน ก่อนคืนผลตามลำดับเดิมไป mix
- วัดด้วย probe 8 phrases ที่ phrase ละ 1 chunk: `RAYON_NUM_THREADS=1` ใช้ 858 ms; Rayon ค่าเริ่มต้นใช้ 160 ms (5.4× เร็วขึ้นบนเครื่องนี้) — ก่อนแก้ รูปแบบงานนี้จะไม่มีงานคู่ใน phrase ให้ Rayon กระจาย
