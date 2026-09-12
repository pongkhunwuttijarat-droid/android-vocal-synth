# Worldline Rust Port — `r1-rs` engine

> สถานะ: **wire เข้า runtime แล้ว + ทดสอบเทียบ `.so` ผ่านทั้งหมด**
> อัปเดต: 2026-09-12

---

## สรุป

มี engine ใหม่ชื่อ **`r1-rs`** — WORLDLINE-R ที่รัน DSP ด้วย Rust ล้วน
**ไม่ต้อง dlopen `libworldline.so`** เลย แต่ให้ผลเหมือน `.so` ในระดับ
sample (relative RMS diff ~5e-5)

เส้นทางเดิม (`r1` = `.so`) ยังเป็น **default** — `r1-rs` เป็นตัวเลือก

```
synth-cli render --engine r1-rs ...
POST /render {"engine": "r1-rs"}     <- synth-server
```

---

## สถาปัตยกรรม

แยกเป็น 2 crate เพื่อให้ขอบเขตชัด:

```
crates/world-rs        <- DSP kernel (แยก stage ตาม WORLD)
  fft.rs               RealFFT
  cheaptrick.rs        spectral envelope
  d4c.rs               aperiodicity
  f0.rs                DIO + StoneMask
  synthesis.rs         Synthesis (periodic + aperiodic + overlap-add)
  analysis.rs          analysis chain

crates/worldline-rs    <- orchestration (port จาก C++ plugin)
  model.rs             Model: BuildF0/BuildSp/BuildAp/Trim/Remap/Synth
  phrase.rs            PhraseSynth: AddRequest/SetCurves/Synth
  timing.rs            classic/timing.cpp (GetTimeMapping ฯลฯ)
  effects.rs           ShiftGender / GetTensionCoefficients / AutoGain
  spline.rs            tk::spline natural cubic (ที่ effects ใช้)
  f0.rs                FrqEstimator + DIO fallback
```

Engine adapter: `synth_cli::engine::WorldlineRsEngine`
(`tools/synth-cli/src/engine.rs`) implement `PhraseRenderer` + `Engine`

### สิ่งที่ใช้ร่วมกับ `.so` path (ไม่ duplicate)

`r1` และ `r1-rs` ใช้ **input conversion ตัวเดียวกัน** จาก
`worldline-plugin`:

- `build_requests()` — `RenderInput` -> `PhonemeRequest[]`
- `sample_phrase_curves()` — expression curves ต่อ frame
- `validate_request()`

ดังนั้นสิ่งที่ต่างกันมีอย่างเดียวคือ **DSP kernel** — ทำให้เทียบกันได้ตรงๆ

---

## Frame grid (จุดที่พลาดตอนแรก)

C++ มี frame 2 ค่าที่ต่างกัน และต้องใช้ให้ถูกตัว:

| ค่า | ที่มา | ใช้กับ |
|---|---|---|
| `11.6` ms | `worldline_plugin::DEFAULT_FRAME_MS` | **sampling density** ของ expression curves (`SetCurves`) |
| `512000/44100 = 11.6099773...` ms | C++ `FrameMs()` -> `WORLDLINE_FRAME_MS_DEFAULT` | **model grid**: `Trim`, `Remap`, p0..p4 timing, `y_length` |

ใช้ `11.6` แทน `11.6099773` ทำให้ `round()` ที่ขอบ frame พลิก ->
output ยาวต่างกัน 416 samples (2496.2 -> 2505.6 ms) ทั้งที่ DSP ถูกต้อง

`worldline_rs::render_phrase(input, curve_frame_ms)` แยกสองค่าออกจากกัน
(`V2_MODEL_FRAME_MS` / `model_frame_ms()`)

`WORLD_FRAME_MS` env override ได้เหมือน C++ `FrameMs()`

---

## ผลการทดสอบเทียบ `.so`

### Stage-by-stage (`tools/world-oracle`)

เครื่องมือใหม่ `world-oracle` ใช้แยกว่า stage ไหนเพี้ยน:

```sh
world-oracle analysis  <wav>              # CheapTrick + D4C เทียบ .so
world-oracle synthesis <wav>              # Synthesis เทียบ .so
world-oracle phrase <project> <voicebank> # PhraseSynth เต็มเส้น
```

ผลกับ voicebank จริง (`_a+_ha+_a+_a+_a+-.wav`, 305 frames):

| Stage | ผล |
|---|---|
| Spectral envelope | corr = 1.000000000000, dB MAE = 0.000000, dB max = 0.000065 |
| Aperiodicity | corr = 0.999994329348, MAE = 0.000575, max = 0.012837 |
| Synthesis (features เดียวกัน) | corr = 0.999999999882, max abs diff = 1.7e-4, rms(diff) = -103.9 dBFS |

### Full phrase (`PhraseSynth` vs `worldline-rs`)

| โปรเจกต์ | .so samples | Rust samples | corr | rms(diff) | max abs diff |
|---|---|---|---|---|---|
| demo-song (1 phrase) | 110081 | 110081 | 0.999999998638 | 9.0e-6 (-100.9 dBFS) | 1.6e-4 |
| scale-demo (29 notes, 14.5 s) | 639489 | 639489 | 0.999999985399 | 3.5e-5 (-89.1 dBFS) | 5.5e-4 |
| demo-song, grid v1 (`WORLD_FRAME_MS=10`) | 110251 | 110251 | 0.999999999208 | 6.8e-6 (-103.3 dBFS) | 1.0e-4 |

**ทั้ง v1 (10 ms) และ v2 (11.61 ms) grid ตรงกัน**

### ผ่าน HTTP API (`synth-server`)

`POST /render` เทียบ `"engine":"r1"` กับ `"engine":"r1-rs"`:

| โปรเจกต์ | ผล |
|---|---|
| demo-song | 132066 = 132066 samples, rms_diff 1.0e-5, `audiocompare` = **PASS** |
| scale-demo | 639443 = 639443 samples, rms_diff 3.6e-5, `audiocompare` = **PASS** |

### Regression test ถาวร

`tools/synth-cli/tests/render_real.rs` (ignored by default):

```sh
WORLDLINE_SO=$PWD/native/build/build-linux/libworldline.so \
  cargo test -p synth-cli --test render_real -- --ignored --nocapture rust_kernel
```

- `rust_kernel_matches_so_on_demo_song`
- `rust_kernel_matches_so_on_scale_demo`

เกณฑ์: sample count เท่ากัน + relative RMS diff < 1e-3 + max abs diff < 5e-3

---

## ข้อจำกัดที่ยังเหลือ

1. ~~**F0 fallback**~~ — port libpyin + libgvps แล้ว; oracle ได้ voicing agreement
   100% และ mean/max cents 0.00 เทียบ `.so`.
2. ~~**Classic mode**~~ — pure-Rust Classic resampler ทำงานและ oracle parity ผ่าน
   (worst max diff 9.11e-5). Full flags/product-level song golden ยังเป็นงาน MS3.
3. **Platinum/R2 neural** — native path ยังใช้ ONNX/runtime; ยังไม่ย้าย neural
   vocoder ไป WASM.
4. **Product integration** — WASM engine/pool พิสูจน์แล้ว แต่ editor ยังไม่ต่อ
   worker pool, OPFS และ multi-track mix.
5. **Performance optimization** — มี baseline แล้ว แต่ยังไม่ได้ profiling/optimize
   kernel และยังต้องวัด memory/performance บน tablet จริง.

---

## Performance (วัดบน dev machine)

scale-demo (29 notes, 14.5 s audio, 4 phrases, ไม่มี cache):

| Engine | elapsed | max RSS |
|---|---|---|
| `r1` (.so, C++ -O2) | **1.39 s** | 172 MB |
| `r1-rs` (Rust, `--release`) | **2.06 s** | 144 MB |
| `r1-rs` (Rust, debug) | ~48 s | — |

release แล้ว Rust ช้ากว่า C++ แค่ **~1.5x** และใช้ RAM น้อยกว่าเล็กน้อย
ยังไม่ได้ทำ profiling/optimize (`world-rs` ใช้ `rustfft` แบบ scalar
ไม่ใช่ SIMD ที่ WORLD ใช้) — ยังมีที่ให้ปรับอีกถ้าต้องการ

สำหรับ browser benchmark, worker fan-out และ memory trade-off ดู
[wasm-performance.md](wasm-performance.md) และ [wasm-parallel.md](wasm-parallel.md).
