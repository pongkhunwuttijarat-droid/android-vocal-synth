# WASM Neural POC — สรุปปิดงาน (2026-09-23)

> ทีม: `vocoder-web` (T1/T5), `r2-wasm` (T2/T4/T7), `poc-verifier` (T3/T6) · Lead ตรวจรับเองรอบสุดท้าย
> คำถามตั้งต้น: *"wasm vocoder → worldline-R2 ทำได้ไหม"* · และ *"native vs wasm เร็วต่างกันแค่ไหน"*

---

## 1. คำตอบสั้น

**ได้ทั้งหมด** — vocoder, MoE acoustic (burn) และ R2 chain เต็ม รันในเบราว์เซอร์ได้จริง
ตัวเลขตรงกับ native ในระดับ **≤1 LSB (quantize) / ~1e-8 (float)** แต่ **ช้ากว่า native ~1.6–1.8× ต่อ 1 thread** และ **RAM ของ wasm เองเล็ก** (ส่วนที่ดูเหมือน 1+ GB คือตัวเบราว์เซอร์)

---

## 2. ผล 4 POC (ตรวจรับโดย Lead + ตรวจอิสระโดย `poc-verifier`)

| POC | รันอะไรจริง | ตัวเลขหลัก | Verdict | เอกสาร |
|---|---|---|---|---|
| **T1 vocoder** | nsf_hifigan (54 MB) ด้วย onnxruntime-web 1.30.0 (wasm EP) | โหลด 244 ms (261 รวม import) · **0.65–0.70× realtime** · hop 512 · deterministic (5 รอบ sha `de5622a3…`) | ✅ | [vocoder-wasm-poc.md](vocoder-wasm-poc.md) |
| **T4 MoE/burn** | checkpoint จริง 7.2 MB โหลด+forward ด้วย burn-ndarray **ใน wasm** (โมดูล 3.0 MB) | โหลด 10.1 ms · linear memory 1.1 → 26.4 → 34.9 MB · determinism **0** · vs native rel mean 1.2e-8 | ✅ | [wasm-moe-run.md](wasm-moe-run.md) |
| **T5 R2 chain** | **mel.onnx (85 KB) → pc-nsf-hifigan (56.6 MB)** ครบสายในเบราว์เซอร์ | chain 1,356 ms @64 เฟรม (**0.55× realtime**) · vocoder = **97–99% ของเวลา** · parity vs native CPU EP **max\|diff\| = 1 LSB (148/32,768)** | ✅ | [r2-wasm-poc.md](r2-wasm-poc.md) |
| **T2 readiness** | feature-gate `ort` ให้ `moe-train` compile wasm32 + ตรวจ `.mpk` | `cargo check --target wasm32 --no-default-features` ผ่าน · native tests 5/5 · `.mpk` 6 MB = acoustic รุ่นเก่า **ไม่ใช่ vocoder** | ✅ | [wasm-r2-poc.md](wasm-r2-poc.md) |
| **T3/T6 ตรวจอิสระ** | รันซ้ำทุก harness + เขียน parser/decode เอง | ผ่านทุกเกณฑ์ + แก้ตัวเลขเรา 2 ตัว (RAM, timing) + รัน wasm MoE เป็นครั้งแรก | ✅ | [vocoder-wasm-verification.md](vocoder-wasm-verification.md) |

คำสั่งเดียวต่อ POC: `./web/run-vocoder.sh` · `./web/run-moe.sh` · `./web/run-r2.sh` (ทุกตัว headless, PASS = exit 0)

---

## 3. ข้อค้นพบข้ามงาน (ที่แก้ความเข้าใจเดิม)

1. **vocoder ของ R2 = ไฟล์ byte-identical กับที่ T1 ใช้** (`b297c0c7…`) ⇒ ชิ้นที่ขาดสำหรับ R2 คือ **mel producer** (`mel.onnx` 85 KB จาก OpenUtou ref) ไม่ใช่ตัว vocoder
2. **ข้ออ้าง ".mpk 6 MB = vocoder" ผิด** — เป็น checkpoint ของ `ContextAcoustic` รุ่นเก่า (WINDOW=5, 1,544,320 params) โหลดเข้า layout ปัจจุบันได้แต่ forward **panic**
3. **RAM: ตัวเลข process-tree เกือบทั้งหมดคือตัวเบราว์เซอร์** — baseline `about:blank` วัดได้ tool 1,075–1,188 MB ⇒ ต้องรายงานเป็น **delta** หรือ **wasm linear memory**
   - T1 delta = **+270–279 MB (tree)** / **+185–200 MB (renderer)** · T5 delta = **+283 MB** · wasm linear memory (T5) = **160 MB** = 2.96× โมเดล
   - ⚠️ วิธีวัด T1 (BFS tree) กับ T4 (flat sum) ต่างกัน ⇒ ห้ามเทียบข้ามกัน
4. **เวลาไม่ reproducible** — รอบต่างกันได้ 1.6–1.8× และ native spread ถึง 61% ⇒ ทิศทาง/อัตราส่วนใช้ได้ แต่ค่าสัมบูรณ์เป็น **indicative** ไม่ใช่ benchmark
5. **บทเรียน orchestrator**: การเพิ่ม workspace member ก่อนเขียน manifest ให้ครบ ทำให้ `cargo` ทั้ง workspace ล้ม (exit 101) ⇒ เขียนไฟล์ให้ครบก่อนแตะ manifest เสมอ

---

## 4. ข้อจำกัดที่ยังเหลือ

- **parity เป็น ORT-vs-ORT** (wasm EP vs native CPU EP) ไม่ใช่ wasm-vs-WORLD-features จริง; input เป็น features สังเคราะห์
- browser ยัง **สร้าง WORLD features (f0/sp_env/ap) เองไม่ได้** ⇒ R2 บนเว็บยังต้องพึ่ง native สำหรับ analysis
- **`numThreads=1`** เพราะไม่มี COOP/COEP ⇒ ยังไม่เคยวัด 4 threads/WebGPU
- `hop=512` / mel bins 128 / sp_size เป็นค่าที่ hardcode ในหน้า harness
- T5 ยังไม่เทียบ native R2 WAV เต็มเส้นทางแบบ sample-by-sample

---

## 5. ข้อเสนอถัดไป (เรียงตามความคุ้ม)

1. **เปิด COOP/COEP** แล้ววัด `ort-web numThreads=4` + WebGPU → เป็นทางเดียวที่ปิด gap 10× ของ vocoder ได้ทันที
2. **ต่อ `world-rs` (pure-Rust WORLD ที่มีอยู่แล้วใน repo) → mel.onnx → vocoder** เพื่อ R2 ที่ไม่พึ่ง server เลย (งานใหม่ ก้อนใหญ่)
3. เปิด **`burn` simd/threads** ให้ MoE wasm (ตอนนี้ single-thread ไม่เปิด simd → ช้ากว่า native 8.9–15.8×)
4. ใช้เส้นทางที่ได้ผลแล้ว: **native server เป็น renderer + browser เป็น UI** (ตาม [web-render-backend.md](web-render-backend.md)) เพราะ neural ยังเป็นงาน pre-render (0.55× realtime) ไม่ใช่ interactive

---

## 6. Reproduce

```sh
# โมเดล (ไม่ commit — ดู models/r2/PROVENANCE.md)
#   mel.onnx จาก ref(openutau+openutau mobile)/core/OpenUtau.Core/Classic/Data/mel.onnx
#   pc-nsf-hifigan จาก registry: https://openutau.github.io/svs-index/registry/v1/softwares/all.json
./web/run-vocoder.sh   # T1: vocoder เดี่ยว
./web/run-moe.sh       # T4: MoE/burn ใน wasm
./web/run-r2.sh        # T5: R2 chain เต็ม + parity
cd native && cargo test -p moe-train -p moe-wasm -p engine-wasm
```
