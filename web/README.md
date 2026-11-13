# Rusted UTAU — web (wasm) target

Engine ทั้งตัวรันบน browser ด้วย `wasm32-unknown-unknown` — **ไม่ต้องมี
server, ไม่มี `.so`, ไม่มี dlopen**

## ไฟล์

| ไฟล์ | หน้าที่ |
|---|---|
| `index.html` | UI harness — import voicebank เข้า OPFS, โหลด `.ustx`, render + เล่น |
| `editor.html` | Flutter-style browser editor shell (Canvas piano roll + inspector + R1/WASM transport) |
| `app/driver.js` | Rust-owned project/command adapter สำหรับ editor |
| `app/roll.js` / `app/viewport.js` | Canvas interaction, note tools, selection และ zoom/pan |
| `app/editor.js` | ผูก editor UI กับ engine + **RenderPool** (มี main-thread fallback) และ project/autosave |
| `app/project-store.js` | **OPFS project store** — `projects/<name>.ustx`, last-opened sidecar, `storage.persist()` |
| `app/r2-runtime.js` | โหลด R2 ONNX sessions ซ้ำใช้ได้: WebGPU-first ทั้ง mel และ vocoder, WASM fallback |
| `editor-test.html` | Headless vertical-slice test ของ editor |
| `engine.js` | adapter: wasm engine ↔ OPFS (อ่าน wav/frq, ส่ง bytes เข้า engine) |
| `samples.js` | `HttpBank` / `OpfsBank` — แหล่ง sample (cache + frq companion) |
| `worker.js` | render worker: wasm instance ของตัวเอง + โหลด sample แบบเพิ่มทีละ chunk |
| `pool.js` | **`RenderPool`** — pool + per-chunk loading + chunk cache + cancel/progress (**editor ใช้ตัวนี้เป็น renderer หลักแล้ว**) |
| `pkg/` | wasm module + JS bindings (generated — `run-smoke.sh` สร้างใหม่) |
| `smoke.html` | headless test page (สร้างจาก script) |
| `fixture/` | voicebank subset + demo-song สำหรับ test (3.2 MB) |
| `run-smoke.sh` | build + serve + รัน headless Chromium แล้วเช็คผล |
| `run-pool.sh` | ทดสอบ RenderPool (bit-identical + cache + incremental) |
| `run-editor.sh` | ทดสอบ editor vertical slice + pool wiring (incremental / cancel) |
| `run-real.sh` | **ไฟล์จริง**: render เพลงจริงด้วย voicebank จริง แล้วเทียบ native bit-exact |
| `r2-test.html` | R2 ONNX inference harness; `?ep=webgpu` ใช้ WebGPU-first ทั้ง mel และ vocoder |
| `real-test.html` | หน้าเทสต์ของ `run-real.sh` (sequential vs pool + ส่ง WAV ให้ runner) |
| `pool-test.html` | หน้าเทียบ pool กับ full render + ตรวจ incremental |
| `parallel.html` | หน้าเทียบ sequential vs Web Worker fan-out (ตรวจ bit-identical) |
| `run-parallel.sh` | ทดสอบ parallel แบบ headless |
| `bench-post.html` / `bench-server.py` | วัดเวลาแบบ real wall-clock (virtual-time ทำ `performance.now()` หยุด) |
| `pool-full.html` | เทียบ 4 โหมด (spawn/pool × ทั้ง bank/เฉพาะ chunk) กับ voicebank จริง |

## รัน

```sh
# 1. ทดสอบ headless (แนะนำ — ยืนยันว่า engine ทำงานจริงบน browser)
./web/run-smoke.sh

# 2. editor vertical slice + pool wiring (incremental / cancel / progress)
./web/run-editor.sh

# 3. ไฟล์จริง: เพลงจริง + voicebank จริง (558 wav, 93 MB) แล้วเทียบ native
./web/run-real.sh                      # → /tmp/wasm-real.wav
PROJECT=/test/golden/scale-demo-app.ustx WORKERS=8 ./web/run-real.sh

# 4. เปิด UI จริง
cd native && cargo build --release --target wasm32-unknown-unknown -p engine-wasm
wasm-bindgen --target web --out-dir ../web/pkg \
  target/wasm32-unknown-unknown/release/engine_wasm.wasm
cd ../web && python3 -m http.server 8901
# เปิด http://127.0.0.1:8901/
```

**ต้องเสิร์ฟผ่าน http(s)** — `file://` ใช้ WebAssembly/ES module ไม่ได้

> **ทำไมมี Python?** `python3 -m http.server` กับ `bench-server.py` เป็น **dev/test harness เท่านั้น**
> (เสิร์ฟไฟล์ + เก็บผล/timing ที่หน้าเว็บ POST กลับมา เพราะ headless `--virtual-time-budget`
> ทำให้ `performance.now()` หยุด). ตัว product เป็น static + wasm ล้วน — host อะไรก็ได้
> หรือจะใช้ `synth-server` เสิร์ฟโฟลเดอร์นี้เองก็ได้ (origin เดียว จะได้ไม่ต้องพึ่ง CORS)

## ผลที่คาดหวังจาก `run-smoke.sh`

```
wasm loaded
aliases=2673
samples=6 withFrq=6
render: 132098 samples in ... ms, peak=0.9884
wav bytes=264240 magic=RIFF
RESULT: PASS
```

`132098 samples / 264240 bytes / peak 0.9884` — **ตัวเลขเดียวกับ native**
(`world-oracle` / `synth-cli render`)

> อัปเดต 2026-09-23 (T23): ตัวเลขเดิม `132066 / 264176` มาจากโค้ดก่อน fix mixer pre-roll ของ task-9;
> หลัง rebuild wasm ด้วยโค้ดใหม่ = **+32 samples** (0.73 ms) ตามที่ mixer ไม่หัก leading ซ้ำอีก
> · `peak` เท่าเดิม 0.9884 · ดู `docs/planning/wasm-refresh-2026-09.md`

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

## RenderPool (editor ใช้เป็น renderer หลักแล้ว)

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

> Update 2026-09-23: **editor ต่อ pool แล้ว** — `app/editor.js` สร้าง
> `RenderPool` ตอน boot (workerCount = cores-1 สูงสุด 4), เรียก
> `pool.setProject()` + `pool.render({signal, onProgress})` ทุกครั้งที่แก้,
> มี epoch guard กันผลของ render เก่ามาทับ, ปุ่ม Stop ยกเลิก render ได้
> (`cancelRender()`) และ chunk overlay วาดสถานะจาก cache จริง
> (`done` / `rendering` / `stale`). ถ้า Worker ใช้ไม่ได้จะถอยไป main-thread
> path เดิม (`driver.renderAll()`) อัตโนมัติ
>
> ยืนยันด้วย `./web/run-editor.sh`: ไม่แก้อะไร → **0/6 chunk ใน 2 ms**,
> แก้ 1 note → **1/6 chunk**, cancel → ผลถูกทิ้งและ audio เดิมยังใช้ได้

## R2 WebGPU experiment (separate harness)

หน้า editor หลักใช้ R1/WASM อย่างเดียว; R2 WebGPU คงไว้เป็น harness ทดลองแยก
ที่ `r2-test.html` ระหว่างรอเส้นทาง R2 ผ่าน local native server.

ดาวน์โหลด ORT WebGPU runtime ที่ตรึง checksum ไว้ด้วย:

```sh
./web/vendor/fetch-ort-webgpu.sh
```

เปิด `r2-test.html?ep=webgpu` เพื่อรัน chain harness บน Chromium; โหมดนี้ขอ
WebGPU EP ก่อนทั้ง mel และ vocoder แล้วใช้ WASM รองรับ operator ที่ WebGPU ใช้ไม่ได้.
ค่าเริ่มต้น harness ยังเป็น `?ep=wasm`. Harness ใช้ deterministic feature tensors
สำหรับ runtime probe.

ผลเทียบก่อนย้าย mel ไป WebGPU (mel WASM, vocoder WebGPU) บน
`fixture/scale-demo.ustx` + fixture bank:

| Metric | Browser · Rust WORLD + ORT WebGPU | Native · C++ WORLD + ORT CPU | Difference |
|---|---:|---:|---:|
| samples | 640,196 | 640,196 | equal |
| peak | 0.90744 | 0.90747 | 0.00003 |
| RMS | 0.28723418 | 0.28723427 | 0.00000008 |
| correlation | — | — | 0.999999999855 |
| max difference (16-bit WAV) | — | — | 2 LSB |

ตัวเลขนี้เป็นผลจาก demo project/bank หนึ่งชุด; ยังควรเทียบ golden เพิ่มกับ
โปรเจกต์และ voicebank อื่น โดยเฉพาะ sample ที่ไม่มี `.frq` ซึ่งใช้ Rust DIO fallback.

ย้าย mel เป็น WebGPU-first แล้วและลองใน Chromium บน RTX 5060 Ti:
สร้าง session ด้วย `[webgpu, wasm]` สำหรับ mel และ vocoder ได้, editor render
ครบ **640,196 samples / 14.52 s ใน 5.38 s**. Harness input เดียวกันให้ mel
WebGPU warm run 19.6 ms เทียบ WASM 10.2 ms และ vocoder 19.3 ms เทียบ 1,845.5 ms;
การสร้าง WebGPU pipeline ครั้งแรกมี overhead ราว 522 ms. Peak/RMS audio ใกล้กัน
ระหว่าง EP แต่ mel/output float hashes ต่างกันตาม GPU kernel. ตัวเลข parity ตาราง
ด้านบนยังเป็นผลจาก mel WASM จึงยังไม่ใช่ sample-level native parity ของ mel WebGPU.

## Projects (OPFS)

`.ustx` เก็บใน Origin Private File System — เปิดหน้าใหม่แล้วงานยังอยู่:

```js
import { ProjectStore } from "./app/project-store.js";
const store = new ProjectStore();            // root = "projects"
await store.save("my-song", driver.yaml());  // → projects/my-song.ustx
await store.list();                          // [{ name, bytes, savedAt }]
await store.load("my-song");
```

Editor ทำให้อัตโนมัติ: **autosave 1500 ms** หลังแก้ (เขียนเฉพาะเมื่อ YAML ต่างจาก
ครั้งล่าสุด), เปิดโปรเจกต์ล่าสุดให้เองตอน boot (`?project=<name>` เพื่อระบุ),
project browser ในแถบซ้าย (New / Save as / Export / Import / ลบ) และเรียก
`navigator.storage.persist()` ครั้งแรกที่บันทึก

> ⚠️ **ล้าง site data = โปรเจกต์หายทั้งหมด** — ใช้ **Export .ustx** เก็บสำเนาไว้เสมอ

## Voicebanks (OPFS)

Voicebank import เข้า OPFS ได้แล้ว — ปุ่ม **Import folder…** ในแถบซ้ายของ
editor เลือกโฟลเดอร์ที่มี `voice/oto.ini` + `*.wav` (+ `*_wav.frq`) เก็บใต้
`<root>/voice/…` เดียวกับ layout ที่ `HttpBank` เสิร์ฟ:

```js
import { OpfsBank } from "./samples.js";
const bank = new OpfsBank({ root: "my-bank" });
await bank.importFiles(fileList);                    // directory-picker FileList
await OpfsBank.installedRoots();                     // ["my-bank", …]
await OpfsBank.removeRoot("my-bank");                // ลบออกจาก OPFS
```

Editor เลือก bank ตอน boot: **OPFS ที่ import ไว้ก่อน → fixture** (ผ่าน
`?voicebank=<root>` เพื่อระบุ หรือ `?voicebank=full` = full Teto ทาง HTTP แบบเดิม),
เลือก/ลบจากแผง Voicebanks ได้ตอนรัน — สลับแล้ว render ใหม่ทันที ส่วน
RenderPool โหลดเฉพาะ wav/frq ที่ chunk อ้างถึง (per-chunk sample loading)
จึงรองรับ bank ใหญ่ได้โดยไม่ยัดทั้งก้อนเข้า engine

> OPFS ใช้ได้บน Chrome/Edge 86+ และ Firefox 111+; Safari ยังไม่รองรับ
> `createWritable` เต็มรูปแบบ — บน Safari editor ยัง boot ด้วย fixture bank ได้

## Browser editor checkpoint

`editor-test.html` ผ่าน headless Chromium โดยครอบ boot, DOM/canvas paint,
note hit-test, command edit, undo, Draw tool, marquee multi-select, audio render,
cursor-anchored zoom, OPFS project store และ voicebank OPFS flow (import /
เลือก / render บน bank ที่ import / ลบ → fallback fixture). Demo ใช้
`fixture/scale-demo.ustx` กับ HTTP fixture bank.

## ยังไม่ทำ

- **Persistence** — OPFS project/voicebank store, autosave และ export flow (pool wiring + progress/cancel ทำแล้ว)
- **Feature breadth** — multi-track audio mix, expression lanes, authoritative
  phoneme labels และ tablet/pen hardware QA
- **Neural engines** — R2 มี browser WebGPU preview แล้ว; DiffSinger/MoE ยัง native-only
- **Deploy** — ยังไม่มี service worker/PWA/static host config
