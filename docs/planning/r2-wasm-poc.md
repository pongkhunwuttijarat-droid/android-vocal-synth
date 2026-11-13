# R2 chain บน wasm — mel.onnx → pc-nsf-hifigan (task-5)

> วัดเมื่อ 2026-09-23 · เครื่องเดียวกับ `docs/architecture/wasm-performance.md` (AMD Ryzen 5 8600G, 12 threads)
> · Chromium headless (new headless) · onnxruntime-web **1.30.0** (wasm EP, SIMD, `numThreads=1`)
> · โมเดล R2 จริงทั้งสองตัว จาก OpenUtau package (ดู §2) · ไม่มี mock

**สรุป: ผ่าน (PASS)** — worldline-R2 neural chain ตัวจริงรันครบในเบราว์เซอร์:
`f0 + sp_env + ap → mel.onnx → mel[1,T,128] → pc-nsf-hifigan → waveform → trim/ease`
โมเดลทั้งสองโหลดจากดิสก์จริง (sha256 ตรวจในเชลล์เทียบ PROVENANCE) และ **parity กับ native CPU EP
บน tensor ชุดเดียวกัน** อยู่ในระดับ ≤1 LSB

**ข้อค้นพบที่แก้ความเข้าใจเดิม (สำคัญ)**: vocoder ONNX ของ package `pc-nsf-hifigan` ที่ registry ให้มา
มี sha256 `b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d` —
**เป็นไฟล์ byte-identical กับ vocoder ที่ T1 ใช้** (`test/golden/diffsinger-kiritan/nsf_hifigan/model.onnx`)
⇒ ประเด็นไม่ใช่ "vocoder คนละตัว" แต่คือ **mel producer คนละตัว**: R2 ป้อน mel จาก `mel.onnx` (WORLD sp_env/ap → mel)
ส่วน DiffSinger ป้อน mel จาก acoustic model ที่เรียนรู้ mel space ของตัวเอง — ใช้ mel ข้ามกันไม่ได้
ไม่ใช่เพราะตัว vocoder ต่างกัน

---

## 1. วิธีรันซ้ำ (คำสั่งเดียว)

```bash
./web/run-r2.sh              # browser chain + parity กับ native CPU EP (onnxruntime-node)
PARITY=0 ./web/run-r2.sh     # เฉพาะ browser chain (ไม่ต้องติดตั้ง onnxruntime-node)
NATIVE_R2=1 ./web/run-r2.sh  # เพิ่ม: full native R2 render ผ่าน synth-cli --engine r2
FRAMES=128 FRAMES2=200 ./web/run-r2.sh   # เปลี่ยนจำนวนเฟรม
```

สคริปต์: (1) ตรวจ sha256 ของโมเดลทั้งสองเทียบ PROVENANCE (2) วัด **baseline RAM** ของ Chromium บน about:blank
ด้วยวิธีเดียวกัน (3) เปิด `web/bench-server.py` เสิร์ฟ repo root + headless Chromium ไปที่ `web/r2-test.html`
(4) **ตรวจ assertions ซ้ำในเชลล์** (5) ติดตั้ง/รัน onnxruntime-node (native CPU EP) ด้วย tensor ชุดเดียวกัน แล้วเทียบ WAV
ทีละ sample (6) ถ้า `NATIVE_R2=1` ก็รัน native R2 เต็มเส้นทาง

ไฟล์ผลลัพธ์ (ไม่ commit):

| ไฟล์ | อะไร |
|---|---|
| `/tmp/r2-run.txt` | log ทั้งหมด (progress + JSON) |
| `/tmp/r2-result.json` | ตัวเลขทั้งหมดจากหน้าเว็บ (รวม `wasmMemory`) |
| `/tmp/r2-wasm.wav` | WAV จากเส้นทาง wasm (browser) |
| `/tmp/r2-native.wav` | WAV จากเส้นทาง native CPU EP (onnxruntime-node) สำหรับ parity |
| `/tmp/r2-native.json` | ตัวเลขฝั่ง native (session/infer/mel fnv/audio fnv) |
| `/tmp/r2-rss.txt`, `/tmp/r2-rss-baseline.txt` | peak RSS ของ process tree (รันจริง / baseline about:blank) |

## 2. Provenance ของโมเดล (ครบใน `models/r2/PROVENANCE.md`)

| ของ | ที่มา | size | sha256 |
|---|---|---|---|
| `models/r2/mel.onnx` | `ref(openutau+openutau mobile)/core/OpenUtau.Core/Classic/Data/mel.onnx` (ไฟล์เดียวกับที่ `WorldlineRenderer.cs:163-178` โหลด) | 85,404 B | `1493ffc868c6fb8b66104e2c6a2902cbf1a8c02bb09215db014f02e9d0e82f94` |
| `models/r2/pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx` | OpenUtau registry item `pc_nsf_hifigan_44.1k_hop512_128bin_2025.02` → `.oudep` (zip) จาก github.com/openvpi/vocoders releases | 56,616,643 B | `b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d` |
| `...2025.02.oudep` (เก็บไว้เป็น artefact ต้นทาง) | URL ในตาราง PROVENANCE | 52,760,610 B | `ba7d43142d41f6900c8264b5662ca7125a50feb8760bb8b9615c61a8f5e6902e` (ตรงกับ `hash` ใน registry) |

`vocoder.yaml` ใน package ยืนยันสเปก: `sample_rate: 44100 · hop_size: 512 · fft_size: 2048 · num_mel_bins: 128 ·
mel_fmin: 40 · mel_fmax: 16000 · mel_base: e · mel_scale: slaney · pitch_controllable: true`
⇒ `sp_size = 2048/2 + 1 = 1025` ตรงกับ input `sp_env`/`ap` ของ mel.onnx

## 3. Chain ที่รัน (ตรงกับต้นฉบับ)

```
f0[1,T] sp_env[1,T,1025] ap[1,T,1025]
   └─ mel.onnx (opset 20, 304 nodes) ─→ mel[1,T,128]
        └─ pc-nsf-hifigan ─→ waveform[1, T*512]
             └─ ตัด leftPad*512, เอา frames*512, ease 512 sample หัว/ท้าย
```

- padding ตาม `WorldlineRenderer.cs:143-150`: `T = ceil((frames + 8 + 4)/16)*16`, `leftPad = 4` frames
  (f0/sp_env pad ด้วย 0, ap pad ด้วย 1)
- trim/ease ตาม `WorldlineRenderer.cs:218-228` (เหมือน `native/crates/worldline-plugin/src/r2_engine.rs:186-192`)
- input เป็น **WORLD-like features สังเคราะห์แบบ deterministic** (ไม่ใช่จาก audio จริง) — ดูข้อจำกัด §7
- โมดูลตัวสร้าง input ใช้ร่วมกันทั้งสองเส้นทาง: `web/vendor/r2-inputs.mjs` (หน้าเว็บ import, parity script import)
  ⇒ ทั้งสองฝั่งรายงาน input FNV และเชลล์ยืนยันว่า **tensor ชุดเดียวกันจริง**

## 4. ตัวเลขที่วัดได้ (browser/wasm)

### 4.1 โหลด + session (wasm EP)

| ขั้น | เวลา | หมายเหตุ |
|---|---|---|
| import ort-web | 12 ms | vendored, version 1.30.0 |
| fetch `mel.onnx` (85,404 B) | 3 ms | จาก local HTTP |
| `InferenceSession.create` mel.onnx | 175 ms | 304 nodes, opset 20 (มี STFT/If/Resize) |
| fetch vocoder (56,616,643 B) | 45 ms | ~1.2 GB/s |
| `InferenceSession.create` vocoder | 56 ms | |
| **รวม** | **291 ms** | โหลดทั้งสองโมเดล |

### 4.2 Inference ต่อ stage (wasm EP, SIMD, numThreads=1)

| frames | padded T | mel.onnx → mel[1,T,128] | vocoder → waveform | chain รวม | realtime | peak (trimmed) | rms |
|---|---|---|---|---|---|---|---|
| 64 | 80 | **39.4 ms** (ครั้งแรก) | **1,317.1 ms** (40,960 samples) | 1,356.5 ms | 0.55× | 0.623760 | 0.086781 |
| 97 | 112 | **10.1 ms** | **1,902.9 ms** (57,344 samples) | 1,913.0 ms | 0.59× | 0.627119 | 0.085347 |

- trimmed ออกมา **32,768 = 64×512** และ **49,664 = 97×512** ตรงตาม hop จาก `vocoder.yaml` (`hop_size: 512`)
- **vocoder กินเวลา 97–99% ของ chain** — mel.onnx แทบฟรี (39 ms ครั้งแรก แล้ว 10 ms)
- mel ที่ `mel.onnx` ผลิตจริง: min **-16.8621** max **-1.9697** mean **-7.1395** (n=64) / mean -6.6165 (n=97)
  — อยู่ในช่วง log-mel ที่สมเหตุสมผล (โมเดลไม่พังกับ feature สังเคราะห์)
- WAV ที่ได้: `/tmp/r2-wasm.wav` 65,580 B (32,768 frames @44.1k)

**ข้อค้นพบข้ามงาน (ตอบคำถามค้างของ T1)**: ใน T1 ผมป้อน mel สังเคราะห์เอง (mel DC ≈ -7) ได้ peak แค่ 0.0065
แต่ mel จริงจาก `mel.onnx` ให้ audio peak **0.62** ⇒ ที่ T1 เบาเพราะ **mel สังเคราะห์เบา** ไม่ใช่ vocoder มีปัญหา
(ยืนยันว่าสายพานนี้ให้ audio ระดับปกติเมื่อ mel มาจากผู้ผลิตจริง)

## 5. Parity: wasm EP vs native CPU EP (tensor ชุดเดียวกัน)

tensor ชุดเดียวกัน (ยืนยันด้วย input FNV ทั้งสองฝั่ง) ยิงเข้า wasm EP (browser) และ native CPU EP
(`onnxruntime-node` 1.30.0 — ORT เวอร์ชันเดียวกันต่าง EP) แล้วเทียบ:

| อย่าง | frames=64 | frames=97 |
|---|---|---|
| input FNV เท่ากัน | f0=911467587 · sp=2025872470 · ap=1952218500 | f0=1251321921 · sp=1572365902 · ap=2620644714 |
| mel bit-identical (f32) | ไม่ (float kernel ต่างกัน) | ไม่ |
| audio bit-identical (f32) | ไม่ | ไม่ |
| mel mean | wasm -7.139546 / native -7.139546 | -6.616480 / -6.616480 |
| audio peak | wasm 0.623760 / native 0.623760 | 0.627119 / 0.627119 |
| session create | mel 175 ms (wasm) vs 38 ms (native) · vocoder 56 vs 66 ms | |
| inference | mel 39.4 vs 6 ms · vocoder **1,317.1 vs 130 ms** | mel 10.1 vs 10 ms · vocoder **1,902.9 vs 173 ms** |

**WAV parity (16-bit, 32,768 frames): max|diff| = 1 LSB · ต่างกัน 148 samples (0.4517%)** ⇒ **PASS (เกณฑ์ ≤2 LSB)**

อ่านตัวเลขให้ถูก:
- **ผ่าน parity ในระดับที่ควรจะเป็น**: 16-bit audio ตรงกันภายใน 1 LSB ส่วน float ไม่ bit-identical เป็นเรื่องปกติ
  ของการย้าย EP (คนละ kernel/vectorization) — ค่า mean/peak ตรงกันถึงทศนิยม 6 ตำแหน่ง
- **เวลาที่เทียบกันไม่แฟร์**: native CPU EP ใช้ default thread pool (12 cores) แต่ wasm ตั้ง `numThreads=1`
  ⇒ ตัวเลข 10× ที่ vocoder **ไม่ใช่** wasm penalty ต่อ core (T1/T2 วัดได้ ~1.6–1.7× ต่อ 1 เธรด)
  ถ้าจะเทียบให้แฟร์ต้องเปิด COOP/COEP แล้วตั้ง `numThreads=4` ทั้งสองฝั่ง (ดู §7)
- parity นี้เป็น **ORT-vs-ORT** ไม่ใช่ "wasm vs C#/native R2 ด้วย WORLD feature จากเสียงจริง"

### 5.1 Native R2 reference (เต็มเส้นทาง, โบนัส)

คำสั่งนี้รันได้จริงกับโมเดลที่ดาวน์โหลดมา (`NATIVE_R2=1 ./web/run-r2.sh`):

```bash
native/target/release/synth-cli render --engine r2 \
  --mel-onnx models/r2/mel.onnx \
  --vocoder-onnx models/r2/pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx \
  --so native/build/build-linux/libworldline.so \
  --project test/golden/sakura-test.ustx --voicebank test/golden/teto-english/library \
  --out /tmp/r2-native-real.wav --verbose
```

| ผล | ค่า |
|---|---|
| phrases | 2 (track 0 "T", phonemizer Universal) |
| audio | **151,235 samples = 3.429 s @44.1k** · peak **0.7880** · rms **0.2699** · `/tmp/r2-native-real.wav` (302,514 B) |
| `WORLD_DUMP_MEL_FILE` (จริง, หลัง mel.onnx) | 179 frames · f0 0–440 Hz · **mel min -14.03 max +2.41 mean -3.78** |
| หมายเหตุ | log ขึ้น "(cached)" ต่อ phrase = **render cache เปิดใช้งาน** ไม่ได้แปลว่า cache hit — พิสูจน์ว่าโมเดลรันจริงจาก mel dump ที่ถูกเขียนใหม่ระหว่างรัน (mtime 14:20) และ WAV ที่ได้ |

⇒ สายพาน R2 ตัวจริงทำงานได้ทั้งสองฝั่ง (native และ wasm) ด้วย **ไฟล์โมเดลชุดเดียวกันเป๊ะ**
· การเทียบ sample-by-sample ระหว่าง native WAV กับ browser ยัง **ไม่ได้ทำ** เพราะ browser ไม่มี sp_env/ap จริง
(WORLD analysis เป็น native C++) — ต้อง dump sp_env/ap ด้วยถึงจะเทียบเต็มเส้นทางได้ (ดู §7.2)

## 6. RAM

วัดด้วยวิธีเดียวกันทั้ง baseline และรันจริง (`web/vendor/rss-sampler.py`, BFS รวมลูกหลาน, ทุก 200 ms):

| ตัวชี้วัด | baseline about:blank | รัน chain จริง | **delta** |
|---|---|---|---|
| peak ของผลรวม RSS ทั้ง tree | 1,185.2 MB | 1,468.3 MB | **+283.1 MB** |
| biggest single process | 221.9 MB | 394.6 MB | **+172.7 MB** |
| **wasm linear memory (final)** | — | **160.1 MB** | ← ตัวเลข engine ที่สื่อความหมาย |
| JS heap (used / limit) | — | 61.7 MB / 4,192 MB | (รวม ArrayBuffer ของโมเดล 54 MB) |

- **wasm linear memory 160.1 MB** ได้จากการ hook `WebAssembly.Memory` ก่อน import ort แล้วอ่าน
  `buffer.byteLength` สูงสุดที่ ORT ขอไว้ ⇒ = **2.96× ขนาดโมเดล ONNX (54 MB)** (ทั้งสองโมเดล + arena + activation;
  mel.onnx แค่ 85 KB จึงแทบไม่มีผล)
- **คำเตือน**: process-tree RSS ส่วนใหญ่คือ Chromium เอง (แท็บเปล่า 1.19 GB) ⇒ delta +283 MB เป็น **upper bound**
  ไม่ใช่ RAM ของ engine ล้วน ๆ · และ **ห้ามเทียบข้ามวิธีวัด** (T1/T5 ใช้ BFS tree, T4 ใช้ flat sum — 1,468.3 กับ 1,045.8
  เทียบกันตรง ๆ ไม่ได้)
- สรุป: RAM ที่ engine ใช้จริง ~160 MB ต่อแท็บ สำหรับโมเดล 54 MB ⇒ เดสก์ท็อป/แล็ปท็อปไหว แต่แท็บเล็ต/มือถือ
  ที่เพดาน wasm memory ต่ำกว่ายังเสี่ยง (โดยเฉพาะเมื่อรวม acoustic/mel producer ตัวอื่น)

## 7. ข้อจำกัด / ทางปรับปรุง (ยังไม่แก้ในงานนี้)

1. **single-thread**: หน้าเว็บตั้ง `ort.env.wasm.numThreads = 1` เพราะ `bench-server.py` (SimpleHTTPRequestHandler)
   ไม่ได้ส่ง COOP/COEP ⇒ ไม่มี `SharedArrayBuffer` ⇒ wasm รันเธรดเดียว (ort จะ fallback เองพร้อม warning)
   task-4 พบว่า wasm ช้ากว่า native 1.7–15.4× จากสาเหตุทำนองเดียวกัน ⇒ **ทางปรับปรุงอันดับหนึ่ง** คือเสิร์ฟด้วย
   COOP: same-origin + COEP: require-corp แล้วเปิด `numThreads = 4` (และอาจทดลอง WebGPU EP) แล้ววัดใหม่
2. **parity เป็น ORT-vs-ORT** (wasm EP vs native CPU EP ของบน onnxruntime เวอร์ชันเดียวกัน) ไม่ใช่
   "wasm vs C#/native R2 ที่ใช้ WORLD features จาก audio จริง" — ตัวหลังต้องมี sp_env/ap จริงซึ่งวิเคราะห์ด้วย WORLD (native)
3. **input เป็น features สังเคราะห์** ไม่ได้มาจากการวิเคราะห์เสียงจริง จึงพิสูจน์ "chain ทำงาน/เวลา/RAM/parity"
   แต่ไม่พิสูจน์คุณภาพเสียงบนเพลงจริง (แม้ค่า mel ที่ได้จะอยู่ในช่วง log-mel ที่สมเหตุสมผล — ดู §4)
4. **hop=512 hardcode** ในหน้าเว็บ/สคริปต์ (ค่าคาดหวังของ assert) ยืนยันแยกจาก `vocoder.yaml` + โครงสร้าง ConvTranspose
   ของ ONNX ไม่ได้ derive ตอนรัน
5. `mel.onnx` ใช้ op ที่หนักกว่า vocoder มาก (STFT, If, Resize, ScatterElements, ConstantOfShape, opset 20)
   — รันได้บน wasm EP ของ ort-web 1.30.0 แต่ยังไม่ได้ทดสอบบน Safari/มือถือ
6. ยังไม่ได้เทียบกับ **native R2 render เต็มเส้นทาง** แบบ sample-by-sample (ดู §5 ว่าทำได้แค่ไหน)

## 8. ไฟล์ที่แตะในงานนี้ (write scope)

`models/r2/**` (binaries gitignored ผ่าน `models/r2/*`; `models/r2/PROVENANCE.md` **tracked**) · `web/r2-test.html` · `web/run-r2.sh` ·
`web/vendor/r2-inputs.mjs` · `web/vendor/r2-parity.mjs` · `web/vendor/rss-sampler.py` ·
`web/vendor/fetch-onnxruntime-node.sh` · `web/vendor/.gitignore` · `.gitignore` (`models/r2/*` + `!models/r2/PROVENANCE.md`) · เอกสารนี้

ไม่ได้แตะ: `web/app/**`, `web/pool.js`, `web/real-test.html`, `web/run-real.sh`, `web/vocoder-test.html`,
`web/run-vocoder.sh`, `web/bench-server.py`, `native/**`
(ใช้ `bench-server.py` เป็นแม่แบบการเก็บผล headless แบบอ่านอย่างเดียว)
