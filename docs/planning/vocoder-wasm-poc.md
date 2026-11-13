# Vocoder บน wasm — POC (task-1)

> วัดเมื่อ 2026-09-23 · เครื่องเดิมกับ `docs/architecture/wasm-performance.md` (AMD Ryzen 5 8600G, 12 threads)
> · Chromium headless (new headless) · onnxruntime-web **1.30.0** (wasm EP, SIMD, 1 thread)
> · โมเดลจริง: `test/golden/diffsinger-kiritan/nsf_hifigan/model.onnx` (56,616,643 bytes = 54.0 MiB)
> · **สถานะการตรวจ**: ผ่านการตรวจอิสระโดย teammate `poc-verifier` (รันเอง 2 รอบ, PASS, WAV sha256 ตรงกับเอกสาร) —
>   ดู `docs/planning/vocoder-wasm-verification.md` · เอกสารนี้ปรับตามข้อท้วงติงของผู้ตรวจ 9 ข้อ วันที่ 2026-09-23

**สรุป: ผ่าน (PASS)** — nsf_hifigan รัน "ในเบราว์เซอร์" ได้จริงด้วย onnxruntime-web wasm backend
ไม่ต้องแก้ Rust ไม่ต้องมี native process: โหลดโมเดล 56.6 MB จากดิสก์เข้า wasm, สร้าง session,
แล้ว inference จริงได้ output ยาว `n_frames × 512` ตรงตามโครงสร้างโมเดล ไม่ใช่ค่าคงที่/ศูนย์
มี WAV จริงให้ฟังที่ `/tmp/vocoder-poc.wav`

---

## 1. วิธีรันซ้ำ (คำสั่งเดียว)

```bash
./web/run-vocoder.sh
```

สคริปต์จะ: (1) ตรวจ/ดาวน์โหลด ort-web ที่ pin ไว้ (2) เปิด `web/bench-server.py` เสิร์ฟ repo root
(3) เปิด headless Chromium ไปที่ `web/vocoder-test.html` (4) รอผล POST `/__result`
(5) **ตรวจซ้ำเองทุกข้อในเชลล์ ไม่เชื่อ verdict จากหน้าเว็บ** แล้วพิมพ์ `==> PASS` / `==> FAIL`

ไฟล์ผลลัพธ์ (ไม่ commit):

| ไฟล์ | อะไร |
|---|---|
| `/tmp/vocoder-poc-run.txt` | log ทั้งหมด (progress + JSON) รวม warn ของ ort |
| `/tmp/vocoder-poc-result.json` | ผลตัวเลขทั้งหมดจากหน้าเว็บ |
| `/tmp/vocoder-poc.wav` | PCM 16-bit 44.1k ที่ vocoder สร้างจริง (เปิดฟังได้) |
| `/tmp/vocoder-poc-rss.txt` | peak RSS ของ process tree ตอนรัน |

ปรับพารามิเตอร์ได้: `N1=64 N2=97 ./web/run-vocoder.sh` (จำนวนเฟรมของ run A/B)
หรือเปิดหน้าเว็บตรง ๆ ผ่าน `bench-server.py`: `?n1=64&n2=97&meldc=-3&probe=-3,-5,-7,-9,-11&wav=0`

## 2. ข้อเท็จจริงของโมเดล (อ่านจากไฟล์ protobuf เอง — ไม่เชื่อเอกสาร)

ตรวจด้วยสคริปต์ protobuf parser เขียนเอง (ไม่มีแพ็กเกจ onnx ในเครื่อง):

| อย่าง | ค่าจริงจากไฟล์ |
|---|---|
| graph name | `torch_jit` · nodes 272 · initializers 213 |
| opset | `ai.onnx` 15 |
| input 1 | `mel` float32 **[1, n_frames, 128]** (dynamic) |
| input 2 | `f0` float32 **[1, n_frames]** (dynamic) |
| output | `waveform` float32 **[1, n_samples]** (dynamic) |
| op ที่ใช้ | Conv 93 · LeakyRelu 86 · Add 59 · Div 7 · Mul 5 · **ConvTranspose 5** · Slice 4 · Sub 2 · Pad 2 · Mod 2 · Transpose · Unsqueeze · CumSum · Reshape · Sin · Tanh · Squeeze |
| **hop** | **512** = iloczyn stride ของ ConvTranspose: 8·8·2·2·2 (kernel 16/16/4/4/4, pads 4/4/1/1/1) |
| sha256 | `b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d` |
| vocab | `vocoder.yaml`: `num_mel_bins: 128`, `pitch_controllable: true` |

⇒ ตรงกับสเปกที่โค้ด native ใช้ (`native/crates/moe-train/src/render.rs:199`: mel `[1,N,128]` + f0 `[1,N]` → audio `N*hop`)
และ `worldline-plugin/src/r2_engine.rs:191` ที่ `audio.truncate(total_frames * hop)`
f0 เป็น **Hz** (render.rs:136 `f0_norm * 500.0`) — POC จึงป้อน f0 186 Hz (ช่วง 148–224 Hz)

**ไม่มี custom op** ทุก op อยู่ใน opset 15 มาตรฐาน ⇒ wasm EP ของ ort-web รันได้โดยไม่ต้องมี EP พิเศษ

**ข้อจำกัดที่ต้องระบุตรง ๆ**: หน้าเว็บ (`web/vocoder-test.html`) **hardcode** `hop = 512` (และ `mel bins = 128`)
เป็นค่าคาดหวังของ assert — **ไม่ได้ derive จากโมเดลตอนรัน** ค่า 512 ยืนยันแยกจาก metadata จริงในตารางข้างบน
(ผลคูณ stride ของ ConvTranspose 5 ตัว = 8·8·2·2·2) ด้วยสคริปต์ protobuf parser เขียนเอง
ถ้าโมเดลเปลี่ยน hop assert `samples == n*hop` จะ fail ทันที (จึงยังเป็นด่านกัน mock ได้) แต่ตัวเลข 512 ในเอกสาร
ต้องอ่านว่า "ยืนยันจาก metadata แยกต่างหาก" ไม่ใช่ "คำนวณสดในหน้าเว็บ"

## 3. ตัวเลขที่วัดได้จริง

> **ขอบเขตของตัวเลข**: ค่าทั้งหมดใน §3 มาจาก **การรันครั้งเดียว (Run A รอบสะอาด)** ไม่ใช่ค่าเฉลี่ยหลายรอบ —
> เจตนาคือรายงานตัวเลขที่วัดได้จริง ไม่ใช่ค่าเฉลี่ยที่ดูดี ตัวเลข ms จึงควรอ่านเป็น "หลัก" ไม่ใช่ "หลักหน่วย"
> ส่วนที่ควรมั่นใจคือ determinism ของ output (WAV byte-identical) และสัดส่วน (per-frame cost) ซึ่งยืนยันข้ามรอบแล้ว

### 3.1 เวลาโหลด (local HTTP + wasm instantiate)

| ขั้น | เวลา | หมายเหตุ |
|---|---|---|
| import ort-web (ESM) | **16 ms** | 368 KB JS + loader shim |
| fetch model 56,616,643 bytes (HTTP จาก bench-server) | **49 ms** | ~1.1 GB/s (ดิสก์เดียวกัน/LAN) |
| `InferenceSession.create()` (wasm instantiate 14.2 MB + copy weights เข้า heap + graph opt) | **196 ms** | |
| **รวมโหลด → พร้อม inference** | **245 ms** | = fetch + session create (49 + 196) · **ไม่รวม** import ort-web · ถ้ารวม import = **261 ms** |

### 3.2 Inference (wasm EP · SIMD · `numThreads=1`)

| run | n_frames | samples | เวลา | realtime | peak | rms |
|---|---|---|---|---|---|---|
| run1-cold | 64 | **32,768** | 1,154.8 ms | 0.64× | 0.0065 | 0.0017 |
| run2-warm | 64 | **32,768** | 1,137.6 ms | 0.65× | 0.0065 | 0.0017 |
| run3-otherN | 97 | **49,664** | 1,621.8 ms | 0.69× | 0.0063 | 0.0017 |

- **hop 512 ยืนยันจากการวัด**: 32,768 = 64×512 และ 49,664 = 97×512 (ตรงกับผลจาก ConvTranspose strides)
- ต้นทุน ≈ **17.8 ms/frame** (n=64) และ ≈ 16.7 ms/frame (n=97) — งานต่อเฟรมแทบคงที่ (หัก overhead ต่อครั้งออก)
- **deterministic**: run1 vs run2 (input เดิม) max|diff| = 0 · และรันสคริปต์เต็ม **3 รอบของผู้เขียน (เปิด Chromium ใหม่ทุกครั้ง)
  + 2 รอบที่ตัวตรวจ `poc-verifier` รันเอง = 5 รอบ** ได้ไฟล์ WAV **byte-identical ทุกรอบ** —
  sha256 `de5622a36af22c17d9156378f39b6a2a7b5956342002e033727b8f63dbd30fdf` (fnv `1450536567`)
  ⇒ ไม่มี randomness ในกราฟ
- n=64 = 0.743 s audio ⇒ เพลง 3 นาที = 180/0.743 × 1.15 s ≈ **4.6 นาที** (wasm 1 thread)

### 3.3 ขนาดที่ต้องโหลดเข้าเบราว์เซอร์

| ของ | bytes | MB |
|---|---|---|
| `ort.min.mjs` | 367,950 | 0.35 |
| `ort-wasm-simd-threaded.mjs` | 24,381 | 0.02 |
| `ort-wasm-simd-threaded.wasm` | 14,239,897 | 13.58 |
| **รวม ort-web runtime** | **14,632,228** | **13.95** |
| `model.onnx` | 56,616,643 | 53.99 |
| **รวมที่ต้องดาวน์โหลด** | **71,248,871** | **67.9 MB** |

(ทั้งหมดเสิร์ฟจาก repo เองด้วย `web/vendor/fetch-ort.sh` — pin version + sha256 ไม่ได้ใช้ CDN ตอนรัน)

sha256 ของไฟล์ที่ vendor (ตรงกับ pin ใน `web/vendor/fetch-ort.sh` — **ตัวตรวจยืนยันแล้ว**):

| ไฟล์ | sha256 |
|---|---|
| `ort.min.mjs` | `0ca19c223d563f244908feb26712f40333980b07576328c8b4a04d132bbf34ad` |
| `ort-wasm-simd-threaded.mjs` | `e13f7f94fc51b4ca72b12faeb1ee95f4ace6dfbc8939bc718aabdc0a27c4299b` |
| `ort-wasm-simd-threaded.wasm` | `3398c10d07d229bd91b364548e130e0e51a8e5704b88c7c083ebbeb78842dee2` |

### 3.4 RAM — ต้องอ่านเป็น **delta เหนือ baseline** (วัดจาก process tree ของ Chromium ทุก 200 ms)

| ตัวชี้วัด | ค่าที่วัด | baseline (about:blank, วิธีเดียวกัน) | **delta** |
|---|---|---|---|
| peak ของผลรวม RSS ทั้ง tree (BFS รวมลูกหลาน) | 1,458.5 MB | 1,188.0 MB | **+270.4 MB** (ช่วงที่วัดได้ +270.4…+278.9) |
| RSS ของ process ที่ใหญ่สุด (renderer ที่โหลดโมเดลเข้า wasm) | 403.7 MB | 222.2 MB | **+181.5 MB** (ช่วง +185…+200 ในบางรอบ) |
| JS heap (used / limit) | 5.0 MB / 4,192 MB | — | (wasm memory **ไม่**นับในนี้) |
| จำนวน process ที่นับ (roots ที่ match profile dir) | 10 | — | — |

**คำเตือนเรื่องวิธีวัด**: ตัวเลข 1,458.5 MB คือ RSS ของ process tree **ทั้งก้อน** — ส่วนใหญ่คือ Chromium เอง
(แท็บเปล่าก็ ~1.19 GB) จึง **ไม่ควรถูกอ้างเป็น "RAM ของ vocoder"** ตัวเลขที่สื่อความหมายคือ delta ข้างบน:
**+270 MB (ทั้ง tree) หรือ +182 MB (renderer process เดียว)** สำหรับโมเดล 54 MB + ORT runtime 13.6 MB ≈ 3.1–3.3× ขนาดโมเดล
· และ **ห้ามเทียบ 1,458.5 MB กับตัวเลขของ T4 (flat sum 1,045.8 MB) ตรง ๆ** เพราะวิธีนับต่างกัน (BFS tree vs flat sum)
· POC นี้ยังไม่ได้แยก wasm linear memory ออกจาก V8 heap/decoder ของ process (task-5 วัด wasm linear memory ได้แล้ว
ด้วยการ hook `WebAssembly.Memory` — ดู `docs/planning/r2-wasm-poc.md`)

(baseline วัดโดยตัวตรวจ `poc-verifier` ด้วยวิธีเดียวกัน: tree_peak 1,188.0 MB / flat_peak 1,075.0 MB / process ใหญ่สุด 222.2 MB)

⇒ สรุปที่ยังใช้ได้: **ขนาดโมเดลไม่ใช่กำแพง แต่ RAM ต่อ process + compute เป็นกำแพงจริง** (ตรงกับ §1.3 ของ web-render-backend.md)

### 3.5 Probe: output ขึ้นกับ mel จริงไหม (mel scale ของโมเดลนี้ไม่มีเอกสาร)

mel ของ nsf_hifigan เป็น learned space — POC จึงไม่เดา scale แต่กวาด mel DC แล้วดูว่า output ตอบสนองไหม
(ป้อน f0 = 186 Hz เท่ากันทุกแถว):

| mel DC | peak | rms | zcr | dominant autocorr |
|---|---|---|---|---|
| -3 | 0.3031 | 0.0773 | 9337 Hz | 86 Hz (corr 0.41) |
| -5 | 0.0456 | 0.0120 | 9472 Hz | 86 Hz (corr 0.42) |
| **-7 (ที่ใช้ใน run A/B)** | 0.0065 | 0.0017 | 9013 Hz | 86 Hz (corr 0.29) |
| -9 | 0.0010 | 0.0002 | 9061 Hz | 86 Hz (corr 0.31) |
| -11 | 0.0001 | 0.00003 | 9119 Hz | 86 Hz (corr 0.31) |

**peak ต่างกัน 2,443 เท่า** เมื่อเปลี่ยนเฉพาะ mel ⇒ กราฟแปลง input จริง ไม่ใช่ passthrough/ค่าคงที่

ตัวตรวจ (`poc-verifier`) รัน probe เองอีก **2 รอบ** ได้ค่าตรงกันทุกหลัก:
`peak(melDc=-3) = 0.30309221148490906` → `peak(melDc=-11) = 0.00012406740279402584` = **2,443×**
⇒ ตัวเลขนี้ไม่ใช่ noise ของรอบเดียว
(หมายเหตุ: ค่า dominant autocorr = 86.13 Hz = 44100/512 = frame rate ⇒ เป็น artifact ของการ upsample
ไม่ใช่ pitch ของเสียง — รายงานไว้ตรง ๆ ไม่ตีความเป็น pitch)

## 4. POC นี้พิสูจน์อะไร / ไม่พิสูจน์อะไร

พิสูจน์ (มีหลักฐานใน JSON + WAV):
- **โหลดจริง**: หน้าเว็บอ่านไฟล์ 56,616,643 bytes และ sha256 ตรงกับไฟล์บนดิสก์ (เชลล์คำนวณเทียบเอง)
- **รันจริง**: `session.inputNames = [mel, f0]`, `outputNames = [waveform]` อ่านจาก graph จริง
- **ผลยาวถูกต้อง**: ทุก run ได้ `samples == n_frames * 512` (ทั้ง n=64 และ n=97) ⇒ hop 512 จริง
- **ไม่ใช่ของปลอม/ค่าคงที่**: finite ทั้งหมด, peak > 0, ค่ามีความหลากหลาย, และ WAV ถูก parse ซ้ำในเชลล์ (independent)
- **deterministic**: run1 vs run2 (input เดิม) max|diff| = 0 และ WAV byte-identical ทุกครั้งที่รัน
  (**3 รอบของผู้เขียน + 2 รอบของตัวตรวจ = 5 รอบ**, sha256 `de5622a36af22c17d9156378f39b6a2a7b5956342002e033727b8f63dbd30fdf`)
- **ตอบสนองต่อ input จริง**: สวิตช์ mel DC (-3 → -11) เปลี่ยน peak ของ output **2,443 เท่า**
  (0.30309221148490906 → 0.00012406740279402584, ตรงกันทั้ง 2 รอบที่ตัวตรวจรันเอง) ⇒ graph แปลง mel จริง
- **ตรวจอิสระแล้ว**: teammate `poc-verifier` รัน POC เอง 2 รอบ PASS, WAV sha256 ตรง, metadata ONNX ตรงทุกข้อ, ไม่พบ mock
  (รายละเอียด: `docs/planning/vocoder-wasm-verification.md`)

ไม่พิสูจน์ (ห้ามตีความเกิน):
- **คุณภาพเสียงเชิง perceptual** — mel ที่ป้อนเป็น mel สังเคราะห์ เพราะ mel ของ pc-nsf-hifigan เป็น
  learned space ที่ไม่มีสเกลเปิดเผย และไม่มี mel/f0 dump จริงใน repo (ตรวจแล้ว: ไม่มี `.npy`/dump)
- **ไม่มีการเทียบ sample-by-sample กับ native** สำหรับ vocoder ตัวนี้ (native ต้องมี mel จาก acoustic/MoE ก่อน)
- **pitch tracking ไม่ได้ถูกวัด**: autocorrelation ของ output ไปติดที่ 86.13 Hz = 44100/512 = **frame rate**
  (artifact ของการ upsample เป็นเฟรม) จึงไม่รายงานตัวเลข pitch ของ vocoder นี้ — ต้องใช้วิธีอื่น (เช่น YIN/cepstrum หรือเทียบสเปกตรัมกับ native)
- ยังเป็น **single-thread** (`numThreads=1` เพราะเซิร์ฟเวอร์ไม่มี COOP/COEP ⇒ ไม่มี SharedArrayBuffer)
  และยังไม่ได้ลอง WebGPU EP
- ไม่ได้วัดกับเพลงเต็ม/โมเดล acoustic 271 MB — POC นี้แตะเฉพาะ vocoder

## 5. บทเรียนที่เกี่ยวกับ worldline-R2

1. **ตัวโค้ด/ขนาดไม่ใช่กำแพง** — vocoder 56.6 MB + runtime 13.9 MB โหลดเข้า wasm และ inference ได้จริงใน 245 ms
   (§3.1 — ไม่รวม import ort-web 16 ms)
   แต่ RAM ที่เพิ่มจริงคือ **+270 MB เหนือ baseline** (§3.4) ยืนยันข้อสรุปเดิมใน `docs/planning/web-render-backend.md` §1.3 ว่ากำแพงจริงคือ RAM/address space
2. **โหลดเร็วมากเพราะเป็น local server** (ดู §3) — บน LAN/ดิสก์เดียวกัน การโหลดไม่ใช่ปัญหา แต่บน 4G ต้องรอ 68 MB
3. **compute คือกำแพง** — wasm 1 thread ได้ **0.64–0.69× realtime** (§3.2) ⇒ เพลง 3 นาทีใช้ ~4.6 นาที; ถ้าจะใช้ต้อง
   เปิด multi-thread (ต้องมี COOP/COEP) หรือ WebGPU EP แล้ววัดใหม่
4. **mel scale ของ vocoder นี้เป็น learned space** ⇒ ถ้าจะต่อสายพาน mel→audio บน wasm ต้องขน
   mel extractor (และค่า normalization ที่ถูกต้อง) มาด้วย ไม่ใช่แค่ vocoder.onnx

## 6. ข้อจำกัดการเขียนไฟล์

ไฟล์ที่แตะในงานนี้ (ใน write scope):
`web/vocoder-test.html` · `web/run-vocoder.sh` · `web/vendor/` (fetch-ort.sh + .gitignore + ort/ ที่ ignore) · `docs/planning/vocoder-wasm-poc.md`

ไม่ได้แตะ: `web/app/**`, `web/pool.js`, `web/real-test.html`, `web/run-real.sh`, `web/bench-server.py`, `native/**`
(ใช้ `bench-server.py` เป็นแม่แบบการเก็บผล headless แบบอ่านอย่างเดียว)
