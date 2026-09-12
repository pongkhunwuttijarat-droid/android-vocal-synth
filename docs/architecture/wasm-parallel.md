# Parallel render บน wasm — Web Worker fan-out

> วัดเมื่อ 2026-09-10 · verification update 2026-09-12 · Chromium 152 headless · Ryzen 5 8600G (12 threads)
> โปรเจกต์: `test/golden/scale-demo-app.ustx` (14.50s audio, 4 chunks)

---

## คำตอบ

**ได้** — และเร็วกว่า single-threaded จริง **2.66x** ด้วย 4 workers
พร้อมผล **bit-identical** กับเส้น single-threaded

```
sequential  : 7051 ms  (2.06x realtime)
workers=2   : 3768 ms  (3.85x realtime)  speedup 1.87x  max|diff| 0.0e+0  bit-identical ✔
workers=4   : 2653 ms  (5.47x realtime)  speedup 2.66x  max|diff| 0.0e+0  bit-identical ✔
```

ทดสอบซ้ำ: `./web/run-parallel.sh`

---

## ไม่ใช้ shared-memory threads — ใช้ Web Worker fan-out

| วิธี | ต้องมี | สถานะ |
|---|---|---|
| `wasm-bindgen-rayon` (shared memory) | nightly + `-Z build-std` + `SharedArrayBuffer` + COOP/COEP headers | ❌ ทำไม่ได้ในเครื่องนี้ (ดูหัวข้อถัดไป) |
| **Web Worker fan-out** | เบราว์เซอร์ทั่วไป | ✅ **ทำแล้ว ใช้งานได้** |

**ทำไม Web Worker ถึงพอ:** parallelism ของ pipeline นี้เป็น **coarse-grained
อยู่แล้ว** — chunk ละ 100–1500 ms ไม่ใช่ระดับ millisecond การส่งงานทั้ง chunk
ให้ worker จึงไม่ต้องใช้ shared memory เลย

แต่ละ worker มี:
- wasm instance ของตัวเอง
- สำเนา voicebank samples ของตัวเอง (ไม่แชร์หน่วยความจำ)
- chunk plan ของตัวเอง (plan เป็น pure function ของ project + voicebank
  จึงได้ผลเหมือนกันทุก worker)

main thread รวมผลด้วย `Mixer` ซึ่งเรียก `runtime::mixer::mix` ตัวเดียวกับที่
pipeline ใช้ → seam crossfade เหมือนกันเป๊ะ

---

## API ที่เพิ่ม (`engine-wasm`)

```ts
// main thread: วางแผนงาน
const n = engine.plan_project(ustxYaml, track);   // → จำนวน chunk
const meta = JSON.parse(engine.chunk_meta_json()); // [{position_ms, leading_ms, ...}]
engine.plan_skipped();                             // phrase ที่ render ไม่ได้

// worker: render chunk ที่ได้รับมอบหมาย
engine.plan_project(ustxYaml, track);   // plan เดียวกัน
const pcm = engine.render_chunk(i);     // Float32Array ของ chunk i

// main thread: รวม
const mixer = new Mixer();
for (const [i, pcm] of chunks) mixer.add_chunk(pcm, meta[i].position_ms, meta[i].leading_ms);
const out = mixer.finish();             // mono f32 44.1 kHz

// baseline (single-thread เทียบ)
engine.render_planned_sequential();
```

ไฟล์: `web/worker.js` (worker), `web/parallel.html` (หน้าเทียบ),
`web/run-parallel.sh` (ทดสอบ headless)

**ไม่ต้องมี COOP/COEP headers** → ใช้ได้บน GitHub Pages และ Safari
(ต่างจาก shared-memory threads)

---

## ข้อจำกัด: `wasm-bindgen-rayon` ทำไม่ได้ในเครื่องนี้

ลองแล้ว เจอ blocker 2 ข้อที่ sandbox ปิดไว้:

```
$ cargo fetch            # wasm-bindgen-rayon v1.3.0
error: failed to open .../wasm-bindgen-rayon-1.3.0.crate
Caused by: Read-only file system (os error 30)      # ~/.cargo/registry

$ rustup toolchain install nightly
error: could not create temp file /home/seal/.rustup/tmp/...
Caused by: Read-only file system (os error 30)      # ~/.rustup
```

ต้องใช้:
1. **nightly** — `std::thread` บน wasm ต้อง `-Z build-std`
   (stable ยัง build `std` สำหรับ wasm atomics ไม่ได้)
2. **`rust-src` component** — ยังไม่ได้ติดตั้ง
3. **network เขียน registry** — sandbox เขียนนอก workspace ไม่ได้

(หมายเหตุ: `cargo fetch` ต้องเขียน `~/.cargo/registry/cache` ซึ่งอยู่ใต้
`/home/seal` — นอก workspace ที่ sandbox อนุญาต)

ถ้าอนุญาต 3 ข้อนี้ ก็ทำ shared-memory threads ได้ — แต่ **ไม่จำเป็น**
เพราะ Web Worker ให้ผลใกล้เคียงแล้ว (2.66x จาก 4 workers)

---

## เทียบกับ native

| config | เวลา | realtime |
|---|---|---|
| native 12 cores (rayon) | 1383 ms | 10.5x |
| **wasm 4 workers** | **2653 ms** | **5.5x** |
| wasm sequential | 7051 ms | 2.1x |
| native sequential | 4304 ms | 3.4x |

- wasm 4 workers เร็วกว่า native sequential → **ช่องว่าง parallel ปิดไปเกือบหมด**
- เหลือ native parallel นำอยู่ 1.9x (มากกว่า cores + SIMD ที่เต็มกว่า)

---

## ทำไมผลถึง bit-identical

`render_planned_chunk` (ใน `synth_cli::pipeline`) ตัดสินใจให้ chunk หนึ่ง render
อย่างไร — เดิม logic นี้อยู่ใน closure ของ `render_project_with` ตอนนี้แยกเป็น
ฟังก์ชันกลางที่ทั้ง 3 ทางเรียกใช้:

| ทาง | เรียก |
|---|---|
| sequential pipeline | `chunk_render_input` + `trim_chunk_audio` |
| rayon (native) | เหมือนกัน (คนละ scheduler) |
| wasm worker | `render_planned_chunk` |

→ ไม่มี logic ซ้ำสองที่ จึงไม่มีทางเพี้ยนได้ และ mix ก็ใช้ `runtime::mixer::mix`
ตัวเดียวกัน (igual-power crossfade ที่ seam)

**ข้อควรระวัง:** ถ้าแก้ `chunk_render_input` / `trim_chunk_audio` ในอนาคต
ต้อง verify `run-parallel.sh` ว่า max|diff| ยัง 0

---

## งานที่ควรทำต่อ

1. **วัดบนแท็บเล็ต** — `hardwareConcurrency` บน Mi Pad อาจเป็น 8
   แต่ memory ต่อ worker (voicebank 93MB × N) เป็นข้อจำกัดจริง
   → ควรเพิ่ม option "worker โหลด sample เฉพาะ chunk ที่ตัวเองรับ" (มี
   `SampleSource` แยกอยู่แล้ว)
2. **worker pool ที่ reuse** — ตอนนี้สร้าง/ทำลาย worker ต่อรอบ
   (voicebank decode ซ้ำ) สำหรับ UI จริงควรคง pool ไว้
3. **cancel ระหว่างทาง** — `render_chunk` ยังไม่มี cancel flag
   (pipeline มี `cancel` แล้ว แต่ engine-wasm ยังไม่ wire)


---

# Pool ดีกว่าไหม — วัดแล้ว

> วัดเพิ่ม 2026-09-10 · Chromium headless · voicebank จริง 558 wav + 558 frq (198 MB)
> scale-demo 14.50s / 4 chunks

## คำตอบ

**ดีกว่า แต่ไม่ใช่ของที่คุ้มที่สุด** — กำไรที่ใหญ่กว่าคือ
**ให้ worker โหลดเฉพาะ sample ที่ chunk ตัวเองต้องใช้**

| โหมด (4 workers, render ×2) | เวลา | worker พร้อมใน | bit-identical |
|---|---|---|---|
| sequential (main thread) | 6860 ms | — | (ตัวอ้างอิง) |
| A. spawn ต่อ render, โหลดทั้ง bank | 3059 / 3186 ms | **625 ms** | ✔ |
| B. **pool**, โหลดทั้ง bank | 2537 / 2528 ms | 626 ms | ✔ |
| C. spawn ต่อ render, **เฉพาะ chunk** | 2332 / 2201 ms | **27 ms** | ✔ |
| D. **pool + เฉพาะ chunk** | **2159 / 2170 ms** | 32 ms | ✔ |

## อ่านตัวเลข

**1. Pool ช่วยจริง ~620 ms ต่อ render — แต่เฉพาะเมื่อโหลดทั้ง bank**

A → B: 3059 → 2537 ms เพราะ pool ไม่ต้อง clone 198 MB + parse 558 wav
ให้ worker ใหม่ทุกครั้ง (`voicebankLoadMs` เดียวกันคือ 625 ms)

**2. การโหลดเฉพาะ chunk ให้กำไรมากกว่า — 23x**

C เทียบ A: worker พร้อมใน **27 ms แทน 625 ms**
`chunk_wav_keys(i)` บอกว่า chunk นั้นใช้ sample ไหน (ปกติ 1–3 ตัว จาก 558)

และไม่ใช่แค่เวลาโหลด — render ก็เร็วขึ้นด้วย (B 2537 → D 2159 ms)
เพราะ worker ที่ถือ 198 MB มี memory footprint ใหญ่กว่า

**3. Pool + เฉพาะ chunk (D) ดีที่สุด แต่ C ไล่ตามมาติดๆ**

C (2332/2201) vs D (2159/2170) ต่างกันแค่ ~130 ms — เพราะพอมองข้ามการ
โหลดแล้ว pool เหลือแค่ประหยัด worker startup + JIT warmup

## Memory — ข้อที่สำคัญบนแท็บเล็ต

| แบบ | ต่อ worker | 4 workers |
|---|---|---|
| โหลดทั้ง bank | ~198 MB | **~790 MB** |
| เฉพาะ chunk | ไม่กี่ MB | ~10 MB |

บน Mi Pad การโหลดทั้ง bank ต่อ worker อาจไม่ผ่านเลย — ข้อนี้ตัดสินได้เลย
ว่า**ต้องใช้ per-chunk loading**

## Incremental re-render (เคสจริงของ UI)

pool warm (เฉพาะ chunk) render ใหม่ทีละ chunk:

```
E. pool warm — render ทีละ chunk : 1409 / 1929 / 2131 / 1398 ms  (chunk 0..3)
   pool warm — render ทั้งเพลง   : 2208 / 2177 ms
```

**ข้อควรรู้:** แก้ note เดียวเร็วกว่า render ทั้งเพลงแค่ ~1–2x ไม่ใช่ 4x
เพราะ chunk ที่หนักที่สุด (chunk 2, 2131 ms) เกือบเท่า render ทั้ง 4 chunk
พร้อมกัน (2208 ms) — parallel ทำให้เพลงเต็มเร็วจนช่องว่างหด

กำไรจริงของ incremental คือ **CPU/แบตเตอรี่** (1 chunk แทน 4) และ
**จำนวน worker ที่ต้องปลุก** ไม่ใช่เวลา wall-clock

ถ้าไม่ใช้ worker เลย incremental ยังชนะชัด: 6860 → 1398 ms (4.9x)

## สรุปคำแนะนำ

1. **ทำ per-chunk sample loading ก่อน** — `engine.chunk_wav_keys(i)` มีแล้ว
   กำไร 625 → 27 ms ต่อ worker และลด RAM จาก ~790 MB เหลือ ~10 MB
2. **pool worker ไว้** — กำไรที่เหลือ ~130–620 ms ต่อ render
   และเลี่ยง JIT warmup ซ้ำ
3. เรียงตามความคุ้ม: per-chunk loading ≫ pool (เมื่อโหลดทั้ง bank) >
   pool (เมื่อโหลดเฉพาะ chunk)

## วิธีวัดซ้ำ

```sh
# fixture: copy voicebank จริงเข้า web (symlink ไม่ได้บน exfat)
mkdir -p web/fixture/full/voice
cp test/golden/teto-english/library/voice/oto.ini web/fixture/full/
cp test/golden/teto-english/library/voice/* web/fixture/full/voice/
rm -rf web/pkg && ./web/run-parallel.sh   # สร้าง pkg
python3 web/bench-server.py &             # เปิด /pool-full.html?run=1
```

(ใช้ `bench-server.py` เพราะ `performance.now()` หยุดใต้ `--virtual-time-budget`)


---

# RenderPool API candidate (`web/pool.js`)

`web/pool.js` รวมทุกอย่างที่วัดไว้ข้างบนเป็นคลาสเดียวที่ host เรียกได้. ตอนนี้
ผ่าน harness แล้ว แต่ `web/app/editor.js` ยังไม่ได้ wire เข้ามา.

```js
const pool = await RenderPool.create({ workerCount: 4, bank, otoPath, otoDir, projectYaml });
await pool.render();          // คืน mixed PCM (bit-identical กับ full render)
await pool.setProject(editedYaml);
await pool.render();          // render เฉพาะ chunk ที่ hash เปลี่ยน
pool.toWav();                 // export จาก cache (ไม่ render ซ้ำ)
```

## หลักการ 3 ข้อ

1. **main thread ไม่โหลด audio** — `plan_project` / `chunk_wav_keys` ใช้แค่
   alias table จาก `oto.ini` จึงวางแผนได้โดยไม่มี sample เลย
2. **worker โหลดเฉพาะ sample ที่ chunk ตัวเองใช้** — `keysFor(indices)` แล้ว
   `bank.load(keys)` ส่งให้เฉพาะ worker นั้น (worker ไม่มี sample ตอนเกิด)
3. **cache key = Rust chunk hash** — `chunk_meta_json()` ส่ง hash มาให้ JS
   จึงใช้ hash เป็น key ของ cache ได้ตรงกับที่ Rust ใช้ตัดสินใจ

## ผลทดสอบ (`web/run-pool.sh`, scale-demo 14.5s, 4 workers)

| step | เวลา | chunks render ใหม่ | samples โหลด |
|---|---|---|---|
| 1. cold render | 2217 ms | 4 | **2** (จาก 558) |
| 2. render ซ้ำ (ไม่มีอะไรเปลี่ยน) | **3 ms** | 0 (reuse 4) | 0 |
| 3. แก้ note เดียว | 2078 ms | **1 จาก 4** (reuse 3) | 0 |
| 4. export WAV จาก cache | **5.2 ms** | 0 | 0 |

**ทุก step bit-identical กับ full render** (`max|diff| = 0.0e+0`) และการแก้ note
มีผลจริง (max|diff| ระหว่าง v1/v2 = 7.36e-1)

อ่านตัวเลข: **step 2 คือกำไรจริงของ cache** — กด play ซ้ำหลัง render เสร็จ
เร็ว 3 ms (แค่ re-mix) ไม่ใช่ 2.4 วินาที

## Bug ที่เจอระหว่างทำ (แก้แล้ว)

**`Chunk::hash()` ไม่ครอบคลุม expression curves และ pitch**

`tools/synth-cli/src/chunking.rs` เดิม hash แค่
`phoneme, position_ms, duration_ms, tone, tone_shift` ของ `RenderPhoneme`
แต่ DYN/GENC/TENC/BREC/VOIC ถูก sample ลง `input.curves` และ PITD ลง
`input.pitches_cents` — **ไม่มีอยู่ใน phoneme record**

ผลคือ: **แก้ curve แล้วได้ PCM เก่า** ทั้ง native cache (`res-{hash}.bin`)
และ wasm chunk cache เพราะ audio เปลี่ยนแต่ key ไม่เปลี่ยน

แก้ให้ hash ครอบคลุม:
- phrase frame (`position_ms/duration_ms/leading_ms/leading_ticks/time_axis_hint`)
  — เป็น origin ของการ sample curve
- ทุก field ของ phoneme ที่ส่งถึง renderer (`leading_ms`, `overlap_ms`,
  `index`, `parent_note` ที่เดิมขาด)
- oto entry เบื้องหลัง phoneme (`offset/cutoff/consonant/preutter/overlap/
  envelope/flags/wav_path`) — เดิมไม่ hash เลย
- pitch + curves เฉพาะ window ที่ chunk นั้น sample (`curve_window`)

`runtime::feed_hash` มี `HashKey` ครบสำหรับ type เหล่านี้อยู่แล้ว แต่**ไม่เคย
ถูกเรียกใช้จาก pipeline** — ตอนนี้ key ที่ใช้จริงครอบคลุมเทียบเท่าแล้ว

**การ clamp window สำคัญ:** `curve_window` อาจคำนวณเกินขนาด array ที่ builder
สร้าง ถ้า slice ตรงๆ จะได้ slice **ว่าง** → key บอดต่อการแก้ curve นั้น
(`hash_window` จึง clamp ให้ non-empty array hash อย่างน้อย 1 สมาชิกเสมอ)
ระหว่างทำ test จับ bug นี้ได้เอง (`hash_changes_when_curves_change` fail)

Regression tests: `hash_changes_when_curves_change`, `hash_changes_when_pitch_changes`,
`hash_changes_with_engine_salt`, `hash_is_stable_for_identical_input`
(`tools/synth-cli/src/chunking.rs`)

## API ที่เพิ่ม

| API | ทำอะไร |
|---|---|
| `Engine.chunk_wav_keys(i)` | wav key ที่ chunk i ต้องใช้ (รวม context) |
| `chunk_meta_json()` มี `hash` | cache key ของ chunk (u64 เป็น string) |
| `Mixer.finish_wav()` | mix → WAV (export โดยไม่ render ซ้ำ) |
| `RenderPool.toWav()` | WAV จาก chunk cache |

## งานที่ยังเหลือ

- **worker pool ยังไม่ reuse ข้าม `setProject`** — pool คงไว้แล้ว แต่
  `render()` เรียก `replan` ทุก worker ทุกครั้ง (ถูกแต่มี overhead เล็ก)
- **cancel ระหว่าง render** — `render_chunk` ยังไม่ wire cancel flag
- **multi-track** — `Mixer.add_chunk` ใช้ `TrackSpec::default()` (volume 0/pan 0)
  จึงยังทำ mute/solo/fader ไม่ได้
- **editor integration** — `web/app/editor.js` ยังใช้ main-thread `driver.renderAll()`;
  ต้องต่อ pool/progress/cancel เข้า UI ก่อนนับเป็น production path
