# WASM Performance — ทำไม wasm ช้ากว่า native และแก้ได้แค่ไหน

> วัดเมื่อ 2026-09-10 · AMD Ryzen 5 8600G (12 threads) · Chromium 152 headless
> โปรเจกต์: `test/golden/scale-demo-app.ustx` (29 notes, 14.50s audio, 4 chunks)
> **วัดซ้ำ 2026-09-23 (§0)** — เครื่องเดิม, Chromium 153, เคลียร์ cache ก่อนทุกรอบ

---

## 0. วัดซ้ำ 2026-09-23 — native vs wasm (ของเดียวกันเป๊ะ)

โปรเจกต์ + bank เดียวกัน (`scale-demo-app.ustx` + Teto bank จริง 558 wav), engine `r1-rs`
ทั้งสองฝั่ง, **เคลียร์ `<voicebank>/../cache` ก่อนทุกการวัด** (ไม่งั้นได้ 0.05 s จาก cache hit),
peak RSS สุ่มอ่าน `VmHWM` จาก `/proc/<pid>/status`, และยืนยัน output **bit-identical**
ระหว่าง native 1 thread / native parallel / wasm (`cmp` ผ่านทั้งหมด)

| config | เวลา | realtime | peak RSS |
|---|---|---|---|
| native `r1-rs` 1 thread (`RAYON_NUM_THREADS=1`) | **4.10 s** | 3.54× | 61.9 MB |
| native `r1-rs` parallel (rayon, 12 threads) | **1.31 s** | 11.07× | 156.5 MB |
| native `r1` ผ่าน `.so` (C++ kernel, parallel) | **1.25 s** | 11.62× | 167.3 MB |
| **wasm sequential** (main thread, SIMD) | **6.82 s** | 2.13× | — |
| **wasm pool** (4 Web Workers) | **2.16 s** | 6.72× | — |

- **wasm ช้ากว่า native ต่อ 1 thread = 6819/4100 = 1.66×** — ตรงกับ 1.64× ของรอบ 2026-09-10
  ⇒ penalty ของ wasm คงที่; ที่เปลี่ยนคือของทั้งสองฝั่งเร็วขึ้น (native 1t 6375→4100 ms,
  wasm 1t 10479→6819 ms)
- parallelism: native 4.10/1.31 = **3.13×** · wasm 6819/2159 = **3.16×** (plan มีแค่ 4 chunks
  ⇒ เพดานคือจำนวน chunk, ไม่ใช่จำนวน worker/thread)
- หมายเหตุ: `r1` (.so) กับ `r1-rs` ให้จำนวน sample เท่ากันแต่ **ไม่ bit-identical** กัน
  (คนละ engine: C++ WORLD vs pure-Rust port) — สำหรับการเทียบ wasm ให้ใช้ `r1-rs` เท่านั้น
- **cache ชนะความเร็วทุกอย่าง**: รอบที่ cache อุ่น native ใช้ **0.05 s** เทียบ 1.31–4.10 s
  ⇒ สำหรับงาน interactive cache key ถูกต้องสำคัญกว่าความเร็วดิบ (ดูบั๊ก cache salt
  ไม่ผูกกับโมเดลใน BACKLOG #7)

**วิธีวัดซ้ำ (2026-09-23):** รัน native ด้วย python wrapper ที่ `shutil.rmtree` cache ก่อนทุกรอบ
และ sample `VmHWM` ทุก 20 ms; ฝั่ง wasm ใช้ `PROJECT=/test/golden/scale-demo-app.ustx ./web/run-real.sh`
(หน้าเว็บรายงาน `sequential` กับ `pool` เป็น wall-clock จริง ไม่ใช้ `--virtual-time-budget`)

### 0.1 คุม parallelism ให้เท่ากัน + วัด CPU จริง

การเทียบ "native 12 threads vs wasm 4 workers" **ไม่แฟร์** — matrix นี้ตอบว่าถ้าคุมให้เท่ากันแล้วเป็นอย่างไร
(native: `RAYON_NUM_THREADS` 1→12 · wasm: `workers` 1→8 · เคลียร์ cache ทุกรอบ)

**native `r1-rs` (scale-demo, 14.50 s audio, 4 chunks):**

| RAYON | wall | CPU (user+sys) | cores ที่ใช้จริง | efficiency | peak RSS |
|---|---|---|---|---|---|
| 1 | 4.09 s | 4.04 s | 0.99 | 99% | 61.8 MB |
| 2 | 2.13 s | 4.06 s | 1.90 | 95% | 91.3 MB |
| **4** | **1.31 s** | 4.19 s | **3.20** | 80% | 139.8 MB |
| 6 | 1.29 s | 4.16 s | 3.23 | 54% | 145.2 MB |
| 8 | 1.31 s | 4.17 s | 3.19 | 40% | 142.0 MB |
| 12 | 1.29 s | 4.17 s | 3.24 | 27% | 138.3 MB |

⇒ **งานทั้งหมด = ~4.1 CPU-seconds คงที่** ไม่ว่าจะขอ thread เท่าไร และเวลา **อิ่มตัวที่ 4 threads**
(1.31 s) เพราะ plan มีแค่ 4 chunks — ขอ 6/8/12 ได้ 0 การปรับปรุง มีแต่ efficiency ตก

**wasm (หน้าจริง, วัด CPU ของ process tree ของ Chromium ทั้งก้อน):**

| workers | render-only (หน้าเว็บ) | CPU ทั้งหน้า | cores เฉลี่ยทั้งหน้า | peak RSS ของ tree |
|---|---|---|---|---|
| 1 | 6,685 ms (seq 6,744) | 14.88 s | 1.08 | 1,325 MB |
| 2 | 3,461 ms | 14.91 s | 1.41 | 1,385 MB |
| **4** | **2,125 ms** | 15.08 s | 1.64 | 1,470 MB |
| 8 | 2,114 ms | 15.26 s | 1.65 | 1,514 MB |

- render-only scaling: 6685 → 3461 → 2125 ms (**1.93×, 3.15×**) แล้ว **อิ่มตัวที่ 4 workers**
  (workers 5–8 ได้ keys `0,0,0,0` = ว่างงาน) — เพดานเดียวกันคือ **จำนวน chunk (4)**
- cores เฉลี่ย "ทั้งหน้า" ต่ำ (1.64) เพราะรวมช่วง boot/fetch/export ที่ single-thread;
  เฉพาะช่วง render ≈ (15.08 − ~7.1) / 2.13 ≈ **3.8 cores จาก 4 workers** = utilization เกือบเต็ม
- **RSS 1.3–1.5 GB คือทั้ง process tree ของ Chromium** (โปรไฟล์ใหม่, ยังไม่รวม GPU process ที่ถูกปิด)
  ไม่ใช่ heap ของ engine — ใช้เทียบกับ native RSS ไม่ได้ตรง ๆ

**เทียบที่ parallelism เท่ากัน:**

| | native | wasm | wasm/native |
|---|---|---|---|
| 1 ทาง | 4.09 s | 6.69 s | **1.63×** |
| 4 ทาง | 1.31 s | 2.13 s | **1.62×** |

⇒ **อัตราส่วน 1.6× คงที่ไม่ว่า parallelism เท่าไร** และทั้งสองฝั่งอิ่มตัวที่ 4 ทางเพราะ plan มี 4 chunks
จะเทียบที่ 4+ ทางต้องใช้เพลงที่ chunks เยอะกว่านี้ (เช่น สร้างโปรเจกต์ยาวจาก scale-demo หลายรอบ)

### 0.2 เพลงยาว: Bad Apple (867 notes, 136 chunks, 504.23 s = 8.4 นาที, 0 skipped)

โปรเจกต์สร้างจาก `test/golden/make-badapple-ustx.py` (จาก `app/lib/data/badapple_demo.dart` — UST จริง tempo 138)
ใช้ Teto bank จริง 558 wav (ต้องโหลด **47** wav)

| ฝั่ง | config | wall | CPU | cores จริง | RSS |
|---|---|---|---|---|---|
| wasm | 1 worker | 109.44 s | 111.31 s | 1.01 | 1.83 GB (tree) |
| wasm | 4 workers | 32.37 s | 100.35 s | 3.00 | 2.07 GB |
| **wasm** | **8 workers** | **16.75 s** | 115.92 s | **6.60** | 2.36 GB |
| native | 1 thread | 46.65 s | 46.32 s | 0.99 | 416 MB |
| native | 4 threads | 36.11 s | 62.75 s | 1.74 | 442 MB |
| native | 12 threads | 37.57 s (อีกรอบ 26.4 s) | 64.80 s | 1.72 | 442 MB |

- **wasm ขยายได้เกือบเต็ม: 1→8 workers = 6.53×** (avg cores 6.6/8 = 82%)
- **native แทบไม่ขยาย: 1→12 = 1.24×** และ CPU *เพิ่ม* จาก 46 → 65 CPU-s ⇒ มีงานงอก/contention
  (คนละพฤติกรรมกับ scale-demo ที่ native ขยาย 3.1×) — ต้องสืบต่อว่า rayon/plan/cache ตรงไหนเป็นคอขวด
- ⇒ บนเพลงจริง **wasm 8 workers เร็วกว่า native 12 threads** (16.75 vs 26.4–37.6 s ตามภาระเครื่อง)
  ⇒ **จุดที่ควรลงทุนไม่ใช่ wasm แต่เป็น parallelism ฝั่ง native**

**ความต่างของผลลัพธ์ native vs wasm (ตรวจแล้ว)**:
- native deterministic 100%: 1t == 4t == 12t (sha `9f95e0ce20cc7279`)
- wasm deterministic ข้าม worker count: 1/4/8 workers ให้ WAV เดียวกัน (fnv `1ffe7b40`)
- native vs wasm: **3 จาก 22,236,513 sample ต่างกัน 1 LSB** (max|diff| = 1, ครั้งแรกที่ sample 1,264,479)
  ⇒ ไม่ใช่บั๊กโครงสร้าง — เป็น float rounding ข้ามเส้น quantize (SIMD/FFT path ต่างกัน)
  ⇒ คำว่า "bit-identical" ใช้ได้กับโปรเจกต์สั้น (scale-demo, sakura); เพลงยาวให้ถือเกณฑ์ **≤1 LSB, ไม่กี่ sample**
- ทั้งสองฝั่ง peak = 1.0000 ⇒ เพลงนี้ **clip** (เรื่อง gain/mixer ควรดูแยก)

---

## สรุป

"wasm ช้ากว่า native 5 เท่า" ที่เห็นตอนแรก **เป็นการเทียบที่ไม่แฟร์** —
native รัน 4 chunks บน 12 cores ส่วน wasm เป็น single-thread

พอหักลบแล้วเหลือ **1.6x** ซึ่งเป็นค่าปกติของ wasm สำหรับ DSP f64
และในระหว่างหาสาเหตุเจอ **bug ประสิทธิภาพจริงใน DSP** ที่แก้แล้วได้ 30-35%

---

## 1. หักลบ parallelism

| config | เวลา | realtime |
|---|---|---|
| native, 12 cores (default, 4 chunks ขนาน) | 1947 ms | 7.5x |
| native, `RAYON_NUM_THREADS=1` | 6375 ms | 2.3x |
| wasm (single-thread) | 10479 ms | 1.4x |

- parallelism = 6375 / 1947 = **3.3x**
- wasm vs native single-core = 10479 / 6375 = **1.64x**

**wasm ทำ parallel ไม่ได้ในตัว engine** เพราะไม่มี thread pool
(`parallel` feature ปิด) — **แต่ host ทำได้** ด้วยการ fan chunk ออกไปยัง
Web Worker: ดู **[wasm-parallel.md](wasm-parallel.md)** (4 workers → 2.66x,
ผล bit-identical, ไม่ต้องใช้ COOP/COEP)

`wasm-bindgen-rayon` (shared-memory threads) ยังทำไม่ได้ในเครื่องนี้เพราะต้อง
nightly + `-Z build-std` + `rust-src` ซึ่ง sandbox นี้เขียน `~/.rustup` /
`~/.cargo` ไม่ได้ (ดูรายละเอียดใน wasm-parallel.md)

---

## 2. หาว่าเวลาหมดไปกับ stage ไหน (ก่อนแก้)

ใส่ timing env-gated (`WORLD_RS_TIME=1`) แล้ววัด native sequential:

| stage | เวลา | สัดส่วน |
|---|---|---|
| D4C (aperiodicity) | 1105–1424 ms | **73–81%** |
| synthesis (WORLD pulse + FFT) | 170–409 ms | 12–21% |
| CheapTrick (envelope) | 84–108 ms | 5–6% |
| analysis (f0/trim/remap) | ~1.3 ms | 0.1% |

**FFT ไม่ใช่คอขวด** — อธิบายว่าทำไมเปิด `wasm_simd` ของ rustfft
(ยืนยันว่าทำงานจริงด้วย `engine.fft_backend()` → `"wasm-simd"`)
แล้วได้แค่ **~9%** (10479 → 9503 ms)

---

## 3. Bug ที่เจอ: FFT planner ถูกสร้างใหม่ในลูป

`crates/world-rs/src/d4c.rs` เดิมเรียก:

```rust
fn power_spectrum(input: &[f64]) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();   // <-- สร้างใหม่ทุกครั้ง
    let fft = planner.plan_fft_forward(n);        // <-- plan ใหม่ทุกครั้ง
    ...
}
```

`power_spectrum` ถูกเรียก **ต่อ frame** จาก `love_train` + `smoothed_power`
+ อีก `coarse_count` (5) ครั้งใน band loop — รวม **~7-8 planner + plan
ต่อ frame** และ `static_centroid` สร้าง planner อีก 1 ตัว

`rustfft` planning มีค่าใช้จ่ายสูง (เลือก algorithm + คำนวณ twiddle)
และมันเกิดขึ้นซ้ำกับขนาดเดิม (4096) ทุก frame

**แก้:** thread-local plan cache ใน `crates/world-rs/src/fft.rs`
(`forward_plan()` / `inverse_plan()`) — เปลี่ยนเป็น hash lookup

> หมายเหตุ: D4C ยังกินสัดส่วนสูงสุด (58–70%) หลังแก้ — ยังมีที่ให้ optimize อีก
> (การ allocate `Vec` ต่อ band ต่อ frame, `sort_by` ด้วย `partial_cmp`)

---

## 4. ผลหลังแก้

| config | ก่อน | หลัง | เร็วขึ้น |
|---|---|---|---|
| native sequential | 6375 ms | **4304 ms** | 32% |
| native 12 cores | 1947 ms | **1383 ms** | 29% |
| wasm (SIMD) | 10479 ms | **6826 ms** | 35% |
| wasm demo-song (2.99s audio) | 835 ms | **534 ms** | 36% |

หลังแก้:
- native parallel = **10.5x realtime**
- native sequential = **3.4x realtime**
- wasm = **2.1x realtime**
- wasm vs native single-core = 6826 / 4304 = **1.59x** (แทบไม่เปลี่ยน)

**ยืนยันความถูกต้อง:** plan cache ไม่เปลี่ยนผลลัพธ์
- `engine-wasm`: in-memory vs disk render ยัง `max|diff| = 0.000e0`
- parity tests 8/8 ผ่าน (`rust_kernel_matches_so_*`, `rust_classic_*`, `rust_pyin_*`)

---

## 5. wasm gotcha ที่เจอ

`std::time::Instant::now()` **panic บน wasm32-unknown-unknown**:

```
panicked at library/std/src/sys/time/unsupported.rs:13:9:
time not implemented on this platform
```

wasm ไม่มี clock — instrumentation ต้อง cfg-gate
(`phrase.rs` ใช้ `timer_start()/timer_ns()` ที่คืน `None`/0 บน wasm)

และ wasm panic จะโผล่เป็น `RuntimeError: unreachable` เฉยๆ ไม่มีข้อความ
(`panic = "abort"`) — `engine-wasm` จึงติดตั้ง panic hook เก็บข้อความไว้
ให้อ่านผ่าน `last_panic()`

---

## 6. ตัวเลขที่ควรได้บนแท็บเล็ต

ยัง**ไม่ได้วัด**บน Mi Pad — desktop 12-thread ได้ 2.1x realtime
ถ้าแท็บเล็ตช้ากว่า ~2x จะเหลือ ~1x realtime ซึ่งแปลว่าเพลง 3 นาที
render รอบแรก ~3 นาที

ควรวัดก่อนตัดสินใจย้าย UI (ดู `docs/planning/wasm-migration-status.md`)

---

## วิธีวัดซ้ำ

```sh
cd native
# native (sequential vs parallel)
cargo build --release -p world-oracle
RAYON_NUM_THREADS=1 ./target/release/world-oracle bench <project.ustx> <voicebank-dir>
./target/release/world-oracle bench <project.ustx> <voicebank-dir>

# stage split
WORLD_RS_TIME=1 RAYON_NUM_THREADS=1 ./target/release/world-oracle bench <project> <bank>

# wasm (real wall-clock — --virtual-time-budget freeze performance.now())
cd ../web && python3 bench-server.py    # แยกหน้าต่าง แล้วเปิด /bench-post.html
```
