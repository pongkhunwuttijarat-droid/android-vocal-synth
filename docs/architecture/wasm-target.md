# Web / wasm Target — engine ทั้งก้อนบน browser

> สถานะ: **engine core รันบน wasm32-unknown-unknown ได้ + render ตรงกับ native ทุก bit**
> อัปเดต: 2026-09-12

---

## สรุป

engine stack ทั้งเส้น compile เป็น `wasm32-unknown-unknown` ได้แล้ว และ
render ผลออกมา **เหมือน native เป๊ะ** (`max|diff| = 0.000e0`) ผ่าน
`engine-wasm` (wasm-bindgen entry point)

มี browser editor static HTML/Canvas แล้วในระดับ vertical slice
(`web/editor.html` + `web/app/*`) และผ่าน headless interaction test. ยังไม่ใช่
product-complete: editor ยัง render main thread และยังขาด OPFS project flow,
multi-track mix, expression lanes, PWA และ neural engines.

---

## crate ที่ wasm ได้ (ทดสอบแล้ว)

| crate | wasm | หมายเหตุ |
|---|---|---|
| `domain` | ✅ | |
| `ustx-codec` | ✅ | |
| `phonemizer` | ✅ | |
| `feed` | ✅ | |
| `phrase` | ✅ | |
| `voicebank` | ✅ | + `Voicebank::from_oto_bytes` (in-memory) |
| `wavwriter` | ✅ | |
| `storage` | ✅ | |
| `runtime` | ✅ | mixer |
| `world-rs` | ✅ | WORLD DSP kernel |
| `worldline-plugin` | ✅ | `--no-default-features` (ตัด FFI) |
| `worldline-rs` | ✅ | PhraseSynth + Classic resampler |
| `mixer-fx` | ✅ | `--no-default-features` (pure-Rust chain) |
| `synth-cli` | ✅ | `--no-default-features` (orchestration) |
| `engine-wasm` | ✅ | wasm-bindgen entry point |

## crate ที่ wasm **ไม่ได้** (โดยตั้งใจ)

| crate | เหตุผล |
|---|---|
| `worldline-sys` | `libloading` (dlopen) — browser ไม่มี |
| `mixer-fx` (ffi feature) | `libloading` |
| `diffsinger`, `moe-train`, `trainer-gui` | `ort` (ONNX runtime) + `burn`/`wgpu` |
| `synth-server`, `synth-cli` (cli bin) | tokio/axum/clap — ไม่จำเป็นใน browser |
| `tools/*` | host-side tooling |

---

## การเปลี่ยนโค้ดที่ทำให้ wasm ได้

### 1. Feature-gate dlopen ทั้งหมด

เดิม `worldline-plugin` และ `mixer-fx` ผูกกับ `libloading` แบบ hard dependency
ทำให้ compile wasm ไม่ผ่านเลย แก้เป็น:

```toml
# worldline-plugin
worldline-sys = { path = "../worldline-sys", optional = true }
[features]
default = ["ffi"]          # native
ffi = ["dep:worldline-sys"] # dlopen — ปิดสำหรับ wasm
r2 = ["dep:ort", "ffi"]
```

`synth-cli` ก็เหมือนกัน:

```toml
[features]
default = ["ffi", "r2", "parallel"]  # native
ffi = ["worldline-plugin/ffi", "mixer-fx/ffi"]
parallel = ["dep:rayon"]   # wasm ไม่มี thread pool
r2 = ["dep:ort", "dep:diffsinger", "dep:moe-train", "dep:burn", ...]
cli = ["dep:clap"]
```

### 2. แยก mixer เป็น trait

`pipeline::render_project` เดิมรับ `Option<&mut mixer_fx::MixerFx>` (`.so`)
ทำให้ wasm ผ่านไม่ได้ แก้เป็น `Option<&mut dyn PostMixFx>` โดยมี 2 impl:

- `MixerFx` (dlopen, native)
- `MixerChain` (pure Rust, ทุก target) — **port ใหม่**

### 3. Port mixer FX chain เป็น pure Rust

`crates/mixer-fx/src/chain.rs` — port จาก `plugins/mixer-fx/mixer_fx.cpp`
(221 บรรทัด C++): gain → 3-band EQ (RBJ cookbook) → compressor → tanh clip
สูตรและลำดับเหมือน C++ ทุกตัว

### 4. Voicebank ในหน่วยความจำ

`Voicebank::from_oto_bytes(id, name, oto_dir, oto_ini_bytes)` — สร้าง alias
table จาก bytes โดยไม่อ่านดิสก์ (browser เก็บ voicebank ใน OPFS)

### 5. SampleSource (seam ของ filesystem)

`crates/worldline-rs/src/source.rs`:

```rust
pub trait SampleSource {
    fn resolve(&self, wav_path: &str) -> Result<ResolvedSample, String>;
}
```

- `FileSource` — native (อ่าน `<voicebank>/voice/*.wav` + `*_wav.frq`)
- `MemorySource` — browser/ทดสอบ (host push samples เข้ามา)

`render_phrase_with_source(input, curve_frame, model_frame, &source)` คือ
entry point ที่ไม่แตะ `std::fs` เลย

### 6. Project parsing จาก string

`pipeline::parse_project(yaml)` — แยกจาก `load_project(path)` เพื่อให้
browser parse project จาก OPFS ได้โดยไม่ต้องมี path

---

## `engine-wasm` API

```ts
import init, { Engine } from "./engine_wasm.js";
await init();

const engine = new Engine();

// 1. voicebank: oto.ini + wav + frq (จาก OPFS / fetch)
engine.set_voicebank("teto-english", "Teto", "voice", otoIniBytes);
for (const key of engine.wav_keys().split("\n")) {   // "voice/xxx.wav"
  engine.add_sample_with_frq(key, wavBytes, frqBytes); // frq จำเป็นต่อความตรง
}

// 2. project (YAML จาก OPFS)
const pcm = engine.render_project(ustxYaml, 0);        // Float32Array
const wav = engine.render_project_wav(ustxYaml, 0);    // Uint8Array (WAV)
```

**สำคัญ:** `add_sample_with_frq` — UTAU bank ส่ง `<stem>_wav.frq` คู่ทุก wav
และ reference **เลือกใช้ frq เสมอ** ถ้าไม่ส่ง ผลจะต่างจาก native (วัดได้
`max|diff| = 1.588` เทียบกับ `0.000e0` เมื่อส่ง)

---

## ผลทดสอบ

`crates/engine-wasm/tests/memory_render.rs` (รัน native แต่ใช้ code path
เดียวกับ browser):

| test | ผล |
|---|---|
| `memory_source_render_matches_disk_render` | 132066 = 132066 samples, **max\|diff\| = 0.000e0** |
| `wav_output_round_trips` | WAV decode กลับได้, >1s, ไม่เงียบ |
| `memory_renderer_renders_a_phrase_from_loaded_samples` | phrase render ผ่าน MemorySource |

**ใน-memory path = disk path ทุก bit** (ไม่ใช่แค่ใกล้เคียง)

ขนาด wasm (release):

| ไฟล์ | ขนาด | gzip |
|---|---|---|
| `engine_wasm_bg.wasm` | 1.43 MB | 508 KB |

---

## pYIN fallback (port เพิ่มในรอบนี้)

`crates/worldline-rs/src/pyin.rs` — port **libpyin** (`pyin.c` / `yin.c` /
`math-funcs.c`) + **libgvps** routine ที่มันใช้
(`gvps_sparse_sampled_hidden_static` — Viterbi บน sparse HMM)

reference ใช้ pYIN ทุกครั้งที่ sample **ไม่มี `.frq`** (`PhraseSynth::AddRequest`
เลือก `FrqEstimator` เมื่อมี, ไม่งั้น `PyinEstimator`) — bank ที่ bundle มาส่ง
`.frq` ครบ แต่ bank ที่ user import อาจไม่มี ถ้าไม่ port ตรงนี้ engine Rust
จะตกไปใช้ DIO แล้วผลต่างจาก `.so`

**gvps ใน C เป็น macro-generated family** (`#include` ซ้ำใน
`gvps_viterbi.hc` 8 variant) — port เฉพาะ variant ที่ reference เรียก:
`SAMPLED + STATIC + HIDDEN`, non-circular, sparse

เทียบ `.so` ผ่าน `world-oracle pyin` (ใช้ `F0(..., method=2)`):

| ตัวชี้วัด | ผล |
|---|---|
| frames | 303 = 303 |
| voiced frames | 261 = 261 |
| voicing agreement | **100.0%** |
| mean \|cents\| | **0.00** |
| max \|cents\| | **0.00** |

**ตรงกันทุกค่าถึงระดับ f64**

Regression test: `rust_pyin_matches_libpyin_through_the_so`

---

## Classic resampler (port เพิ่มในรอบนี้)

`crates/worldline-rs/src/resampler.rs` — port `classic/resampler.cpp`
(194 บรรทัด C++): `Resample()` + `ApplyPitch` + `ApplyEffects`

- frame คงที่ 10 ms, padding 2 frame (เหมือน reference)
- pitch = `440 × 2^(1/12)^(tone + bend×0.01 - 69)`
- flags: `g` (gender), `Mt` (tension), `Mb` (breathiness), `Mv` (voicing)
- auto-gain วัดจาก audio **หลัง** trim (ต่างจาก phrase path)

ผลล่าสุดจาก `world-oracle classic`:

| metric | result |
|---|---|
| length parity | ✅ ทุก fixture |
| worst max\|diff\| | **9.11e-5** |
| status | ✅ oracle parity gate ผ่าน |

ตาราง per-phoneme รุ่นก่อนหน้านี้เป็นผลจาก fixture ก่อนแก้ pitch-bend boundary
และไม่ใช่ตัวเลข gate ล่าสุด; เก็บรายละเอียดการไล่สาเหตุไว้ใน git history.

---

## Performance

ดูรายละเอียดที่ **[wasm-performance.md](wasm-performance.md)**

| config (scale-demo 14.5s audio) | เวลา | realtime |
|---|---|---|
| native 12 cores | 1383 ms | 10.5x |
| native sequential | 4304 ms | 3.4x |
| wasm (SIMD + FFT plan cache) | 6826 ms | 2.1x |

**wasm vs native single-core ≈ 1.6x** — ที่เหลือของช่องว่างเดิมคือ
parallelism ที่ wasm ทำไม่ได้ (ไม่มี thread pool)

build wasm ด้วย `RUSTFLAGS="-C target-feature=+simd128"` เพื่อให้ rustfft
ใช้ f64 SIMD path (`web/run-smoke.sh` ใส่ให้แล้ว) — เช็คด้วย
`engine.fft_backend()` → `"wasm-simd"` / `"scalar"`

> ระหว่างหาสาเหตุเจอ bug ประสิทธิภาพจริงใน D4C: `FftPlanner::new()` ถูกสร้าง
> ใหม่ในลูปต่อ frame (~7-8 ครั้ง/frame) — แคช plan แล้วเร็วขึ้น 30-35%
> ทั้ง native และ wasm

---

## ข้อจำกัด / งานที่เหลือ

1. ~~Classic resampler ยังไม่ parity~~ — ปิดแล้ว (worst 9.11e-5)
2. ~~F0/pyin fallback ยังไม่ port~~ — ปิดแล้ว (100% ตรงกับ libpyin)
3. **Editor wiring** — vertical slice มีแล้ว แต่ยังต้องต่อ `RenderPool`,
   progress/cancel, OPFS/project browser, autosave และ export flow
4. **Neural engines (r2/diffsinger/moe)** — ยัง native-only (`ort`/`burn`);
   ตาม feasibility doc ต้องใช้ `burn-import` (ONNX → burn) หรือ `ort-web`
5. **ไม่มี threads ใน engine** — `parallel` feature ปิดสำหรับ wasm
   แต่ host fan chunk ออกไป Web Worker ได้: 4 workers → 2.66x,
   bit-identical (ดู [wasm-parallel.md](wasm-parallel.md))
6. **Deploy** — ยังไม่มี service worker / static host config
