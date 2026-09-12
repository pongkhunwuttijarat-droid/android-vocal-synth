# Lilt — web (wasm) target

Engine ทั้งตัวรันบน browser ด้วย `wasm32-unknown-unknown` — **ไม่ต้องมี
server, ไม่มี `.so`, ไม่มี dlopen**

## ไฟล์

| ไฟล์ | หน้าที่ |
|---|---|
| `index.html` | UI harness — import voicebank เข้า OPFS, โหลด `.ustx`, render + เล่น |
| `editor.html` | Browser editor vertical slice (Canvas piano roll + inspector + transport) |
| `app/driver.js` | Rust-owned project/command adapter สำหรับ editor |
| `app/roll.js` / `app/viewport.js` | Canvas interaction, note tools, selection และ zoom/pan |
| `app/editor.js` | ผูก editor UI กับ engine; ปัจจุบันยัง render บน main thread |
| `editor-test.html` | Headless vertical-slice test ของ editor |
| `engine.js` | adapter: wasm engine ↔ OPFS (อ่าน wav/frq, ส่ง bytes เข้า engine) |
| `samples.js` | `HttpBank` / `OpfsBank` — แหล่ง sample (cache + frq companion) |
| `worker.js` | render worker: wasm instance ของตัวเอง + โหลด sample แบบเพิ่มทีละ chunk |
| `pool.js` | **`RenderPool`** — pool + per-chunk loading + chunk cache (ผ่าน harness; รอ wire เข้า editor) |
| `pkg/` | wasm module + JS bindings (generated — `run-smoke.sh` สร้างใหม่) |
| `smoke.html` | headless test page (สร้างจาก script) |
| `fixture/` | voicebank subset + demo-song สำหรับ test (3.2 MB) |
| `run-smoke.sh` | build + serve + รัน headless Chromium แล้วเช็คผล |
| `run-pool.sh` | ทดสอบ RenderPool (bit-identical + cache + incremental) |
| `pool-test.html` | หน้าเทียบ pool กับ full render + ตรวจ incremental |
| `parallel.html` | หน้าเทียบ sequential vs Web Worker fan-out (ตรวจ bit-identical) |
| `run-parallel.sh` | ทดสอบ parallel แบบ headless |
| `bench-post.html` / `bench-server.py` | วัดเวลาแบบ real wall-clock (virtual-time ทำ `performance.now()` หยุด) |
| `pool-full.html` | เทียบ 4 โหมด (spawn/pool × ทั้ง bank/เฉพาะ chunk) กับ voicebank จริง |

## รัน

```sh
# 1. ทดสอบ headless (แนะนำ — ยืนยันว่า engine ทำงานจริงบน browser)
./web/run-smoke.sh

# 2. เปิด UI จริง
cd native && cargo build --release --target wasm32-unknown-unknown -p engine-wasm
wasm-bindgen --target web --out-dir ../web/pkg \
  target/wasm32-unknown-unknown/release/engine_wasm.wasm
cd ../web && python3 -m http.server 8901
# เปิด http://127.0.0.1:8901/
```

**ต้องเสิร์ฟผ่าน http(s)** — `file://` ใช้ WebAssembly/ES module ไม่ได้

## ผลที่คาดหวังจาก `run-smoke.sh`

```
wasm loaded
aliases=2673
samples=6 withFrq=6
render: 132066 samples in ... ms, peak=0.9884
wav bytes=264176 magic=RIFF
RESULT: PASS
```

`132066 samples / 264176 bytes / peak 0.9884` — **ตัวเลขเดียวกับ native**
(`world-oracle` / `synth-cli render`)

## API ที่ engine เปิดให้ JS

```ts
engine.set_voicebank(id, name, otoDir, otoIniBytes)  // สร้าง alias table
engine.wav_keys(): string                            // ต้องโหลด wav เหล่านี้
engine.add_sample_with_frq(key, wavBytes, frqBytes)  // แนะนำ (frq = ความตรง)
engine.add_sample(key, wavBytes)                     // ไม่มี frq (F0 estimate)
engine.alias_count(): number
engine.sample_count(): number
engine.render_project(ustxYaml, track): Float32Array
engine.render_project_wav(ustxYaml, track): Uint8Array
```

> **frq สำคัญ:** UTAU bank ส่ง `<stem>_wav.frq` คู่ทุก wav และ reference
> เลือกใช้เสมอ ถ้าไม่ส่ง ผลจะต่างจาก native (วัดได้ max|diff| 1.588 เทียบกับ
> 0.000e0 เมื่อส่ง)

## Parallel render (Web Worker)

wasm ในตัว engine เป็น single-threaded แต่ **host fan chunk ออกไปหลาย
Web Worker ได้** — ไม่ต้องใช้ `SharedArrayBuffer` / COOP-COEP headers:

```sh
./web/run-parallel.sh
# sequential  : 7051 ms  (2.06x realtime)
# workers=2   : 3768 ms  (3.85x realtime)  speedup 1.87x  bit-identical
# workers=4   : 2653 ms  (5.47x realtime)  speedup 2.66x  bit-identical
```

API: `engine.plan_project()` → `engine.chunk_meta_json()` →
`engine.render_chunk(i)` ต่อ worker → `Mixer.add_chunk()` + `finish()`
รายละเอียด: `docs/architecture/wasm-parallel.md`

### Pool กับ per-chunk loading

วัดกับ voicebank จริง (558 wav, 198 MB) แล้ว — **per-chunk loading คุ้มกว่า pool**:

```ts
const n = engine.plan_project(yaml, 0);
// worker i รับ chunk [i, i+W, i+2W, ...]
const keys = indices.flatMap((k) => engine.chunk_wav_keys(k).split("\n"));
// ส่งเฉพาะ keys เหล่านั้นให้ worker → worker พร้อมใน 27 ms แทน 625 ms
```

| โหมด | worker พร้อม | render (4 workers) | RAM ต่อ worker |
|---|---|---|---|
| โหลดทั้ง bank | 625 ms | 2537 ms | ~198 MB |
| **เฉพาะ chunk** | **27 ms** | **2159 ms** | ไม่กี่ MB |

ต่อให้ pool แล้ว การโหลดทั้ง bank ยังกิน ~790 MB รวม 4 workers —
บนแท็บเล็ตควรใช้ per-chunk

## RenderPool (production candidate; editor ยังไม่ wire)

```js
import { HttpBank } from "./samples.js";   // หรือ OpfsBank
import { RenderPool } from "./pool.js";

const bank = new OpfsBank({ root: "voicebank" });
const pool = await RenderPool.create({ workerCount: 4, bank, otoPath: "voice/oto.ini", otoDir: "voice", projectYaml });
await pool.render();                 // bit-identical กับ full render
await pool.setProject(editedYaml);
await pool.render();                 // render เฉพาะ chunk ที่เปลี่ยน
pool.toWav();                        // export จาก cache
```

| step | เวลา | chunks render ใหม่ |
|---|---|---|
| cold | 2217 ms | 4 (โหลด sample 2 จาก 558) |
| ไม่มีอะไรเปลี่ยน | **3 ms** | 0 |
| แก้ note เดียว | 2078 ms | 1 จาก 4 |
| export WAV | 5.2 ms | 0 |

ทดสอบ: `./web/run-pool.sh`
รายละเอียด + ประวัติการแก้ `Chunk::hash` ให้ครอบคลุม curves/pitch:
`docs/architecture/wasm-parallel.md`

> Update 2026-09-12: `Chunk::hash` ครอบ pitch/expression-curve window และมี
> regression tests แล้ว. `run-pool.sh` ยังผ่านแบบ bit-identical แต่หน้า
> `editor.html` ปัจจุบันใช้ `driver.renderAll()` บน main thread; import ของ
> `RenderPool` เป็นงาน wiring ถัดไป ไม่ควรตีความว่า editor ใช้ pool แล้ว.

## Browser editor checkpoint

`editor-test.html` ผ่าน headless Chromium โดยครอบ boot, DOM/canvas paint,
note hit-test, command edit, undo, Draw tool, marquee multi-select, audio render และ
cursor-anchored zoom. Demo ใช้ `fixture/scale-demo.ustx` กับ HTTP fixture bank.

## ยังไม่ทำ

- **Production editor wiring** — vertical slice มีแล้ว แต่ยังต้องต่อ RenderPool,
  progress/cancel, OPFS/project browser, autosave และ export flow
- **Feature breadth** — multi-track audio mix, expression lanes, authoritative
  phoneme labels และ tablet/pen hardware QA
- **Neural engines** (r2/diffsinger/moe) — ยัง native-only
- **Deploy** — ยังไม่มี service worker/PWA/static host config
