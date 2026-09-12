# WASM Migration — สถานะการย้ายไป web

> อัปเดต: 2026-09-12
> ต่อจาก: `docs/planning/web-static-feasibility.md` (feasibility study 2026-08-10)

---

## คำตอบสั้น

**engine ย้ายได้แล้ว** — ทั้ง stack compile เป็น `wasm32-unknown-unknown`
และ render ผลตรงกับ native **ทุก bit** (`max|diff| = 0.000e0`)

**UI เริ่มย้ายแล้ว:** มี browser editor vertical slice ที่แก้โน้ต, undo/redo,
render/play และทดสอบ headless ผ่าน. อย่างไรก็ตามยังไม่ใช่ product flow เต็มตัว —
หน้า editor ยัง render บน main thread และยังไม่ต่อ OPFS/project browser,
multi-track mixer, expression lanes หรือ PWA. Neural engines (ort/burn) ยัง native-only.

---

## เทียบกับ feasibility doc

| เสา | เอกสารเดิมประเมิน | สถานะจริง |
|---|---|---|
| worldline C++ → wasm | ⚠️ "จุดเสี่ยงหลัก ต้อง Emscripten" | ✅ **ไม่ต้อง Emscripten แล้ว** — port เป็น pure Rust เสร็จ (`worldline-rs`) |
| F0 fallback (libpyin) | (ไม่ได้ประเมิน) | ✅ port libpyin + libgvps เสร็จ — ตรงกับ `.so` 100% |
| Rust engine → wasm | ✅ | ✅ ทำแล้ว (`engine-wasm`) |
| Local storage (OPFS) | ✅ | 🟡 มี `OpfsBank` adapter แล้ว แต่ editor demo ยังใช้ HTTP fixture |
| Plugin .so / dlopen | ❌ ต้อง redesign | ✅ feature-gate แล้ว (`ffi` feature) |
| burn บน WASM | ✅ | ⬜ ยังไม่ทำ (neural) |
| ONNX → burn | ✅ | ⬜ ยังไม่ทำ (neural) |
| train บน WebGPU | 🔶 ต้อง spike | ⬜ ยังไม่ทำ |
| Browser editor | (ยังไม่ได้ประเมิน) | 🟡 Vertical slice ผ่าน headless test; product wiring ยังเหลือ |
| Static hosting | ✅ | ⬜ ยังไม่มี PWA/deploy config |

**การค้นพบสำคัญ:** feasibility doc คิดว่า worldline ต้อง compile C++ ด้วย
Emscripten — แต่จริงๆ **port เป็น Rust ตรงกว่าและได้ผลตรงกว่า** เพราะตอนนี้
Rust engine ให้ผลเหมือน `.so` ระดับ sample อยู่แล้ว ไม่ต้องพึ่ง Emscripten
runtime เลย

---

## สถาปัตยกรรม

```
เดิม (native)                      ใหม่ (web)
─────────────────────────────     ─────────────────────────────
APK + Kotlin + JNI                static HTML/JS + wasm-bindgen
synth-server (port 18080)         ไม่ต้องมี (in-process)
libworldline.so (dlopen)          worldline-rs (wasm, ไม่มี .so)
libmixerfx.so (dlopen)            mixer-fx::MixerChain (pure Rust)
Flutter app                       static HTML + Canvas editor (กำลัง port)
voicebank ใน filesDir             HttpBank fixture / OpfsBank adapter
LAN HTTP client                   wasm-bindgen (in-process)
```

crate ที่เพิ่ม:

| crate | หน้าที่ |
|---|---|
| `world-rs` | WORLD DSP kernel (มีอยู่แล้ว) |
| `worldline-rs` | PhraseSynth + **Classic resampler** (port ใหม่) |
| `engine-wasm` | wasm-bindgen entry point (ใหม่) |
| `tools/world-oracle` | เครื่องมือเทียบ stage-by-stage (ใหม่) |

---

## หลักฐานความถูกต้อง

| test | ผล |
|---|---|
| `engine-wasm` in-memory vs disk render | 132066 samples, **max\|diff\| = 0.000e0** |
| `world-oracle phrase` demo-song | 110081 = 110081, rms diff 9.0e-6 |
| `world-oracle phrase` scale-demo | 639489 = 639489, rms diff 3.5e-5 |
| `world-oracle classic` | worst max\|diff\| 9.11e-5 |
| `world-oracle pyin` | voicing agreement 100%, mean/max cents 0.00 |
| headless Chromium (`web/run-smoke.sh`) | 132066 samples, peak 0.9884 = native |
| `rust_kernel_matches_so_*` | PASS |
| `rust_classic_resampler_matches_so` | PASS |
| `cargo test -p engine-wasm` (2026-09-12) | **11/11 PASS** — memory render + project commands |
| `web/run-pool.sh` (2026-09-12) | pool/sequential + incremental **bit-identical**, unchanged render 0 chunks |
| `web/editor-test.html` (2026-09-12) | **PASS** — boot/canvas/edit/undo/draw/marquee/render/zoom |

ขนาด wasm (release): **1.43 MB** (508 KB gzip)

---

## งานที่เหลือ (เรียงตามความสำคัญ)

1. **ต่อ editor เข้ากับ RenderPool** — `web/pool.js` ผ่าน bit-identical และ
   incremental test แล้ว แต่ `web/app/editor.js` ยังเรียก `driver.renderAll()` บน
   main thread; ต้อง wire progress/cancel/epoch และ cache state เข้าหน้า editor
2. **Project + voicebank persistence** — ต่อ `OpfsBank`, import, project browser,
   autosave และ export `.ustx` เข้ากับ editor (ตอนนี้ใช้ fixture ผ่าน HTTP)
3. **UI production breadth** — multi-track mix, expression lanes, phoneme labels
   authoritative, project/voicebank flows และ tablet/pen hardware QA
4. **Neural engines** — r2 / diffsinger / moe ยัง native-only ต้องเลือกว่าจะ
   ใช้ `burn-import` (ONNX → burn → WebGPU) หรือ `ort-web`
5. **Static hosting + service worker** — PWA/offline/versioning และ deploy จริง

---

## วิธีใช้

```sh
# engine wasm (release)
cd native
cargo build --release --target wasm32-unknown-unknown -p engine-wasm
wasm-bindgen --target web --out-dir web-pkg \
  target/wasm32-unknown-unknown/release/engine_wasm.wasm

# ทดสอบ parity กับ native (ไม่ต้อง browser)
cargo test -p engine-wasm
cargo test -p synth-cli --test render_real -- --ignored   # ต้องมี .so
```
