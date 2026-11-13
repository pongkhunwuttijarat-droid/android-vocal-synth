# Web Render Backend — แยก renderer ให้ต่อได้ทั้ง wasm และ server

> สถานะ: **แผน (ยังไม่เริ่ม)** · 2026-09-23
> ต่อจาก: [web-ui-design.md](web-ui-design.md) §7 · [wasm-migration-status.md](wasm-migration-status.md) · CONTRACT-v1 (`docs/CONTRACT-v1.md`)
> บันทึกที่มา: [BACKLOG.md](BACKLOG.md) §B

เป้าหมาย: ให้เว็บ editor เลือก **backend การ render** ได้ — `wasm` (Web Worker pool in-process,
ตอนนี้) หรือ `server` (remote `synth-server` ผ่าน HTTP แบบเดียวกับที่ Flutter app ใช้)
โดยที่โค้ด UI/roll/เอกสารไม่ต้องรู้ว่ากำลังใช้ตัวไหน

> **ข้อกำหนด (2026-09-23):** เมื่อต่อ server ได้แล้ว ต้อง **ใช้งาน render backend ได้ครบ** —
> ไม่ใช่โหมดลดฟีเจอร์. progress, แก้ 1 note = render 1 chunk, cancel, export ทุกอย่างต้องทำงาน
> เหมือนโหมด wasm (ดู §1.1)
>
> **"server" ในเอกสารนี้ = `native/tools/synth-server` ตัว native (Rust + `.so`/ONNX) เท่านั้น**
> ไม่ใช่ server ที่รัน wasm — เครื่องที่รัน server มีโมเดล/plugin ครบ ส่วนบราวเซอร์เป็นแค่ client

---

## 1. หลักการ: planner อยู่ local, renderer สลับได้

จุดสำคัญที่ทำให้เรื่องนี้ง่ายกว่าที่คิด: **การวางแผน chunk ไม่ต้องใช้เสียงเลย**

| งาน | อยู่ที่ | หมายเหตุ |
|---|---|---|
| เอกสาร (`.ustx`), command, undo/redo | wasm `Engine` ในบราวเซอร์ | ต้อง local เสมอ (offline + undo) |
| **plan chunk** (`plan_project` / `chunk_meta_json` / `chunk_wav_keys` / `plan_skipped`) | wasm `Engine` ในบราวเซอร์ | ใช้แค่ alias table จาก `oto.ini` — ไม่แตะ sample |
| **ผลิต PCM** | **backend ที่เลือก** | wasm pool หรือ HTTP server |
| เล่นเสียง + playhead | Web Audio | ไม่เปลี่ยน |
| เก็บโปรเจกต์ | OPFS (`app/project-store.js`) | ไม่เปลี่ยน |

ผลคือ chunk overlay, ตัวเลข chunk, skip report และ cache key ฝั่ง UI **เหมือนกันทั้งสอง backend**
— server มีหน้าที่แค่คืน PCM

### 1.1 Parity — สองโหมดต้องได้เท่ากัน

| ความสามารถ | wasm pool | server | ต้องทำอะไร |
|---|---|---|---|
| render ทั้งเพลง | ✅ | ✅ (v1) | S2 (`project_yaml`) |
| plan / chunk overlay / skip report | ✅ (planner local) | ✅ (planner local ตัวเดียวกัน) | ไม่ต้องทำ — ใช้ planner ในบราวเซอร์ |
| **progress ต่อ chunk** | ✅ `onProgress` | ❌ v1 (indeterminate) | **S3** (`/plan` + `/render-chunk`) |
| **แก้ 1 note → render 1 chunk** (chunk cache) | ✅ | ❌ v1 (ทั้งเพลง) | **S3** + cache key `engineSalt:hash` |
| cancel | ✅ token | ✅ `POST /cancel` + epoch guard | S1 (CORS) |
| export WAV | ✅ `toWav()` (จาก cache) | ✅ ใช้ WAV ที่ server คืน / ประกอบจาก chunk cache | — |
| mixer / post-fx | ✅ ใน wasm | ต้องใช้ `POST /post-fx` (มีแล้ว) | wire เข้า backend |
| phoneme labels | ✅ local wasm | ✅ ใช้ local ก็ได้ (ไม่กิน server) | — |
| เลือก engine | เฉพาะที่มีใน wasm (r1-rs) | **ครบกว่า** — `r1`, `r1-rs`, `classic`, `r2`, `diffsinger`, `moe` | UI เลือก engine ต่อ backend |
| offline | ✅ | ❌ ต้องมีเน็ต/LAN | เก็บ wasm เป็นค่าเริ่มต้น |

⇒ **v1 (whole-project POST) เป็น stepping stone เท่านั้น** — จะประกาศว่าโหมด server "ใช้งานได้"
ต้องผ่าน S3 ให้ parity ครบก่อน (ดูเกณฑ์ผ่าน step 3)

### 1.2 Engine ที่แต่ละโหมดให้ได้

| engine | wasm (in-process) | native server | หมายเหตุ |
|---|---|---|---|
| `r1-rs` (pure-Rust WORLDLINE-R) | ✅ | ✅ | ตัวที่ทำให้ **bit-identical** กันได้ (`cmp` ผ่านแล้ว) |
| `r1` (WORLDLINE-R ผ่าน `.so`) | ❌ | ✅ | wasm ไม่มี dlopen |
| `classic` (`.so`) | ❌ | ✅ | ยังไม่ port เป็น Rust |
| `r2` (mel→HiFiGAN ONNX) | ⚠️ ทำได้แต่ยังไม่คุ้ม | ✅ | ดู §1.3 |
| `diffsinger` | ⚠️ ทำได้แต่ยังไม่คุ้ม | ✅ | โมเดล 271 MB + vocoder 56 MB |
| `moe` (burn) | 🟡 compile ผ่าน (ยังไม่รันจริง) | ✅ | MoE student `.mpk` 8–40 MB (bigset) — **vocoder ไม่มีเวอร์ชัน burn**: `pc-nsf-hifigan.mpk` (6 MB) ที่เข้าใจกันว่าใช่ จริง ๆ เป็น acoustic รุ่นเก่า ดู §1.4 |

⇒ **neural = native server**: งาน neural ทั้งหมดวิ่งบนเครื่องที่รัน server บราวเซอร์ไม่ต้องโหลด
โมเดลร้อย MB; ส่วน wasm เก็บไว้สำหรับ classic path ที่ทำงาน offline ได้

### 1.3 Neural บน wasm — ผล spike 2026-09-23

ลอง build จริงบนเครื่องนี้ (target `wasm32-unknown-unknown`):

| ลอง | ผล | เงื่อนไข |
|---|---|---|
| `cargo check -p diffsinger --target wasm32-unknown-unknown` (ตรง ๆ) | ❌ | `ort-sys: no prebuilt binaries available for target wasm32-unknown-unknown` — เพราะ `ort` เปิด default feature `download-binaries` |
| `ort 2.0.0-rc.13` แบบ `default-features = false` | ✅ **compile ผ่าน** | ใช้ `alternative-backend` → runtime มาจาก **onnxruntime-web (JS)** ที่โหลดในหน้าเว็บ |
| `burn 0.21` (`ndarray`, `default-features = false`) | ✅ **compile ผ่าน** | ต้องมี `getrandom = { version = "0.4", features = ["wasm_js"] }` **เท่านั้น** — ยืนยันซ้ำแล้วว่าไม่ต้องตั้ง `RUSTFLAGS='--cfg getrandom_backend="wasm_js"'` (ค่านั้นไม่อยู่ใน check-cfg ของ getrandom 0.4.3) |

สรุป: **ตัวโค้ดไม่ใช่กำแพง และขนาดดาวน์โหลดก็ไม่ใช่กำแพงหลัก** (327 MB โหลดครั้งเดียว + cache ได้
บน LAN ไม่กี่วินาที) — ที่เป็นกำแพงจริงคือ
1. **RAM/address space ตอนรัน** — โมเดลต้องเข้า wasm heap ทั้งก้อน + arena ของ ORT-web (มัก 1.5–2×)
   + activation (DiffSinger เป็น diffusion หลาย step) ⇒ แท็บเดียวอาจแตะหลายร้อย MB ถึงระดับ GB
   บนแท็บเล็ต/Safari เพดานต่ำกว่ามาก → เสี่ยง OOM
2. **compute** — ORT-web ยังไม่มี execution provider ที่แรงเท่า native (WebGPU EP ยังใหม่)
3. **ไม่ได้อะไรกลับมา** — เครื่องที่รัน server อยู่ใกล้ ๆ (LAN) มีโมเดล + runtime + EP ครบแล้ว

⇒ ถ้าจะทำ neural บน wasm จริง องค์ประกอบแยกกัน: **acoustic/mel** ใช้ burn ได้ (MoE student 8–40 MB)
แต่ **vocoder ยังเป็น ONNX** ⇒ ต้องพก ort-web (แบบที่ POC ด้านล่างพิสูจน์แล้ว) หรือแปลง ONNX→burn ด้วย `burn-import` (ยังไม่ได้ทำ)
หรือพิสูจน์สายพานด้วย **fake model 260 bytes** ที่มีอยู่แล้ว (`test/golden/diffsinger-fake`) ก่อน — ไม่ใช่ DiffSinger ตัวเต็ม

**POC จริงแล้ว (2026-09-23, `./web/run-vocoder.sh`)** — รัน nsf_hifigan (56.6 MB) ด้วย **onnxruntime-web 1.30.0**
(wasm EP, SIMD) ใน headless Chromium:

| วัด | ค่า |
|---|---|
| โหลดรวม | **244 ms** (import ort 15 + fetch 51 + session create 193) |
| inference | n=64 → 0.743 s audio ใน 1,075 ms = **0.69× realtime** |
| hop / sr | 512 / 44100 (ยืนยันจาก metadata + ConvTranspose strides) |
| ขนาด | ort-web 13.95 MB + โมเดล 53.99 MB = **67.9 MB** |
| RAM | peak process tree **1,457 MB** (process ใหญ่สุด 404 MB = wasm heap) |
| determinism | WAV byte-identical 3 รอบ (sha256 `de5622a3…`) |

⇒ **vocoder รันในเบราว์เซอร์ได้จริง** และ **กำแพงไม่ใช่ขนาดโหลด (244 ms) แต่เป็น RAM + compute**
— ข้อจำกัดที่ผู้เขียนรายงานตรง ๆ: mel ที่ป้อนเป็น **mel สังเคราะห์** (learned space ไม่มีสเกลเปิดเผย)
จึงพิสูจน์ได้แค่ "โหลด+รันจริง/เวลา/RAM/ความยาว" ไม่ได้พิสูจน์คุณภาพเสียง ⇒
[vocoder-wasm-poc.md](vocoder-wasm-poc.md)

### 1.4 ฝั่ง Rust พร้อมแค่ไหน (T2, 2026-09-23)

รายละเอียด/หลักฐานเต็ม: [wasm-r2-poc.md](wasm-r2-poc.md) — สรุป:

| เรื่อง | ผลจริง |
|---|---|
| `ort` บน wasm32 | default features **ล้ม** (`ort-sys: no prebuilt binaries available for target wasm32-unknown-unknown`; `dist.tsv` ไม่มีแถว wasm32) → ต้อง `default-features=false` + `alternative-backend`, runtime = ort-web + asset ของ onnxruntime-web |
| **กำแพงจริงของ ort บน wasm** | ort-web **ไม่มี sync `Session::run`** และ **ไม่มี `commit_from_file`** ⇒ `moe-train/src/render.rs` ที่เป็น sync ใช้ซ้ำบน wasm ไม่ได้ ต้อง port เป็น `run_async` |
| `moe-train` → wasm32 | ✅ `cargo check -p moe-train --target wasm32-unknown-unknown --no-default-features` ผ่าน (ort ถูก feature-gate) และ native ไม่พัง (`cargo test -p moe-train` 5 passed) |
| เกินโจทย์ | build example ได้ `.wasm` จริง **16.4 MB** (debug) export `moe_forward_probe` — แต่ instantiate ตรง ๆ ไม่ได้ ต้องมี wasm-bindgen glue (import `__wbg_getRandomValues_*`) |
| `pc-nsf-hifigan.mpk` (6 MB) | **ไม่ใช่ vocoder** — เป็น burn checkpoint ของ `ContextAcoustic` รุ่นเก่า (WINDOW=5): 7 key, 1,544,320 พารามิเตอร์; โหลดเข้า layout ปัจจุบันคืน OK (burn ไม่ตรวจ shape) แต่ forward **panic** ⇒ ใช้เป็น burn vocoder ไม่ได้ |
| ยังไม่พิสูจน์ | ยังไม่มีการรัน wasm จริง (ไม่มีเสียง/ไม่มีเวลา-RAM), ยังไม่มี crate `moe-wasm` (ต้องเพิ่ม workspace member) |

**ตัวเลขวัดจริงบนเครื่องนี้ (2026-09-23, `synth-cli --engine diffsinger`, scale-demo 29 notes → 14.5 s audio):**

| โมเดล | wall time | peak RSS | ผลลัพธ์ |
|---|---|---|---|
| fake (260 bytes) | 0.0–0.1 s | 50 MB | PCM ขยะ (peak เกิน 1.0) |
| **kiritan จริง (acoustic 271 MB + vocoder 56 MB)** | **57.0 s** | **645 MB** | PCM ใช้ได้ (peak 0.64) |

อ่านตัวเลข: native ONNX CPU = ช้ากว่า realtime ~3.9× และ **RSS ≈ 2× ขนาดโมเดล** (327 MB → 645 MB)
⇒ ORT-web บน wasm (SIMD, มักไม่กี่ thread) จะช้ากว่านี้อีก ~2–5× = **หลายนาทีต่อเพลง 14.5 วิ**
และต้องแบกโมเดลทั้งก้อนใน linear memory ⇒ **เดสก์ท็อปทำ demo ได้ · แท็บเล็ต/มือถือไม่ไหว**
และไม่คุ้มเมื่อ native server อยู่ LAN เดียวกัน (งาน neural ควรเป็นงานของเครื่อง server)

---

## 2. Interface ที่ต้องมี (`web/app/render-backend.js`)

```js
/**
 * @typedef {Object} RenderBackend
 * @property {string} id            "wasm" | "server"
 * @property {string} label         ชื่อที่โชว์ใน UI
 * @property {string} engineSalt    id ของ engine ที่ใช้ทำ cache key ("r1-rs", "r1", ...)
 * @property {Array}  plan          chunk meta จาก planner local (start_ms/end_ms/hash/index)
 * @property {Object} stats         { lastRendered, lastReused, lastRenderMs, ... }
 *
 * @property {(yaml: string, track: number, opts?: {force?: boolean}) => Promise<{chunks:number, dirty:number, skipped:string[]}>} setProject
 * @property {(opts?: {signal?: {cancelled:boolean}, onProgress?: (p:{rendered:number,total:number}) => void}) => Promise<{samples: Float32Array|null, cancelled?: boolean, stats: object}>} render
 * @property {() => Promise<void>|void} cancel      ยกเลิกงานที่ค้าง (ผลถูกทิ้ง ไม่ publish)
 * @property {() => Uint8Array} toWav               export จากสิ่งที่ cache ไว้
 * @property {() => void} dispose                   คืน resource (terminate workers / abort request)
 */
```

สัญญาเหมือน `RenderPool` วันนี้เป๊ะ (ตั้งใจ) — `render()` คืน `samples: null` เมื่อถูกยกเลิก
และห้าม publish อะไรลง cache เมื่อ `signal.cancelled`

### 2.1 `WasmRenderBackend`
คือ `RenderPool` เดิม + adapter บาง ๆ (worker pool, per-chunk loading, chunk cache)
ส่วน main-thread path (`driver.renderAll()`) ที่เป็น fallback ตอน Worker ใช้ไม่ได้ เก็บไว้ใน
backend นี้ (ลองสร้าง pool → fail → ถอยเป็น main thread)

### 2.2 `ServerRenderBackend`
```js
new ServerRenderBackend({ baseUrl, voicebank, engine: "r1-rs", planner /* wasm Engine */ })
```

- `setProject` — แค่จำ YAML + ให้ planner วางแผน (ไม่ยิง request)
- `render` — `POST /render` → ได้ `audio/wav` (16-bit) → `decodeWav16()` → Float32Array
- `cancel` — `POST /cancel` + set `signal.cancelled` (ผลที่มาถึงทีหลังถูกทิ้งเพราะ epoch guard เดิม)
- `toWav` — คืน WAV ที่ได้จาก server ตรง ๆ (ไม่ต้อง quantize ซ้ำ)
- `engineSalt` — ต้องตรงกับ engine ฝั่ง server (ค่าเริ่มต้น server = `r1` ใช้ `.so`, `r1-rs` = kernel เดียวกับ wasm)

ไฟล์ใหม่: `web/app/wav.js` (`decodeWav16`) ตามชื่อที่ `web-ui-design.md` §7 วางไว้แล้ว

---

## 3. ต้องแก้ฝั่ง server (Rust, `native/tools/synth-server`)

ปัจจุบัน: `/render` รับ `{project: "<absolute path ฝั่ง server>", voicebank, track, engine}`
และ **404 ถ้าไฟล์ไม่มี** · ไม่มี CORS · ไม่มี `/plan`

| # | งาน | รายละเอียด | ความเสี่ยง |
|---|---|---|---|
| S1 | **CORS** | middleware เพิ่ม `Access-Control-Allow-Origin` (ค่าเริ่ม `*`, override ด้วย `--cors <origin>`), allow `GET/POST/DELETE/OPTIONS`, allow header `content-type`, ตอบ `OPTIONS` 204 | ต่ำ — ไม่แตะ handler เดิม |
| S2 | **`project_yaml` inline** | `RenderRequest` เพิ่ม `#[serde(default)] project_yaml: Option<String>`; ถ้ามี ให้เขียน temp `.ustx` (ใต้ `--projects`/`temp`) แล้วเข้าเส้นทางเดิม — `project` path ยังใช้ได้เหมือนเดิม (backwards compatible) | ต่ำ — `pipeline::parse_project` มีอยู่แล้ว |
| S3 | **`GET /plan` + `POST /render-chunk`** (**จำเป็นสำหรับ parity** ไม่ใช่ optional) | เปิด chunk plan (`chunk_meta_json`) และ render ทีละ chunk ให้ client — ใช้ `plan_track_chunks`/`render_chunk` ที่มีอยู่แล้วใน `render_service` | กลาง — ต้องจัดการ state ของ plan ต่อ request (`plan_id` + cache ฝั่ง server) |
| S4 | **`--static <dir>`** (ทางเลือก deploy) | ให้ server เสิร์ฟ `web/` เอง → origin เดียว ไม่ต้องพึ่ง CORS | ต่ำ |
| S5 | **(ไว้ทีหลัง) auth + progress** | token สำหรับ LAN, SSE/WS สำหรับ progress จริง | สูง — ยังไม่ทำ |

หมายเหตุ S3: ถ้ามี `/plan` แล้ว client เทียบ plan กับ planner local ได้ (ควรตรงกันเพราะใช้ crate
เดียวกัน) — ถ้าไม่ตรงให้ fallback ไปโหมด whole-project

---

## 4. ต้องแก้ฝั่งเว็บ

| # | ไฟล์ | งาน |
|---|---|---|
| W1 | `web/app/render-backend.js` (ใหม่) | interface + `WasmRenderBackend` (adapter ของ `RenderPool`) |
| W2 | `web/app/server-backend.js` (ใหม่) | `ServerRenderBackend` (fetch + decode WAV + cancel + health) |
| W3 | `web/app/wav.js` (ใหม่) | `decodeWav16(bytes) → Float32Array`, `encodeWav16()` (ใช้ตอน export ถ้าจำเป็น) |
| W4 | `web/app/editor.js` | เปลี่ยน `_renderWithPool()` → `_renderWithBackend()`; `this.pool` → `this.backend`; cache key = `${engineSalt}:${hash}`; สลับ backend = mark dirty + re-render เต็ม |
| W5 | `web/editor.html` | toolbar: `<select id="engineSel">` (wasm / server) + ช่อง URL server + จุดบอกสถานะ health (`GET /health`: `so_loaded`, `version`) + รายการ voicebank จาก `GET /voicebanks` |
| W6 | `web/app/settings.js` (ใหม่, เล็ก) | เก็บ URL/โหมดที่เลือกใน `localStorage` |
| W7 | เทสต์ | `web/run-server.sh` + `web/server-test.html` |
| W8 | docs | อัปเดต `web/README.md`, `web-ui-design.md` §7, `wasm-migration-status.md` |

UI ต้องมี error state ที่ผู้ใช้เห็นทุกกรณี (ต่อ server ไม่ได้ / voicebank ไม่มีบน server /
render ล้มเหลว) ตาม CONTRACT-v1 §4.6

---

## 5. ประเด็นที่ต้องตัดสิน/ระวัง

| เรื่อง | แนวทาง |
|---|---|
| **Quantization** | server คืน WAV 16-bit แต่ pool คืน float ก่อน quantize → **ผลไม่เหมือนกันเป๊ะ** ถ้าต้องการ bit-identical ให้ตั้ง `engine: "r1-rs"` (kernel เดียวกับ wasm) แล้ว `cmp` ได้ |
| **Cache key** | ต้องมี `engineSalt` (r1 กับ r1-rs ให้ chunk hash ต่างกัน — เหมือนที่แอป Flutter ต้อง re-render เต็มเมื่อสลับ engine) **และต้องรวม identity ของโมเดล** ไม่ใช่แค่ชนิด engine — ดูบั๊กที่เจอ: BACKLOG "cache salt ไม่ผูกกับโมเดล" |
| **Progress** | v1: indeterminate ("กำลัง render ที่ server…") + `/cancel`; v2 (`/plan`+`/render-chunk`): progress/cache/incremental เท่ากับ wasm |
| **Voicebank** | server resolve จากชื่อ/พาธของตัวเอง — client ต้องส่ง id ที่ server รู้จัก (เลือกจาก `/voicebanks`) ไม่งั้น error ชัด ๆ |
| **Offline** | โหมด server ต้องมีเน็ต/LAN; โหมด wasm ทำงาน offline ได้ → เก็บ wasm เป็นค่าเริ่มต้น |
| **Timeout** | เพลงยาว render เป็นนาที → ต้องมี timeout/abort (`AbortController`) และบอกผู้ใช้; อนาคตค่อยทำ streaming progress |
| **Concurrency** | `/cancel` ฝั่ง server เป็น flag ก้อนเดียว → เหมาะกับผู้ใช้คนเดียว/เครื่องเดียว (LAN dev) |

---

## 6. ลำดับงาน + เกณฑ์ผ่าน

| Step | งาน | เกณฑ์ผ่าน |
|---|---|---|
| **0** | W1: แยก interface + `WasmRenderBackend` (พฤติกรรมเดิมเป๊ะ) | `./web/run-editor.sh` ผ่านเหมือนเดิม (0/6 chunk, 1/6 chunk, cancel) — ไม่มี behavior change |
| **1** | S1+S2: CORS + `project_yaml` | `cargo test -p synth-server` ผ่าน + `curl` ข้าม origin ได้ + render จาก YAML ตรงกับ render จากไฟล์ (`cmp`) |
| **2** | W2/W3/W5 + `run-server.sh` | เพลงจริง (`sakura-test.ustx` + bank 558 wav) render ผ่าน HTTP จาก browser แล้ว **`cmp` bit-identical** กับผล wasm/native (`engine=r1-rs`) |
| **3** | S3 + per-chunk cache ฝั่ง client (**ปิด parity**) | แก้ 1 note → server ถูกขอ **1 chunk** (ไม่ใช่ทั้งเพลง) + progress วิ่งจริง + cancel กลางทางได้ + export จาก cache ได้ — เท่ากับโหมด wasm |
| **4** | S4 (optional) + deploy | เปิด `synth-server --static web/` แล้วใช้งานได้จาก origin เดียว |

ประมาณการ: step 0 ≈ ครึ่งวัน · step 1 ≈ ครึ่งวัน · step 2 ≈ 1 วัน · step 3 ≈ 1–2 วัน (ทั้งหมดยังไม่เริ่ม)

---

## 7. สิ่งที่ยังไม่ทำในรอบนี้

- auth/token, multi-user, cloud sync
- streaming/SSE progress (ต้องมีก่อนใช้กับเพลงยาวมาก ๆ)
- ให้ server เป็นเจ้าของเอกสาร (ตอนนี้เอกสารอยู่ wasm/OPFS เสมอ — ตั้งใจ)
- neural engine (`r2`/`diffsinger`/`moe`) ผ่าน server: ใช้ได้อยู่แล้วถ้า server เปิด ONNX แต่ UI ยังไม่มีตัวเลือก
