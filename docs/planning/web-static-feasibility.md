# Web Static Feasibility — ย้ายทั้งระบบเป็น static web + local storage + burn/WebGPU

**สถานะ:** Feasibility study — 2026-08-10
**คำถาม:** เปลี่ยนทุกอย่างที่ทำไว้ (Android voice synth) เป็น web-based ที่ใช้ static web เป็นหลัก ใช้ local web storage เก็บ voicebank/model และ train/render ด้วย Rust burn บน WebGPU ได้แค่ไหน?

> เอกสารนี้เป็น baseline ก่อนมี `worldline-rs`/`engine-wasm`. สมมติฐานเดิมที่ว่า
> worldline ต้องใช้ Emscripten ถูก supersede แล้ว; ดู [WASM Migration Status](wasm-migration-status.md)
> และ [WASM Target](../architecture/wasm-target.md) สำหรับสถานะ implementation ปัจจุบัน.

---

## Verdict

| เสา | ความเป็นไปได้ | ระดับ |
|---|---|---|
| WebGPU browser support | ✅ ทุกเบราว์เซอร์หลักเปิด default แล้ว (desktop + mobile) | **พร้อม** |
| burn บน WASM (inference) | ✅ Official — Burn Book มี chapter "WebAssembly" เฉพาะ (Flex + WebGpu backends) | **พร้อม** |
| ONNX → burn (ALL-BURN door) | ✅ `burn-import`/`burn-onnx` แปลง .onnx → native Rust module ตอน build (ModelGen) | **พร้อม (ต้อง verify op coverage ของ vocoder)** |
| Local storage ใหญ่ (voicebank 93MB) | ✅ OPFS (Origin Private File System) — quota 60% ของ disk, ขอ persistent ได้ | **พร้อม** |
| ort บน web | ✅ `ort-web` backend มีอยู่ (Emscripten ORT + wasm-bindgen bridge) — fallback ถ้า burn-import ไม่ครอบคลุม | fallback |
| **worldline C++ (WORLD kernel) → wasm** | ✅ ไม่ต้องทำในเส้นทางปัจจุบัน — `worldline-rs`/`world-rs` pure Rust ใช้แทนได้ | **ปิดความเสี่ยงหลัก** |
| **train บน WebGPU ใน browser** | 🔶 burn autodiff เป็น backend-generic + cubecl รองรับ WebGPU — แต่**ไม่มี official browser-training example** ต้อง spike | **ต้องพิสูจน์** |
| Plugin .so / dlopen | ❌ ไม่มี dlopen บน web → ต้อง static-link ทุก plugin เป็น wasm เดียว (build-time feature flags) หรือ dynamic wasm loading | เปลี่ยนสถาปัตยกรรม |

**สรุปสถานะปัจจุบัน:** engine WASM และ browser editor vertical slice ทำแล้ว.
งานหลักที่เหลือคือ (1) ต่อ editor กับ RenderPool/OPFS/multi-track/PWA, (2) ย้าย
neural engines ถ้าต้องการ, และ (3) spike training บน WebGPU. Emscripten สำหรับ
C++ เป็นเพียงทางเลือก legacy ไม่ใช่ dependency ของ classic/browser path แล้ว.

---

## 1. WebGPU browser support — ✅ พร้อมจริง

- Chrome/Edge: เม.ย. 2023, **Safari 26: มิ.ย. 2025, Firefox 141: ก.ค. 2025** — ทุกตัวเปิด default ตั้งแต่ปลาย 2025
- **Mobile: iOS 26 Safari + Chrome Android มี WebGPU** → เงื่อนไข "SD870 min" ของเดิมกลายเป็น "browser ไหนก็ได้ที่อัปเดต"
- WebGPU ต้องการ secure context (https/localhost) — static host (GitHub Pages/Cloudflare Pages) เป็น https อยู่แล้ว ✅

## 2. burn บน WASM — ✅ official

- Burn Book "WebAssembly" chapter: *"Burn supports WebAssembly (WASM) execution using the **Flex** and **WebGpu** backends, allowing models to run directly in the browser"*
- Official examples: `mnist-inference-web`, `image-classification-web` (import ONNX ตอน build → wasm-bindgen → inference ใน browser), `remote-inference-web`
- Known config: **getrandom ต้องตั้งค่า explicit เมื่อใช้ WebGpu**
- burn-wgpu backend รองรับ "Vulkan, Metal, DX11/12, OpenGL, **WebGPU**"
- ⚠️ moe-train ใช้ burn 0.17 — ต้อง verify ว่า 0.17 + wgpu + wasm32 compile ผ่าน (spike item 1)

## 3. ONNX door: burn-import — ✅ ALL-BURN ทางนี้

- `tracel-ai/burn-onnx` (แยก repo พ.ค. 2026): `burn_import::onnx::ModelGen` — แปลง .onnx → Rust source + weights ตอน **build time** → backend-agnostic → รันบน WebGPU ได้
- ใช้ได้กับของที่มีอยู่: `mel.onnx` (85KB) + `pc_nsf_hifigan.onnx` (56MB) → กลายเป็น burn module → **ไม่ต้องพึ่ง ort เลยบน web** (ตรงวิสัยทัศน์ ALL-BURN)
- ⚠️ burn-import รองรับ ONNX op เป็น subset — hifigan เป็น conv stack น่าจะผ่าน แต่ **ต้อง verify op coverage** (spike item 2)
- Fallback: `ort-web` (pykeio/ort alternative backend) — ORT compile ด้วย Emscripten, bridge กับ Rust ผ่าน wasm-bindgen, สอง WASM context แยกกัน (ต้อง sync data เอง) — ใช้ได้แต่ไม่ "ALL-BURN"

## 4. Local web storage — ✅ OPFS

- **OPFS** (Origin Private File System): ไฟล์ใหญ่ๆ เก็บได้จริง (voicebank 93MB + models 55MB ≈ 150MB ไม่มีปัญหา)
  - Chrome: quota ~60% ของ disk, ขอ `navigator.storage.persist()` ได้ (PWA ติดตั้งแล้วได้อัตโนมัติ)
  - Safari 15.2+/Firefox/Chrome รองรับ OPFS หมด
- **File System Access API** (`showDirectoryPicker`): **Safari ไม่รองรับ** → import voicebank บน Safari ต้อง fallback เป็น zip upload (มี JS zip lib หรือ Rust `zip` crate ใน wasm)
- IndexedDB ใช้เก็บ metadata/checkpoint; OPFS เก็บ wav/model/frq
- ล้าง site data = ลบ OPFS (เหมือน uninstall app) — ต้องมี export/backup UI

## 5. worldline C++ → wasm — superseded by pure Rust

- worldline = vendored C++ (WORLD, libpyin, libgvps, spline, libnpy) ปกติ build ผ่าน CMake → `.so`
- บน web ไม่มี .so/dlopen แต่แก้ด้วย `world-rs` + `worldline-rs` pure Rust แล้ว
  (`engine-wasm` ใช้เส้นทางนี้และเทียบ native ได้)
- Emscripten C++ ยังทำได้เป็น legacy/compatibility option หากต้องการตรงกับ C++
  binary โดยตรง แต่ไม่ใช่ dependency ของ browser editor ปัจจุบัน

## 6. Plugin architecture (dlopen mixerfx) — ❌ ต้อง redesign

- กฎเดิม: "logic ทั้งหมด = Rust .so engine plugin (dlopen)" — **dlopen ไม่มีบน web**
- ทางที่ง่ายสุด: **static-link ทุก plugin เป็น wasm ตัวเดียว** — เลือก plugin ตอน build ผ่าน feature flags (`--features r1,r2,mixerfx`) — `Engine trait + Box<dyn Engine>` ยังอยู่ครบที่ระดับ Rust
- ทาง dynamic: instantiate หลาย wasm module (ESM-integration) — ซับซ้อน ไม่คุ้มตอนนี้

## 7. Training บน WebGPU ใน browser — 🔶 ต้อง spike

- cubecl (ตัว engine ข้างใต้ burn-wgpu) target รวม **WebGPU** ไว้ ("CUDA, ROCm, WebGPU through different compiler frontends") + autodiff เป็น generic → โครงสร้างรองรับ
- แต่ official examples ของ burn เป็น **inference** บน web เป็นหลัก (MNIST inference web) — ยังไม่มี browser training example ทางการ
- ขนาดที่ต้อง train (MoE adapter ~1M params, 15 epochs) = เล็กพอสำหรับ WebGPU ใน browser แน่นอน (desktop iGPU 43× CPU; WebGPU บนเครื่องเดียวกันน่าจะใกล้เคียง wgpu native แต่มี overhead)
- ข้อจำกัด:
  - wasm32 memory สูงสุด ~4GB (browser มัก ~2-4GB); WebGPU buffer limit ต่างกันต่อ device (mobile เล็กกว่า)
  - **ไม่มี threads บน GH Pages**: wasm threads ต้อง SharedArrayBuffer → ต้อง COOP/COEP headers → **GitHub Pages ส่ง headers เหล่านั้นไม่ได้** (Cloudflare Pages ส่งได้ผ่าน `_headers`) — แต่ WebGPU path ไม่ต้องพึ่ง CPU threads (GPU ทำงานเอง) → ไม่เป็น blocker
  - f32 storage ใน WebGPU มี limit (f16 เป็น optional) — burn จัดการได้ แต่ต้องเช็ค

## 8. Static hosting — ✅

- ทุกอย่างเป็นไฟล์ static: `index.html` + `engine.wasm` + models + JS — ใช้ GitHub Pages / Cloudflare Pages / เปิด `file://` (มีข้อจำกัด WebGPU ต้อง secure context)
- Service Worker → offline/PWA ได้
- **ของที่หายไปจากสถาปัตยกรรมเดิม (ดีด้วย):** synth-server (port 18080), trainer-gui (axum), EngineProcess, JNI/Kotlin MethodChannel — ทั้งหมดกลายเป็น wasm + JS bridge

---

## Architecture mapping (ของเดิม → web)

| ของเดิม | บน static web |
|---|---|
| Flutter app (`app/`) | static HTML/JS UI — **มี ui-mock/index.html + index-mixer.html + index-om.html อยู่แล้ว** (trace จาก OM source + Flutter เดิม) |
| Rust crates: domain/feed/phrase/ustx-codec/voicebank/phonemizer/storage/mixer-fx | compile เป็น `wasm32-unknown-unknown` + wasm-bindgen; **IO ผ่าน injectable storage adapter** (OPFS) — pattern เดียวกับ vanilla-offline-web-app skill |
| worldline-plugin (.so WORLD C++) | Emscripten → wasm (phase หลัง) หรือข้ามไป neural-first |
| diffsinger / r2 (ort + onnx) | `burn-import` ตอน build → burn native → WebGPU (ALL-BURN) / fallback `ort-web` |
| moe-train (burn wgpu) | burn WebGPU ใน browser — data จาก OPFS/IndexedDB, checkpoint ทุก epoch ลง IndexedDB (กัน tab ปิด) |
| voicebank (oto/wav/frq) | OPFS (import: directory picker บน Chrome/FF, zip บน Safari) |
| phonemizer | pure Rust → wasm ✅ |
| JNI/Kotlin platform plugin | wasm-bindgen JS bridge (บทบาท "platform plugin" = JS adapter) |
| synth-server/trainer-gui/EngineProcess | ❌ ไม่ต้องมี — ทุกอย่างใน-process |

## สิ่งที่ได้ vs เสีย

**ได้:** ไม่ต้อง build APK/deploy device, ใช้ได้ทุกแพลตฟอร์ม (รวม iOS!), อัปเดต = push static file, ไม่มี server cost, เปิด URL ก็เล่น, golden test ยังรันเป็น native ได้เหมือนเดิม
**เสีย:** ไม่มี dlopen (plugin ต้อง static), ไม่มี background processing จริง, Safari ไม่มี directory picker, ไม่มี threads บน GH Pages (ต้อง Cloudflare ถ้าจะใช้), WebGPU ยังไม่ถึงประสิทธิภาพ native Vulkan

---

## ขั้นตอนแนะนำ (POC-first)

1. **Spike A — burn 0.17 → wasm32 + WebGPU inference**: compile moe-train (หรือ crate เล็กๆ) เป็น wasm, รัน `mel.onnx`→`hifigan.onnx` ผ่าน burn-import (หรืออย่างน้อย burn module เล็กๆ) ใน browser — พิสูจน์ toolchain + op coverage
2. **Spike B — train ใน browser**: Autodiff<Wgpu> บน WebGPU 1 รอบเล็ก (ไม่ต้องทั้ง MoE) — เช็คว่า backward pass + optimizer ทำงานบน WebGPU backend จริง
3. **Spike C — OPFS voicebank**: import Teto bank (93MB) ลง OPFS, อ่าน oto.ini/wav ผ่าน wasm-bindgen storage adapter
4. ผ่านครบ 3 → port UI จาก ui-mock + ต่อ wasm engine → static deploy (Cloudflare Pages ไว้ก่อน เผื่อ threads)
5. Emscripten worldline C++ เป็น optional legacy phase เท่านั้น; classic browser path
   ใช้ pure-Rust port ที่ผ่าน parity แล้ว

## Risks / ต้องเช็คเพิ่ม

- [ ] burn 0.17 + wgpu compile `wasm32-unknown-unknown` จริง (getrandom/js feature)
- [ ] burn-import ครอบคลุม op ของ mel.onnx + pc_nsf_hifigan.onnx ไหม
- [ ] Autodiff บน WebGPU backend ทำงานครบ (backward + Adam) — ไม่มี official example
- [ ] Emscripten build worldline (ถ้าทำ classic) — alloc/thread/FFI
- [ ] WebGPU buffer limits บนมือถือ (iOS 26 / Android Chrome) พอสำหรับ mel/hifigan batch ไหม
