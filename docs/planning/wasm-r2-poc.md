# wasm-r2-poc — R2/MoE ฝั่ง Rust บน wasm32 + ตรวจ .mpk vocoder

> task: `task-2` (ทีม r2-wasm) · วันที่: 2026-09-23 · repo: `android-voice-synth` (Rusted UTAU)
> write scope: `native/crates/moe-train/**`, `docs/planning/wasm-r2-poc.md`
> หลักการ: ทุกข้ออ้างมี **คำสั่งเต็ม + ผลรันจริง/exit code** หรือ **path:line** กำกับ · ไม่สรุปเกินหลักฐาน

## 0. สรุปคำตอบ (ผ่าน/ไม่ผ่าน)

| # | ข้อ | ผล | หลักฐาน |
|---|---|---|---|
| a1 | `ort 2.0.0-rc.13` default features บน `wasm32-unknown-unknown` | **ไม่ผ่าน (ล้มจริง)** — `no prebuilt binaries available for target wasm32-unknown-unknown` | §1.2 (รันซ้ำในรีโปนี้) |
| a2 | `ort` แบบ `default-features = false` + `alternative-backend` → compile ผ่านบน wasm32 | **ผ่าน — รันจริงแล้ว** ใน crate เดี่ยว `/tmp/r2w-a2` (exit 0, ไม่ตั้ง RUSTFLAGS) | §1.3 |
| a3 | runtime ฝั่ง JS ที่ต้องใช้ | **`ort-web` 0.3.0+1.27 + assets จาก `onnxruntime-web` (Emscripten)** — ไม่ใช่ ONNX Runtime native | §1.4 |
| b1 | `cargo check -p moe-train --target wasm32-unknown-unknown --no-default-features` | **ผ่าน** exit 0 | §2.2 |
| b2 | native ไม่พัง: `cargo test -p moe-train` | **ผ่าน** 5 passed / 0 failed | §2.3 |
| b3 | (เกินโจทย์) build cdylib → ได้ `.wasm` จริง 16,395,838 B | **ผ่าน** exit 0 | §2.4 |
| c1 | `native/test-data/pc-nsf-hifigan.mpk` เป็น checkpoint burn จริง | **ผ่าน** (NamedMpkFileRecorder อ่านได้ + metadata ครบ) | §3.1–3.2 |
| c2 | เอามาทำ **burn vocoder** บน wasm ได้ไหม | **ไม่ได้** — เป็น acoustic (mel predictor) รุ่นเก่า ไม่ใช่ NSF-HiFiGAN | §3.3 |
| c3 | ใช้เป็น checkpoint **acoustic** บน wasm | 🟡 **ได้ในหลักการ ยังไม่พิสูจน์** (6.2 MB, MLP ล้วน, burn-ndarray link ผ่าน) แต่ต้องสร้างสถาปัตยกรรมเก่าให้ตรงและยังไม่รันจริง | §3.4 |

---

## 1. (a) `ort 2.0.0-rc.13` กับ `wasm32-unknown-unknown`

registry path ที่ใช้อ้างอิงทั้งหัวข้อ:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ort-2.0.0-rc.13/` (ย่อเป็น `ort/`)
และ `.../ort-sys-2.0.0-rc.13/` (ย่อเป็น `ort-sys/`)

### 1.1 กลไกที่ทำให้ wasm build ล้ม (สรุปสั้น)

1. `ort` เปิด `download-binaries` มาใน default features → `ort/ Cargo.toml:108-116`
   `default = ["std","ndarray","tracing","download-binaries","tls-native","copy-dylibs","api-27"]`
   และ `download-binaries = ["ort-sys/download-binaries"]` → `ort/Cargo.toml:118`
2. `ort-sys` เป็น crate ที่ `links = "onnxruntime"` และมี build script `build/main.rs` → `ort-sys/Cargo.toml:18-19`
3. ตาราง prebuilt (`ort-sys/build/dist.tsv`) **ไม่มีแถวของ wasm32 เลย** — มี 16 บรรทัด (header + 15 dist)
   เป้าหมายที่มีคือ `x86_64/aarch64-*` บน linux/darwin/windows เท่านั้น → `grep -c wasm build/dist.tsv` = `0`
4. `resolve_dist()` หา dist ไม่เจอ → `log::error!("no prebuilt binaries available for target {target} ...")`
   → `ort-sys/build/download/resolve.rs:96-108`
5. `log::error!` ของ ort-sys พิมพ์ `cargo::error=...` ซึ่ง Cargo ถือเป็น **build failure**
   → `ort-sys/build/log.rs` (`macro_rules! error` → `println!("cargo::error={}", ...)`)

### 1.2 หลักฐานเชิงประจักษ์ (รันในรีโปนี้)

```bash
cd native
cargo check -p moe-train --target wasm32-unknown-unknown      # default features → เปิด ort-vocoder + wgpu
```

ผลจริง (exit 101):

```text
   Compiling ort-sys v2.0.0-rc.13
error: ort-sys@2.0.0-rc.13: no prebuilt binaries available for target wasm32-unknown-unknown
error: build script logged errors
```

log ดิบ: `/tmp/r2w-6-wasmdefault.log` (บรรทัด 71-73)
→ ตรงกับข้ออ้างใน `docs/planning/web-render-backend.md` §1.3 แบบคำต่อคำ และเกิดที่ **build script** ของ ort-sys
(build script รันแม้ใน `cargo check` จึงล้มตั้งแต่ยังไม่แตะโค้ดของเรา)

### 1.3 path ที่ถูกต้อง + ทำไม

ทางที่ 1 — ตัด ort ออกทั้งหมด (วิธีที่ task นี้ใช้จริง, §2): `ort` เป็น optional dependency
และ build ด้วย `--no-default-features` → ort/ort-sys ไม่ถูกดึงเข้า graph เลย

ทางที่ 2 — ใช้ ort บน wasm จริง (`alternative-backend`):

```toml
ort = { version = "=2.0.0-rc.13", default-features = false,
        features = ["std", "ndarray", "api-27", "alternative-backend"] }
```

- `alternative-backend = ["ort-sys/disable-linking"]` → `ort/Cargo.toml:95`
- `disable-linking` ทำให้ build script ของ ort-sys `return` ทันที (ไม่ดาวน์โหลด ไม่ link) → `ort-sys/build/main.rs:25-33`
- เมื่อ `target_arch = "wasm32"` ตัว ort เองบังคับเส้นทาง "ยังไม่มี API" เหมือน alternative backend:
  `ort/src/lib.rs:202-212` → `ort::api()` panic ถ้าไม่ได้เรียก `ort::set_api` ก่อน
  (`set_api` อยู่ที่ `ort/src/lib.rs:262`; ข้อความ panic อยู่ที่ `ort/src/lib.rs:205-207`)
- signature ฝั่ง wasm32 เป็น **async** ไม่ใช่ FFI ปกติ:
  `CreateSession(model_path: &str)` / `CreateSessionFromArray(&[u8])` คืน `Pin<Box<dyn Future<Output = OrtStatusPtr>>>`
  → `ort-sys/src/lib.rs:965-989` และ `RunAsync` → `ort-sys/src/lib.rs:1562-1580`
  (ฝั่ง stub ที่มากับ crate ก็เป็นแบบเดียวกัน → `ort-sys/src/stub.rs:94-123`, `:1529-1539`; stub ตอบ `ORT_NOT_IMPLEMENTED`)
- session builder: `commit_from_url` / `commit_from_memory` มีเฉพาะ wasm32 และเป็น `async fn`;
  `commit_from_file` ไม่มีบน wasm → `ort/src/session/builder/impl_commit.rs:118-147` (wasm) เทียบ `:81-115` (native)
- execution provider เฉพาะ wasm: `WASM`, `WebNN` + `WebGPU` (wasm เปิด WebGPU ให้เสมอ)
  → `ort/src/ep/mod.rs:444-449` และ `:468-475`; `ort-sys/build/static_link/mod.rs:67`

**หลักฐานรันจริง (2026-09-23):** สร้าง crate เดี่ยว `/tmp/r2w-a2` ที่มี dependency เดียวคือสูตรข้างบน
(ไม่มี `ort-web` ก็ compile ผ่าน — `ort-web` จำเป็นตอน **runtime** เท่านั้น) แล้วรัน:

```bash
$ env | grep -i RUSTFLAGS     # ไม่มีอะไร (ไม่มีการตั้ง RUSTFLAGS เลย)
$ cargo check --target wasm32-unknown-unknown --offline
    Finished `dev` profile [unoptimized + debuginfo] target(s) in ...   # exit 0
```

⇒ ข้อ a2 = **ผ่านจริง** ไม่ใช่แค่ข้อความในเอกสาร upstream

### 1.4 runtime ฝั่ง JS ต้องเป็นอะไร

**ต้องเป็น `ort-web` (crate `ort-web = "0.3.0+1.27"`) ซึ่งเป็นสะพานไป ONNX Runtime ที่คอมไพล์ด้วย Emscripten
และโหลด asset จากแพ็กเกจ npm `onnxruntime-web`** — ไม่ใช่ ONNX Runtime native ที่ ort ดาวน์โหลด

กลไก: ONNX Runtime (C++ → Emscripten WASM) กับ Rust (wasm-bindgen WASM) อยู่คนละ WebAssembly context
จึง link กันตรง ๆ ไม่ได้; `ort-web` ทำหน้าที่ bridge + sync ข้อมูลข้าม context

การใช้งาน (จากเอกสารทางการ https://ort.pyke.io/backends/web):

```rust
ort::set_api(ort_web::api(FEATURE_WEBGPU).await?);   // ต้องเรียกก่อนใช้ ort API อื่นใด
let mut session = Session::builder()?
    .with_execution_providers([ep::WebGPU::default().build()])?
    .commit_from_url("./model.onnx").await?;         // async บนเว็บ
let outputs = session.run_async(ort::inputs![...]).await?;
```

ข้อเท็จจริงที่กระทบงานนี้โดยตรง:

| เรื่อง | native (ที่ repo ใช้อยู่) | wasm + ort-web |
|---|---|---|
| สร้าง session | `commit_from_file(path)` | ❌ ไม่มี → `commit_from_url` / `commit_from_memory` (async) |
| รัน | `session.run(inputs)` (sync) | ❌ **ไม่รองรับ sync** → `run_async(...)` + sync output |
| asset runtime | `libonnxruntime.so` จาก pyke CDN | JS + `.wasm` จาก `onnxruntime-web` dist (ค่าเริ่มต้นดึงจาก `cdn.pyke.io`; self-host ได้ผ่าน `Dist`) |
| EP | CPU/CUDA/… | WASM (CPU), WebGPU, WebNN (+WebGL ผ่าน FEATURE_WEBGL) |

ข้อสังเกต: เอกสาร upstream `ort/src/environment.rs:511-514` อ้างถึง `ort-web` ว่าเป็น alternative backend ที่เก็บ telemetry
โดยค่าเริ่มต้น (ส่งไป `signal.pyke.io`) และ `ort-web` ปิดได้ด้วย `EnvironmentBuilder::with_telemetry(false)`

**ผลต่อโค้ดเดิมของรีโป:** `native/crates/moe-train/src/render.rs` ใช้ `Session::builder().commit_from_file(...)` + `session.run(...)`
แบบ sync ทั้งหมด → **ถ้าจะรัน vocoder บน wasm ต้องเขียน path ใหม่เป็น async ort-web ไม่ใช่แค่เปลี่ยน feature flag**

---

## 2. (b) ทำให้ `moe-train` compile บน wasm32

### 2.1 สิ่งที่แก้

ไฟล์: `native/crates/moe-train/Cargo.toml`

```toml
[features]
default = ["wgpu", "train", "ort-vocoder"]
wgpu = ["burn/wgpu"]        # native
train = ["burn/train"]
ort-vocoder = ["dep:ort"]   # ONNX Runtime vocoder — native เท่านั้น

[dependencies]
ort = { version = "2.0.0-rc.13", features = ["ndarray"], optional = true }

# burn แยกตาม target: native คงเดิม / wasm32 ไม่มี wgpu-cubecl
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
burn = { version = "0.21", features = ["ndarray", "wgpu", "std", "train"] }
burn-ndarray = { version = "0.21", features = ["std"] }

[target.'cfg(target_arch = "wasm32")'.dependencies]
burn = { version = "0.21", default-features = false, features = ["ndarray", "std"] }
getrandom = { version = "0.4", features = ["wasm_js"] }

[[bin]]
name = "moe-train"
required-features = ["wgpu", "ort-vocoder"]   # CLI เป็น native-only
```

ไฟล์: `src/render.rs` — ใส่ `#[cfg(feature = "ort-vocoder")]` ให้ทุกฟังก์ชันที่แตะ `ort`
(`open_vocoder_threads`, `open_vocoder`, `run_vocoder`, `run_vocoder_once`, `render_phrase_wav`, `render_phrase_wav_with_session`)
ไฟล์: `src/baseline.rs` — test `f16_smoke` ผูกกับ `#[cfg(all(test, feature = "wgpu"))]`
ไฟล์: `src/lib.rs` — คอมเมนต์กำกับว่า wasm build ต้องใช้ `--no-default-features`

### 2.2 เกณฑ์ผ่านหลัก (คำสั่ง + ผลจริง)

```bash
cd native
cargo check -p moe-train --target wasm32-unknown-unknown --no-default-features
# → Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.16s
# → WASM_EXIT=0
```

log: `/tmp/r2w-1-wasm.log` · ตรวจแล้วว่าไม่มี `ort`, `ort-sys`, `burn-wgpu`, `wgpu` ใน graph ของคำสั่งนี้:

```bash
cargo tree -p moe-train --target wasm32-unknown-unknown --no-default-features -e normal --prefix none | grep -i "wgpu|cubecl|^ort "
# → ไม่มี wgpu / burn-wgpu / ort ; พบเพียง cubecl-common, cubecl-zspace (lib ย่อยที่ burn-tensor พึ่งโดยไม่ผูก GPU backend)
```

burn ที่เข้า graph บน wasm: `burn, burn-core, burn-autodiff, burn-backend, burn-ir, burn-ndarray, burn-nn, burn-optim, burn-std, burn-tensor` (ไม่มี `burn-wgpu`)

### 2.3 native ต้องไม่พัง

```bash
cargo check -p moe-train        # lib + bin, default features
# → Finished `dev` profile ... ; NATIVE_EXIT=0

cargo test -p moe-train
# running 5 tests
# test phoneme_groups::tests::five_groups ... ok
# test phoneme_groups::tests::falls_back_to_first_char ... ok
# test phoneme_groups::tests::maps_known_symbols ... ok
# test phoneme_groups::tests::strips_cvvc_markers ... ok
# test baseline::f16_smoke::f16_forward_is_finite ... ok
# test result: ok. 5 passed; 0 failed; 0 ignored; finished in 7.66s
# TEST_EXIT=0
```

log: `/tmp/r2w-4-test.log` · (ผลทดสอบเท่าเดิมก่อน/หลังแก้ — 5 ตัว, ตัว wgpu-f16 ยังรันผ่าน)

**ข้อควรรู้เรื่อง gate:** test `f16_smoke` ถูกผูกกับ `#[cfg(all(test, feature = "wgpu"))]`
⇒ ถ้ารัน `cargo test -p moe-train --no-default-features` test ตัวนั้นจะ **หายไป** (เหลือ 4 ตัว)
การนับ "5 passed" ข้างบนจึงต้องรันด้วย **default features** เท่านั้น

### 2.4 เกินโจทย์: พิสูจน์ว่า link เป็น `.wasm` จริงได้

เพิ่ม `examples/moe_wasm_smoke.rs` ที่ประกาศ `crate-type = ["cdylib"]` และ export `moe_forward_probe()`
(สร้าง MoE acoustic เล็ก ๆ → forward 1 ครั้ง → คืน RMS ของ mel 128 bins)

```bash
cargo build -p moe-train --example moe_wasm_smoke \
  --target wasm32-unknown-unknown --no-default-features
# → Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.31s ; WASMBUILD_EXIT=0
# target/wasm32-unknown-unknown/debug/examples/moe_wasm_smoke.wasm = 16,395,838 bytes
# sha256 76398cb32068710d897399252e77cde77bc5722b8bf4e7f662fc39e856bd0b8a
```

ตรวจโครงสร้างไฟล์ wasm ด้วย parser ของตัวเอง (`/tmp/r2w-wasm-info.py`):

| รายการ | ค่า |
|---|---|
| wasm version | 1 |
| memory (min) | 19 หน้า (~1.2 MB) — ไม่กำหนด max |
| imports | 6: `__wbindgen_placeholder__` x4 (รวม `__wbg_getRandomValues_*`, `__wbindgen_throw`), `__wbindgen_externref_xform__` x2 |
| exports | 1383 (รวม `moe_forward_probe`) |

⇒ burn-ndarray + โค้ด MoE acoustic **codegen + link เป็น wasm module ได้จริง** บนเครื่องนี้

#### หลักฐานอิสระจากผู้ตรวจ (poc-verifier คนละคนกับผู้เขียน T2)

ผู้ตรวจจับ glue เองด้วย `wasm-bindgen --target nodejs` แล้วรัน `moe_forward_probe()` ใน **node**
(คนละ host กับที่ผู้เขียนใช้) ได้ค่าจริง (ไม่ใช่ 0/ค่าคงที่) และ **ต่างกันคนละ process**:

| process | `moe_forward_probe()` (RMS ของ mel 128 bins) |
|---|---|
| A | `1.2979992628097534` |
| A (ซ้ำ) | `1.4312840700149536` |
| B | `1.086662769317627` |
| B (ซ้ำ) | `1.1687109470367432` |

⇒ ปิดช่องว่าง "ยังไม่เคยรัน wasm" ของ T2 ด้วยหลักฐานจากคนละคน (ค่าต่างกันเพราะน้ำหนักถูก init แบบสุ่ม;
ที่สำคัญคือ finite, ไม่ใช่ศูนย์, และไม่ใช่ค่าคงที่)

### 2.5 เรื่อง getrandom 0.4 — แก้ความเข้าใจในโจทย์

โจทย์ระบุว่าต้องมี `RUSTFLAGS='--cfg getrandom_backend="wasm_js"'` ด้วย — **จากหลักฐาน ไม่ต้องใช้**

- `getrandom-0.4.3/Cargo.toml:158` ประกาศ `check-cfg` ของ `getrandom_backend` ไว้แค่
  `values("custom","efi_rng","rdrand","rndr","linux_getrandom","linux_raw","windows_legacy","unsupported","extern_impl")`
  → `"wasm_js"` **ไม่ใช่** ค่าที่ backend รู้จัก
- `getrandom-0.4.3/src/backends.rs:171-184` เลือก backend wasm จาก **crate feature** ตรง ๆ:
  `if #[cfg(feature = "wasm_js")] { mod wasm_js; } else { compile_error!(...) }`
- หลักฐานรันจริง: ทุกคำสั่งใน §2.2/§2.4 รันโดย**ไม่ได้ตั้ง RUSTFLAGS เลย** และผ่านทั้งหมด
  → สิ่งที่จำเป็นจริงคือ `getrandom = { version = "0.4", features = ["wasm_js"] }` บน target wasm32 เท่านั้น
  (ในรีโปนี้ `rand 0.10.2` ซึ่งมาจาก `burn-ndarray` เป็นตัวดึง `getrandom 0.4.3`)
- ตรวจแล้วว่าไม่มี `.cargo/config.toml` ที่ซ่อน rustflags: `native/.cargo/config.toml` มีแค่ `[alias]` 2 ตัว
- **ผู้ตรวจยืนยันซ้ำอิสระ** (poc-verifier): `Cargo.toml:158` ของ getrandom 0.4.3 ไม่มี `wasm_js` ใน `check-cfg` values,
  `backends.rs:171-184` เลือก backend จาก crate feature, และไม่มี rustflags ใน config หรือ env ใด ๆ — ตรงกับข้อสรุปข้างบน

`wasm-bindgen`/`js-sys` ของ getrandom ถูก target-gate ไว้ที่ wasm อยู่แล้ว (`getrandom-0.4.3/Cargo.toml:90-92, 98-100`)
จึงใส่ `getrandom` เฉพาะตาราง wasm32 — native ไม่ได้ dependency เพิ่ม

### 2.6 ข้อจำกัดของ (b) — อ่านก่อนอ้างว่า "พร้อม"

1. `cargo check`/`cargo build` พิสูจน์แค่ **compile + link** — ตอนส่งงาน T2 ยังไม่มีการรัน wasm
   (ภายหลังผู้ตรวจอิสระรัน `moe_forward_probe()` ใน node สำเร็จ — ดู §2.4 — จึงปิดช่องนี้ **เฉพาะ probe**;
   การโหลด checkpoint จริง + วัดเวลา/RAM อยู่นอกขอบเขต T2)
2. `.wasm` ที่ได้ **instantiate ตรง ๆ ไม่ได้** เพราะ import `__wbg_getRandomValues_*` จาก `__wbindgen_placeholder__`
   → ต้องมี JS glue จาก `wasm-bindgen` (ในขอบเขต T2 ไม่ได้ทำ; ผู้ตรวจสร้าง glue ฝั่ง node เองเพื่อทดสอบ)
3. เป็น **debug build 16.4 MB** — ยังไม่ได้ทำ release/opt (ขนาดจริงจะลดลงมาก)
4. `--no-default-features` ตัด bin `moe-train` ออก (ด้วย `required-features`) → คำสั่งนี้ตรวจ **lib เท่านั้น**
5. ยังไม่ได้สร้าง `native/crates/moe-wasm/` — การเพิ่ม workspace member ต้องแก้ `native/Cargo.toml` ซึ่งอยู่นอก write scope
   (ถ้า Lead ต้องการ crate wasm จริง ๆ ต้องอนุมัติแก้ `native/Cargo.toml`)
6. wasm32 ยังไม่มีเส้นทาง vocoder: `ort` ถูกตัดออก และโค้ด vocoder เดิมเป็น sync (ดู §1.4)

---

## 3. (c) ตรวจ `native/test-data/pc-nsf-hifigan.mpk`

ไฟล์: 6,177,944 bytes · sha256 `94c9caac24e413bbc53ab357555f6a889ecaf18dbbb8d9cd643f7720db82298f`

เครื่องมือ: `native/crates/moe-train/examples/inspect_mpk.rs` (`cargo run -p moe-train --example inspect_mpk -- test-data/pc-nsf-hifigan.mpk`)
และ cross-check ด้วย msgpack decoder ของตัวเอง (`/tmp/r2w-mpk-decode.py`, อ่านได้ครบทั้ง 6,177,944 ไบต์)

### 3.1 มันคือไฟล์อะไร

เป็น **burn NamedMpk record จริง** — เปิดด้วย `NamedMpkFileRecorder::<FullPrecisionSettings>` ได้

metadata ในไฟล์:

| key | ค่า |
|---|---|
| float | `f32` |
| int | `i32` |
| format | `burn_core::record::file::NamedMpkFileRecorder<burn_core::record::settings::FullPrecisionSettings>` |
| version | `0.21.0` |
| settings | `FullPrecisionSettings` |

โครงสร้าง top-level: `{ metadata, item, _b }` โดย `item` มี field: `emb`, `fc_in`, `hiddens` (array 1 ตัว), `out`, `dropout = null`

### 3.2 key จริงทั้งหมด (7 tensors)

| key | shape | จำนวน | min | max | mean | rms |
|---|---|---|---|---|---|---|
| `emb.weight` | `[512, 64]` | 32,768 | −0.90192 | 0.86039 | −0.000090 | 0.086390 |
| `fc_in.weight` | `[322, 1024]` | 329,728 | −7.10991 | 0.92826 | −0.000151 | 0.046462 |
| `fc_in.bias` | `[1024]` | 1,024 | −0.26853 | 0.35085 | −0.015475 | 0.088603 |
| `hiddens[0].weight` | `[1024, 1024]` | 1,048,576 | −0.78331 | 1.02042 | −0.001292 | 0.024555 |
| `hiddens[0].bias` | `[1024]` | 1,024 | −0.67614 | 0.43542 | 0.153841 | 0.194891 |
| `out.weight` | `[1024, 128]` | 131,072 | −0.40822 | 0.40404 | −0.027862 | 0.065281 |
| `out.bias` | `[128]` | 128 | −0.81734 | 0.12647 | −0.370355 | 0.417891 |

dtype ของทุกตัว = `F32` · รวม **1,544,320 พารามิเตอร์** = 6,177,280 ไบต์ (ไฟล์ 6,177,944 ไบต์ = payload + msgpack overhead 664 ไบต์)
ค่าทั้งหมดเป็นค่าจริงที่กระจายตัว (ไม่ใช่ศูนย์/ค่าคงที่) → เป็น checkpoint ที่เทรนแล้ว ไม่ใช่ไฟล์เปล่า

### 3.3 ตอบคำถาม: เอามาทำ burn vocoder บน wasm ได้ไหม → **ไม่ได้**

เหตุผลจากหลักฐาน:

1. โครงสร้าง tensor เป็น `Embedding` + `Linear` ล้วน — **ไม่มีเลเยอร์ของ HiFiGAN/NSF generator เลย**
   (ไม่มี conv/upsample/resblock/`noise_convs`/`source` weight ที่ vocoder ต้องมี)
2. `out.weight` = `[1024, 128]` → **128 = จำนวน mel bin (เอาต์พุตของ acoustic) ไม่ใช่ waveform**
3. `fc_in.weight` = `[322, 1024]` และ `322 = 5*64 + 2` → ตรงกับ `ContextAcoustic` ที่ `WINDOW=5` (±2)
   embedding 512 ช่อง × 64 มิติ = ค่าคงที่รุ่นเก่าของ `native/crates/moe-train/src/baseline.rs`
4. ชื่อ field ทั้ง 5 (`emb`, `fc_in`, `hiddens`, `out`, `dropout`) ตรงกับ struct `ContextAcoustic`
   ที่ `native/crates/moe-train/src/baseline.rs:43-50` เป๊ะ
5. ยืนยันด้วยการโหลดเข้า layout **ปัจจุบัน** (`WINDOW=9`, `EMBED_SLOTS=4096`, hidden 1024, depth 1):
   `load_file` คืน **OK** (เพราะ burn 0.21 ไม่ตรวจ shape ตอน load — `Tensor::from_item` สร้าง tensor จาก shape ในไฟล์)
   แต่พอ **forward จริง** กลับ panic:

```text
thread main panicked at burn-ndarray-0.21.0/src/ops/matmul.rs:125:9:
  Dimensions are incompatible for matrix multiplication
current forward     : PANIC (shape mismatch - the file is NOT the current 9x architecture)
```

⇒ ไฟล์นี้คือ **`ContextAcoustic` รุ่นเก่า (acoustic mel predictor)** ไม่ใช่ `pc-nsf-hifigan`
ชื่อไฟล์ทำให้เข้าใจผิด · `grep -rn pc-nsf-hifigan` ในรีโปเจอแค่ใน docs และคอมเมนต์ — ไม่มีโค้ดไหนโหลด `.mpk` นี้
(ตัว vocoder จริงในเส้นทาง r2/moe คือ ONNX `pc-nsf-hifigan.onnx` ~56.6 MB ที่โหลดผ่าน ort)

### 3.4 สิ่งที่ไฟล์นี้ทำได้จริง (ข้อจำกัด)

- ใช้เป็น **checkpoint acoustic ของ MoE บน wasm** ได้ในหลักการ: 6.2 MB (f32) · MLP ล้วน · ไม่ต้องใช้ GPU
  และ burn-ndarray link เป็น `.wasm` ผ่านแล้ว (§2.4)
- แต่ **ยังไม่พิสูจน์**: ยังไม่ได้สร้างเลเยอร์ legacy ให้ตรงแล้วรัน forward บน wasm จริง
  (ต้องมี embed 512×64, fc_in 322→1024 รับ context 5 เฟรม, hidden 1024×1, out 1024→128)
- และการจะได้ **เสียง** ต้องมี vocoder — ซึ่งคือ ONNX ที่ต้องพึ่ง ort-web (async) ตาม §1.4 ไม่ใช่ `.mpk` นี้

---

## 4. ไฟล์ที่แตะในงานนี้

| ไฟล์ | สถานะ |
|---|---|
| `native/crates/moe-train/Cargo.toml` | feature gating + burn แยกตาม target + getrandom wasm_js |
| `native/crates/moe-train/src/render.rs` | `#[cfg(feature = "ort-vocoder")]` 6 ฟังก์ชัน |
| `native/crates/moe-train/src/baseline.rs` | test `f16_smoke` ผูกกับ feature `wgpu` |
| `native/crates/moe-train/src/lib.rs` | คอมเมนต์ feature gating |
| `native/crates/moe-train/examples/inspect_mpk.rs` | เครื่องมือตรวจ `.mpk` (ใหม่) |
| `native/crates/moe-train/examples/moe_wasm_smoke.rs` | wasm cdylib probe (ใหม่) |
| `docs/planning/wasm-r2-poc.md` | เอกสารนี้ |
| `native/Cargo.lock` | **ผลข้างเคียงจาก cargo** — เพิ่ม 1 บรรทัด: `"getrandom 0.4.3"` เข้า dependency list ของ package `moe-train`
(เพราะเพิ่ม `getrandom` เข้า wasm32 dep table; ไม่มีเวอร์ชันอื่นถูกเปลี่ยน) |

ไม่ได้แตะ: `web/**`, `native/crates/engine-wasm/**`, `native/tools/**`, `native/Cargo.toml`

## 5. ยังไม่ผ่าน / งานต่อไป (ถ้าจะทำ neural บน wasm จริง)

1. ~~**ยังไม่รัน wasm**~~ — **ปิดแล้วบางส่วน:** ผู้ตรวจอิสระรัน `moe_forward_probe()` จริงบน wasm (node +
   `wasm-bindgen --target nodejs`, ดู §2.4) ⇒ ตัวโค้ดรันบน wasm ได้จริง ไม่ใช่แค่ compile
   ที่ยังเหลือ: โหลด checkpoint จริง + วัดเวลา/RAM (นอกขอบเขต T2) และ ort-web ต้อง vendor asset + `Dist`
2. **vocoder ต้อง port เป็น async ort-web** (`run_async` + `sync`) — งานเดิมใช้ sync ทั้งหมด จึงใช้ซ้ำตรง ๆ ไม่ได้
3. **ไม่มี `.mpk` ที่เป็น vocoder** ในรีโปนี้ — ถ้าจะทำ burn vocoder ต้องเทรน/convert สถาปัตยกรรม HiFiGAN-NSF เป็น burn ก่อน
4. RAM/เวลา: ยังไม่มีตัวเลขวัดจริงบน wasm ในเอกสารนี้ (ต้องวัดหลังมี host จริง) — อย่าเพิ่งสรุปว่าใช้ได้บนแท็บเล็ต/มือถือ