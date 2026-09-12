# WASM Performance — ทำไม wasm ช้ากว่า native และแก้ได้แค่ไหน

> วัดเมื่อ 2026-09-10 · AMD Ryzen 5 8600G (12 threads) · Chromium 152 headless
> โปรเจกต์: `test/golden/scale-demo-app.ustx` (29 notes, 14.50s audio, 4 chunks)

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
