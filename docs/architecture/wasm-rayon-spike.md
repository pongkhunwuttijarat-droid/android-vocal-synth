# WASM Shared-Memory Threads (`wasm-bindgen-rayon`) — Feasibility Spike

> วัดเมื่อ 2026-09-11 · Chromium 152.0.7977.82 headless · AMD Ryzen 5 8600G (12 threads, `navigator.hardwareConcurrency = 12`)
> nightly `1.100.0-nightly (67eda617e 2026-09-10)` + `rust-src` · stable `1.97.1` (baseline)
> โปรเจกต์: `test/golden/scale-demo-app.ustx` (14.50s audio, 4 chunks) + voicebank จริง `test/golden/teto-english/library` (558 wav + 558 frq, 86.9 MiB)
> โค้ดทดลองอยู่ **นอก repo** ที่ `~/rusted-utau-rayon-spike/` — ไม่ได้แก้ `native/` เลย

---

## คำตอบ (verdict)

**ทำได้ (works)** — shared-memory threads รันจริง, ได้ **bit-identical** (`max|diff| = 0.0e+0`), และเร็วกว่า single-thread **2.84x**

แต่ **ยังไม่คุ้มที่จะเปลี่ยนมาใช้แทน Web Worker** เพราะกำไรเหนือ 4 Web Worker ที่มีอยู่คือ **~6% เท่านั้น** (2486 ms vs 2653 ms) ขณะที่ต้องจ่ายด้วย nightly + `-Z build-std`, COOP/COEP, module โตขึ้น 8.8% และข้อจำกัดเรื่องลำดับการ init

```
[t0]  ไม่มี pool   : sequential 7021 ms | parallel 7152 ms (2.03x RT)  speedup 0.98x  max|diff| 0.0e+0
[t4]  pool 4 threads: sequential 7069 ms | parallel 2486 ms (5.83x RT)  speedup 2.84x  max|diff| 0.0e+0  ✔
```

**คอขวดจริงไม่ใช่กลไก threading แต่คือ chunk load imbalance** — chunk หนักสุด 2202 ms จากทั้งหมด 7106 ms → เพดานที่ 4 chunks คือ ~3.2x ไม่ว่าใช้ pool เท่าไร

---

## 1. ผลการวัด

### 1.1 Render — `scale-demo-app.ustx` (14.50s audio, 4 chunks)

pool size vs เวลา (min of 2 runs, real wall-clock):

| pool | sequential | parallel | speedup | realtime | `max\|diff\|` | thread ที่เห็น |
|---|---|---|---|---|---|---|
| **0** (ไม่ init pool) | 7021 ms | 7152 ms | 0.98x | 2.03x | **0.0e+0** | `[0,0,0,0]` |
| 1 | 7077 ms | 7205 ms | 0.98x | 2.01x | **0.0e+0** | `[0,0,0,0]` |
| 2 | 7014 ms | 3876 ms | 1.81x | 3.74x | **0.0e+0** | `[0,1,0,1]` |
| **4** | 7069 ms | **2486 ms** | **2.84x** | **5.83x** | **0.0e+0** | `[2,1,0,3]` |
| 4 (วัดซ้ำ) | 7102 ms | 2469 ms | 2.88x | 5.87x | **0.0e+0** | `[1,0,2,3]` |
| 8 | 7070 ms | 2461 ms | 2.87x | 5.89x | **0.0e+0** | `[0,1,2,3]` |
| 12 | 8535 ms | 3032 ms | 2.81x | 4.78x | **0.0e+0** | `[0,1,2,3]` |

อ่านตัวเลข:

- **8 และ 12 threads ไม่ได้อะไรเพิ่ม** จาก 4 — เพราะมีแค่ 4 chunks ที่ fan ออกได้ เห็นจาก `thread_log()` ที่โชว์แค่ index `0..3` แม้ pool จะมี 8/12 threads ส่วน 12 threads ยัง *ช้ากว่า* เพราะ pool startup (265–276 ms) + ความร้อนสะสมจากการวัดต่อเนื่อง
- sequential ~7050 ms ตรงกับที่ `wasm-parallel.md` รายงานไว้ (7051 ms) → **harness นี้ reproduce baseline ของโปรเจกต์ได้** จึงเทียบข้ามเอกสารได้
- `parallel` ที่ pool=0/1 ช้ากว่า sequential ~2% — rayon scheduler overhead ล้วน ๆ ไม่มีงานขนานจริง

### 1.2 `demo-song.ustx` (2.99s, **1 chunk**)

| pool | sequential | parallel | speedup | `max\|diff\|` |
|---|---|---|---|---|
| 0 | 602 ms | 660 ms | 0.91x | 0.0e+0 |
| 4 | 565 ms | 562 ms | 1.01x | 0.0e+0 |

1 chunk → ขนานไม่ได้ตามคาด (ยืนยันว่า logic ถูก ไม่ได้ over-report) 565 ms ใกล้กับ 534 ms ที่ `wasm-performance.md` รายงาน

### 1.3 Bit-identical — ยืนยัน 2 ระดับ

**(ก) ภายใน module เดียวกัน** — `render_planned_sequential()` vs `render_project()` (par_iter) → `max|diff| = 0.0e+0` **ทุก config** (pool 0/1/2/4/8/12)

**(ข) ข้าม module** — threaded module vs **non-threaded module ที่ build ด้วย stable 1.97.1** (คนละ wasm คนละ toolchain):

| | non-threaded module | threaded module (ทุก pool) |
|---|---|---|
| checksum(scale 14.50s) | `d4a651c16f67fd58` | `d4a651c16f67fd58` |
| checksum(demo 2.99s) | `0563140f3b7bbc32` | `0563140f3b7bbc32` |
| peak (scale / demo) | 0.6668 / 0.9884 | 0.6668 / 0.9884 |
| samples | 639443 / 132066 | 639443 / 132066 |

**สรุป: threads ไม่เปลี่ยนตัวเลขเลย** — parity bar ของโปรเจกต์ (`max|diff| = 0.0e+0`) ยังผ่าน

เหตุผลเชิงโครงสร้าง: FFT plan cache ใน `world-rs/src/fft.rs` เป็น `thread_local!` → worker thread แต่ละตัวได้ cache ของตัวเอง ไม่มีการแชร์ plan ข้าม thread และ chunk เป็นงานอิสระ (คนละ sub-phrase, คนละ output buffer) การแข่งกันมีแค่ `RenderCache` mutex ซึ่ง pipeline ล็อกเฉพาะตอน get/put อยู่แล้ว

### 1.4 Per-chunk (pool warm) — ต้นตอของเพดาน 2.84x

```
c0=1467 ms (3.50s audio)
c1=1966 ms (3.50s)
c2=2202 ms (4.00s)   <-- หนักสุด
c3=1471 ms (3.50s)
sum = 7106 ms  (= sequential 7069 ms)
max = 2202 ms  (= เพดานที่ 4 threads)
```

4 threads → ideal = `max` = 2202 ms, ได้จริง 2486 ms (ส่วนต่าง ~284 ms = plan + mix + scheduling)
2 threads → จับคู่ (c0+c1)=3433 (c2+c3)=3673 → ideal 3673, ได้จริง 3876 ms ✔ สอดคล้อง

ดังนั้น **การเพิ่ม pool size ไม่ช่วยอะไรอีกแล้ว** สิ่งที่จะช่วยคือเพิ่มจำนวน chunk / เกลี่ย chunk ให้เท่ากัน — ซึ่งช่วย **ทั้ง** เส้น rayon และเส้น Web Worker

### 1.5 ขนาด module

| | ก่อน wasm-bindgen | หลัง wasm-bindgen (ที่ browser โหลดจริง) |
|---|---|---|
| non-threaded (stable) | 1,843,814 B | 1,763,305 B |
| **threaded** (nightly + build-std) | 2,501,298 B | **1,917,749 B** |
| **delta** | +657,484 B (+35.7%) | **+154,444 B (+8.8%)** |

ส่วนต่างหลัง bindgen เล็กกว่ามากเพราะ transform ตัด symbol ที่ไม่ใช้ออก แต่ **+154 KB คือราคาจริงที่ผู้ใช้จ่ายข้ามเครือข่าย**

### 1.6 `crossOriginIsolated` — ได้มาแล้ว

```
crossOriginIsolated: true
SharedArrayBuffer: function
hardwareConcurrency: 12
thread_support: true
initThreadPool(4): 144.0 ms
rayon_threads(): 4
fft_backend: wasm-simd
```

headers ที่ต้องส่ง (ใน `serve.py`):

```
Cross-Origin-Opener-Policy:   same-origin
Cross-Origin-Embedder-Policy: require-corp
Cross-Origin-Resource-Policy: same-origin
```

และ MIME type ที่จำเป็น: `.wasm → application/wasm`, `.js → text/javascript`
ถ้า `.wasm` ไม่ใช่ `application/wasm` จะ fallback ไป `WebAssembly.instantiate` (ช้า) โดยไม่ error

---

## 2. Blocker ที่เจอ (ตามลำดับที่ชนจริง)

ทั้งหมดนี้คือ "สิ่งที่ต้องรู้" — **ไม่มีข้อไหนแก้ไม่ได้** แต่ README ของ `wasm-bindgen-rayon` **ไม่พอ** สำหรับ toolchain ปัจจุบัน

### B1 — nightly + `rust-src` (ตามคาด)

```
$ rustup toolchain install nightly --profile minimal --component rust-src --target wasm32-unknown-unknown
nightly-x86_64-unknown-linux-gnu installed - rustc 1.100.0-nightly (67eda617e 2026-09-10)
$ ls ~/.rustup/toolchains/nightly-*/lib/rustlib/src/rust/library/Cargo.toml   # ต้องมี
```

### B2 — `+atomics` เฉย ๆ **ไม่** ทำให้ memory เป็น shared

`+atomics,+bulk-memory` (สูตรใน README) ให้ memory `flags=0x0` (ไม่ shared) — พิสูจน์ด้วย minimal crate:

```
### A) +atomics,+bulk-memory + build-std
  MEMORY[0] flags=0x0 shared=no min=17 pages max=None
### B) A + --shared-memory + --max-memory
  MEMORY[0] flags=0x3 shared=YES min=17 pages max=16384
```

ผลคือ wasm-bindgen จะ **เงียบ ๆ ข้าม** threading transform ไป (`is_enabled()` เช็ค `mem.shared`) → ได้ module ที่ compile ผ่านแต่ **thread ไม่ทำงาน** อาการแบบนี้หลอกง่ายมาก

### B3 — memory ต้องเป็น **imported**

```
$ wasm-bindgen --target web --out-dir pkg ...
error: failed to prepare module for threading
Caused by:
    threading requires memory to be imported; add `-Clink-arg=--import-memory` to RUSTFLAGS
```

### B4 — `__wasm_init_tls` หาย (blocker ตัวจริง)

```
Caused by:
    failed to find `__wasm_init_tls`
```

**root cause ที่พบ:** `wasm-bindgen-cli-support 0.2.127` (`src/transforms/threads/mod.rs`) เรียก
`delete_synthetic_func(module, "__wasm_init_tls")`, `delete_synthetic_global("__tls_size")`, `"__tls_align"`
และ `wasm_conventions::get_tls_base()` ซึ่งต้องการ **export** ชื่อ `__tls_base`
— ทั้ง 4 ตัวเป็น synthetic symbol ที่ **LLD สร้าง** ไม่ใช่ Rust

แต่เมื่อดัก linker argv จริงของ rustc nightly 1.100 ปรากฏว่า **rustc ไม่ส่ง flag เหล่านี้เลย**:

```
=== --export targets rustc requested ===
bump
spawn_and_count
touch_tls
=== any tls/shared-memory related ===
(none)
```

LLD ใน toolchain นี้ **สร้าง** symbol พวกนี้ (grep เจอ `__wasm_init_tls` ใน `rust-lld` binary)
แต่ **ไม่ export ให้โดย default** อีกต่อไป — ขณะที่ wasm-bindgen ยังต้องการมันอยู่
(เทียบ:`__tls_base` โผล่ใน binary เพราะ std อ้างถึง ไม่ใช่เพราะถูก export)

ทดสอบยืนยัน: ใส่ `--export=` เองแล้ว symbol ปรากฏทันที

```
$ RUSTFLAGS="... -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align" ...
  __wasm_init_tls      2      # ก่อนใส่ = 0
  __tls_size           2
  __tls_align          2
```

### B5 — `failed to find tls base`

หลัง B4 ผ่าน ยังต้อง export `__tls_base` เพิ่ม (`get_tls_base()` หา export ชื่อนี้เท่านั้น ไม่มี heuristic):

```
Caused by:
    failed to find tls base
```

### B6 — `initThreadPool()` ต้องมาก่อน render ครั้งแรกเสมอ

ถ้า render ก่อน (ซึ่งเรียก `par_iter` → rayon สร้าง global pool ขนาด 1 thread ไปแล้ว):

```
render before pool: 132066 samples
rayon_threads() after implicit init: 1
initThreadPool(4) AFTER render: FAILED -> Error: called `Result::unwrap_throw()` on an `Err` value
```

คือ rayon `build_global()` คืน `GlobalPoolAlreadyInitialized` แล้ว `wasm-bindgen-rayon` `.unwrap_throw()`
→ **ผู้ใช้ต้องเรียก `await initThreadPool(n)` เป็นอย่างแรกก่อนแตะ engine** ไม่งั้นได้ 1 thread ตลอดไปโดยไม่มี error

### B7–B8 — ปัญหาที่ harness เอง (ไม่เกี่ยวกับ wasm)

- HTTP server override `log_message` แล้วพังตอน 404 (`TypeError: argument of type 'HTTPStatus' is not a container or iterable`) → connection ถูกตัด → browser ขึ้น `TypeError: Failed to fetch` เฉย ๆ **ไม่มี 404 ให้เห็น** — ทำให้ debug ยากมาก ต้องแก้ให้ logger ไม่ throw
- `Math.max(...pcm)` กับ array 639k ตัว → `RangeError: Maximum call stack size exceeded` (spread เกิน argument stack)

---

## 3. คำสั่งที่ใช้ได้จริง

### 3.1 Build

```sh
export PATH="$HOME/.cargo/bin:$PATH"

# ต้องมี: nightly + rust-src component (สำหรับ -Z build-std)
rustup toolchain install nightly --profile minimal \
  --component rust-src --target wasm32-unknown-unknown

FLAGS="-C target-feature=+simd128,+atomics,+bulk-memory,+mutable-globals"
FLAGS="$FLAGS -C link-arg=--shared-memory"
FLAGS="$FLAGS -C link-arg=--import-memory"
FLAGS="$FLAGS -C link-arg=--max-memory=2147483648"
# สี่ตัวนี้คือส่วนที่ README ของ wasm-bindgen-rayon ไม่ได้บอก แต่ขาดไม่ได้ใน toolchain นี้
FLAGS="$FLAGS -C link-arg=--export=__wasm_init_tls"
FLAGS="$FLAGS -C link-arg=--export=__tls_size"
FLAGS="$FLAGS -C link-arg=--export=__tls_align"
FLAGS="$FLAGS -C link-arg=--export=__tls_base"

cd native
RUSTFLAGS="$FLAGS" cargo +nightly build --release \
  --target wasm32-unknown-unknown -p engine-wasm \
  -Z build-std=std,panic_abort
```

### 3.2 แก้ที่ต้องทำในโค้ด (ในสำเนาเท่านั้น)

`crates/engine-wasm/Cargo.toml`
```toml
rayon = "1"
# SPIKE: เปิด `parallel` ให้ wasm เพื่อให้ par_iter compile เข้ามา
synth-cli = { path = "../../tools/synth-cli", default-features = false, features = ["parallel"] }

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen-rayon = "1.3"
```

`crates/engine-wasm/src/lib.rs`
```rust
#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_rayon::init_thread_pool;      // → initThreadPool(n) ฝั่ง JS

impl PhraseRenderer for MemoryRenderer {
    fn parallel_safe(&self) -> bool { true }        // เดิม false
    ...
}
```

> หมายเหตุ: ไม่ต้องแก้ `synth-cli/src/pipeline.rs` เลย — `par_iter` path มีอยู่แล้วและเปิดด้วย feature `parallel`

### 3.3 Bindings + serve + วัด

```sh
wasm-bindgen --target web --out-dir pkg --out-name engine_wasm engine_wasm.wasm
```

**ต้องมี `pkg/index.js`** เพราะ `workerHelpers.js` ของ wasm-bindgen-rayon ทำ `await import('../../..')`
(จาก `pkg/snippets/<crate>/src/` ขึ้นมา 3 ชั้น = ตัว directory `pkg/`) ซึ่ง bundler resolve ผ่าน `package.json`
แต่ static server ทำไม่ได้ ถ้าไม่มีจะได้ `Failed to fetch` ตอน spawn worker:

```js
// pkg/index.js
export * from './engine_wasm.js';
export { default } from './engine_wasm.js';
```

ฝั่ง page:

```js
import * as pkg from "./pkg/engine_wasm.js";
await pkg.default();                                   // wasm init
if (pkg.initThreadPool) await pkg.initThreadPool(navigator.hardwareConcurrency);
const engine = new pkg.Engine();
// ... set_voicebank / add_sample ...
const pcm = engine.render_project(yaml, 0);            // par_iter ทำงานบน pool
```

รัน headless (**ห้ามใช้ `--virtual-time-budget`** — freeze `performance.now()` แล้วอ่านได้ 0):

```sh
chromium-browser --headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage \
  --user-data-dir="$(mktemp -d)" "http://127.0.0.1:PORT/bench.html?threads=4"
```

วัดซ้ำ: `~/rusted-utau-rayon-spike/web/run.sh <pkg-dir> <threads> <fixture> <label>`
(harness เต็มอยู่ที่ `~/rusted-utau-rayon-spike/web/` — `serve.py`, `bench.html`, `run.sh`, `order.html`)

> **หมายเหตุการวัด:** การโหลด voicebank 558 ไฟล์ใช้เวลา ~25 s (1,116 HTTP request เรียงกันผ่าน Python server)
> ตัวเลข render ทั้งหมด **ไม่รวม** เวลาโหลด — จับเวลาครอบ `plan_project` + render + mix เท่านั้น

---

## 4. เทียบกับ Web Worker ที่มีอยู่

| | **rayon 4 threads** (spike นี้) | **4 Web Workers** (`wasm-parallel.md`) |
|---|---|---|
| wall clock (scale-demo) | **2486 ms** | 2653 ms |
| realtime | **5.83x** | 5.47x |
| speedup vs sequential | **2.84x** | 2.66x |
| bit-identical | ✔ `0.0e+0` | ✔ `0.0e+0` |
| voicebank RAM | **1 ชุด** (~87 MiB wav) | 4 ชุด (เอกสารระบุ ~198 MB/worker) |
| COOP/COEP | **ต้องมี** | ไม่ต้อง |
| static hosting (GitHub Pages) | ❌ ใช้ไม่ได้ | ✔ |
| host code ที่ต้องเขียน | ไม่มี (par_iter) | `worker.js` + plan/render_chunk/Mixer protocol (มีแล้ว) |
| incremental re-render | ยังไม่มี scheduling ระดับ chunk | per-chunk dispatch + `chunk_wav_keys` มีแล้ว |
| worker ตาย/ค้าง | spin lock อาจ lock ทั้ง tab | isolate ใน worker |

**กำไรจริงของ rayon = ~167 ms (~6%)** ซึ่งอยู่ในระดับ noise/thermal ของการวัดชุดนี้ (sequential ที่วัดได้แกว่ง 6982–8535 ms)
และ **ยังไม่นับ** ต้นทุนที่เพิ่ม: nightly + `-Z build-std`, COOP/COEP, +154 KB module, พันธะเรื่องลำดับ `initThreadPool`

---

## 5. ถ้าจะ adopt จริง — ต้นทุนและสิ่งที่ต้องทำ

**ต้นทุน**

1. **Toolchain** — pin nightly + `rust-src` ตลอดไป (nightly พังได้ทุกคืน; README ของ crate เองแนะนำให้ pin) และ `-Z build-std` ทำให้ build std/core/alloc ใหม่ทุก clean build → CI time โต
2. **Deployment** — COOP/COEP ทำให้ **static host ธรรมดาใช้ไม่ได้** (GitHub Pages, S3 ที่ตั้ง header ไม่ได้) ต้องมี reverse proxy หรือ host ที่ตั้ง header ได้ — และต้องทดสอบ cross-origin resource ทุกตัวว่ามี CORP
3. **Module size** — +154 KB (+8.8%) ที่ browser ต้องโหลด
4. **API contract** — `initThreadPool(n)` ต้องเป็น call แรก ห้าม render ก่อน (B6) ต้องใส่ในทุก entry point รวมถึง test harness
5. **Memory** — ต้อง `--max-memory` (ใช้ 2 GiB) → SharedArrayBuffer จอง address space; บนมือถือ/tablet ต้องวัดซ้ำ
6. **Flag fragility** — `--export=__wasm_init_tls/__tls_size/__tls_align/__tls_base` เป็น workaround ของช่องว่างระหว่าง rustc/LLD/wasm-bindgen วันใดที่ฝั่งใดฝั่งหนึ่งแก้อาจพังแบบเงียบ ๆ (B2 คือตัวอย่างอาการเงียบ) ควรมี build check ที่ assert ว่า memory เป็น shared

**ลำดับความคุ้มถ้าจะทำต่อ**

1. **เพิ่มจำนวน chunk / เกลี่ย chunk ให้เท่ากันก่อน** — ได้กับ **ทั้งสองเส้นทาง** และเป็นคอขวดจริง (chunk หนักสุด 2202 ms จาก 7106 ms) คุ้มกว่าการเปลี่ยนกลไก threading
2. **per-chunk sample loading** — ตามที่ `wasm-parallel.md` สรุปไว้แล้ว (625 → 27 ms/worker, RAM 790 MB → 10 MB)
3. **rayon** — พิจารณาเมื่อข้อ (1) หมดแล้ว *และ* ข้อจำกัดที่เหลือคือ**หน่วยความจำบนแท็บเล็ต** (rayon ใช้ voicebank ชุดเดียว) หรือต้องการขนาน**ภายใน** chunk (D4C ~60–70% ของเวลา ยังเป็น single-thread อยู่ — ตรงนี้คือจุดที่ shared memory ชนะ message passing จริง ๆ)

---

## 6. คำแนะนำ (recommendation)

**ไม่เปลี่ยน** — คง **Web Worker fan-out** เป็นเส้นทางที่ ship จริง

เหตุผล: กำไร 6% ไม่คุ้มกับ nightly toolchain + สูญเสีย static hosting + module โต + พันธะ `initThreadPool` และที่สำคัญคือ **เพดานของทั้งสองวิธีมาจาก chunk imbalance ไม่ใช่กลไกขนาน** — 4 chunks ให้เพดาน ~3.2x เท่ากันทั้งคู่ การไปลงทุนกับ threading mechanism ก่อนแก้ chunking คือ optimize ผิดจุด

**เก็บ rayon ไว้เป็นทางเลือกสำหรับ 2 สถานการณ์:**

1. **หน่วยความจำบนแท็บเล็ต** — ถ้า 4 workers × voicebank (หรือแม้ per-chunk loading แล้ว) ยังเกินงบ RAM ของ Mi Pad การถือ voicebank ชุดเดียวแล้วขนานใน-process คือคำตอบเดียวที่ Web Worker ให้ไม่ได้
2. **ขนานภายใน chunk** — D4C/CheapTrick/synthesis ยังเป็น single-thread (D4C 60–70%) งานระดับ band/frame เป็น fine-grained ซึ่ง **ไม่เหมาะกับ Web Worker เลย** (message passing ต่อ frame ไม่ไหว) นี่คือที่ที่ shared memory ชนะจริง — และเป็นงานที่จะได้กำไรหลัก ไม่ใช่ 6% จากการขนาน chunk

**ถ้าจะทำต่อ:** เริ่มจาก instrument D4C ว่าขนานระดับ band/frame ได้ไหม แล้วประเมินใหม่ — ถ้าขนานได้ rayon จะมีเหตุผลชัดเจนกว่านี้มาก

**ผลข้างเคียงที่ได้ฟรีจาก spike นี้:** ยืนยันว่า DSP ทั้งเส้น (รวม `thread_local` FFT plan cache) **thread-safe และ deterministic** — `max|diff| = 0.0e+0` ข้าม pool size 0/1/2/4/8/12 และข้าม module ที่ build ด้วยคนละ toolchain นี่เป็นหลักฐานที่ใช้ตัดสินใจเรื่องขนานภายใน chunk ได้เลย

---

## 7. ไฟล์/สถานะ

| | |
|---|---|
| โค้ดทดลอง | `~/rusted-utau-rayon-spike/` (นอก repo — `native/`, `native-st/` baseline, `web/` harness, `pkg-mt/`, `pkg-base/`) |
| แก้ใน repo | **ไม่มี** — `native/` ไม่ถูกแตะ (`parallel_safe()` ยังเป็น `false`, ไม่มี `wasm-bindgen-rayon` ใน `Cargo.toml`) |
| ไฟล์เดียวที่เพิ่มใน repo | `docs/architecture/wasm-rayon-spike.md` (เอกสารนี้) |
