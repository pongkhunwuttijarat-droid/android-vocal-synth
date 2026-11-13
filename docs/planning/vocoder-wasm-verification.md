# Vocoder WASM / R2-MoE — รายงานตรวจสอบอิสระ (T3)

> สถานะ: **ตรวจของจริงแล้ว T1 · T2 · T4 + วัด RAM baseline ปิดช่องว่าง** · ผู้ตรวจ: teammate `poc-verifier` (แยกจากผู้เขียน T1/T2/T4)
> วันที่: 2026-09-23 · task: `task-3` · write scope: ไฟล์นี้ไฟล์เดียว
> หลักการ: พยายาม**หักล้าง** — ข้อความในเอกสารไม่ถือเป็นหลักฐาน ต้องมีคำสั่ง+ผลรันจริง หรือ artefact ที่ตรวจเอง

## 0. สรุป verdict

| งาน | verdict | หมายเหตุสั้น |
|---|---|---|
| **T1** POC vocoder บน browser | **ผ่าน** (ข้อควรแก้ในเอกสาร 6 ข้อ → §1.5) | รันซ้ำเอง 2 รอบ PASS ทั้งคู่ · WAV byte-identical ตรงกับที่อ้าง · metadata โมเดลตรงทุกข้อ · ไม่พบ hardcode/mock |
| **T2** R2/MoE wasm readiness | **ผ่าน** (ข้อควรแก้ 3 ข้อ → §2.6) | artefact ตรงทั้ง size/hash/imports · `cargo test` 5/5 · wasm build จาก target dir ใหม่ได้ **hash เดิมเป๊ะ** · **ผู้ตรวจรัน wasm จริงสำเร็จ** (เกินขอบเขตที่ T2 อ้าง) |
| **T4** MoE/burn รัน wasm จริง | **ผ่าน** (ข้อควรแก้ 8 ข้อ → §6.5) | `./web/run-moe.sh` PASS 2/2 รอบ · memory 1,114,112→26,411,008→34,930,688 **ตรงเป๊ะ** · determinism 0 · ค่า wasm/native ตรงถึง 1e-8 · **แต่เวลาไม่ reproducible (±40–80%)** |
| **RAM** ทบทวนเลข T1/T4 | **ต้องแก้การอ้าง** → §7 | baseline `about:blank` (วิธีเดียวกับ T1) = **1,188.0 MB** ⇒ T1 delta **+270–279 MB** · T4 flat delta **−29 MB** ⇒ **ห้ามอ้างเลข tree ดิบ** |

**เครื่องมือตรวจอิสระที่เขียนเอง (ไม่พึ่งเครื่องมือของผู้เขียน):**
- `/tmp/pv-onnxmeta-9f3.py` — protobuf parser อ่าน ONNX `ModelProto`/`GraphProto` ตรงจากไบต์ (เครื่องนี้ python ไม่มี `onnx`/`onnxruntime`)
- `/tmp/pv-onnxgraph-8d2.py` — parser เต็ม: opset/node/initializer/attribute (strides, kernel, pads)
- `/tmp/pv-wasminfo-4a1.py` — WASM binary parser: version/imports/exports/memory
- `/tmp/pv-msgpack-2b7.py` + `/tmp/pv-mpkstats-3c9.py` — msgpack decoder เองสำหรับ `.mpk` (ไม่ใช้ burn)
- `/tmp/pv-t1-summary.py` — สรุปผลรัน POC

> ⚠️ **บทเรียนจากเครื่องมือตัวเอง:** parser ONNX เวอร์ชันแรกอ่านผิด (นึกว่า `graph = 8`, `opset = 2`) เพราะ ONNX **ไม่เรียงเลข field ตามลำดับใน .proto** (`graph = 7`, `opset_import = 8`) — แก้แล้วจึงอ่านถูก ทุกข้อสรุปใน §1.2 มาจาก parser ที่แก้แล้วเท่านั้น

---

## 1. T1 — POC vocoder บน browser

### 1.1 รันซ้ำด้วยคำสั่งของ T1 เอง (ไม่แก้)

```bash
bash web/run-vocoder.sh     # × 2 ครั้งอิสระ (คนละ process, คนละ Chromium, คนละ port)
```
ก่อนรัน ผมสำรอง artefact เดิมของ T1 ไป `/tmp/pv-backup/` (สคริปต์ `rm -f` ผลลัพธ์เดิมทิ้ง)

| ตัวเลข | T1 เอกสาร (Run A 13:59) | **ของผม Run 1** | **ของผม Run 2** |
|---|---|---|---|
| import ort-web | 16 ms | 17.1 ms | 15.1 ms |
| fetch model | 49 ms | 52.5 ms | 53.2 ms |
| session create | 196 ms | 194 ms | 195.8 ms |
| run1-cold n=64 | 1154.8 ms (0.64×) | **1148.3 ms (0.647×)** | **1147.0 ms (0.648×)** |
| run2-warm n=64 | 1137.6 ms (0.65×) | 1129.5 ms (0.658×) | 1123.6 ms (0.661×) |
| run3-otherN n=97 | 1621.8 ms (0.69×) | 1614.5 ms (0.698×) | 1624.0 ms (0.693×) |
| peak RSS tree | 1458.5 MB | 1458.4 MB | 1466.9 MB |
| biggest process | 403.7 MB | 421.7 MB | 407.5 MB |
| js heap | 5.0 MB | 5.521 MB | 5.752 MB |
| WAV sha256 | `de5622a36af22c17…` | **`de5622a36af22c17…`** | **`de5622a36af22c17…`** |
| wav fnv | 1450536567 | 1450536567 | 1450536567 |
| verdict | PASS | **==> PASS** | **==> PASS** |

⇒ เวลา **reproducible ภายใน ~1%** · WAV **byte-identical ข้ามรอบ และตรงกับที่เอกสารอ้างเป๊ะ**
⇒ RSS/JS heap **ผันแปรตามรอบ** (1458–1467 MB · 403–422 MB · 4.87–5.75 MB) — เอกสารรายงานค่าเดียวไม่ระบุรอบ

### 1.2 ตรวจ artefact + metadata โมเดล (ไม่เชื่อเอกสาร)

| สิ่งที่อ้าง (T1 §2) | ตรวจด้วย | ผล |
|---|---|---|
| sha256 `b297c0c7…` | `sha256sum` ไฟล์บนดิสก์ | **ตรง** |
| opset `ai.onnx` 15 | parser เอง | **ตรง** `[{'domain':'','version':15}]` |
| graph `torch_jit` · 272 nodes · 213 initializers | parser เอง | **ตรงทั้งสามค่า** |
| ไม่มี custom op | ไล่ node domain | **ตรง** — domain ของทั้ง 272 node เป็น `''` |
| op histogram (Conv 93 · LeakyRelu 86 · Add 59 · Div 7 · Mul 5 · ConvTranspose 5 · Slice 4 · Sub 2 · Pad 2 · Mod 2 · …) | parser เอง | **ตรงทุกตัว** |
| `mel` f32 `[1,n_frames,128]` · `f0` f32 `[1,n_frames]` · `waveform` `[1,n_samples]` | parser เอง | **ตรง** (elem_type 1 = FLOAT) |
| **hop 512 = ConvTranspose strides 8·8·2·2·2** (kernel 16/16/4/4/4, pads 4/4/1/1/1) | ดึง attribute เอง | **ตรง** — ผลคูณ strides = **512** เป๊ะ |
| `vocoder.yaml`: `num_mel_bins: 128`, `pitch_controllable: true` | `cat` | **ตรง** |
| ort-web 13.95 MB (367,950+24,381+14,239,897) | `ls`+`sha256sum` | **ตรง** และ sha256 ตรงกับ pin ใน `web/vendor/fetch-ort.sh` ทั้ง 3 ไฟล์ |
| โมเดล 53.99 MB · รวม 67.9 MB (71,248,871 B) | คำนวณซ้ำ | **ตรง** |
| native อ้าง `render.rs:196` · `r2_engine.rs:186` | อ่านโค้ดจริง | ⚠️ **เลขบรรทัดคลาดเคลื่อน** — จริงคือ `render.rs:199` · `r2_engine.rs:191` (ข้อความที่อ้างมีจริง) |

### 1.3 โมเดลถูกโหลด/รันจริงไหม (ไม่ใช่ค่าคงที่/ศูนย์)

หลักฐานที่แข็งที่สุดคือ **probe sweep** ซึ่งผมรันซ้ำได้ค่า**ตรงกันทุกหลักทศนิยมทั้งสองรอบ**:

| mel DC | peak (Run 1) | peak (Run 2) |
|---|---|---|
| −3 | 0.30309221148490906 | 0.30309221148490906 |
| −5 | 0.045649152249097824 | 0.045649152249097824 |
| −7 | 0.00647825887426734 | 0.00647825887426734 |
| −9 | 0.0010221382835879922 | 0.0010221382835879922 |
| −11 | 0.00012406740279402584 | 0.00012406740279402584 |

- peak max/min = 0.30309221/0.00012407 = **2,443×** ⇒ ตรงกับที่อ้าง และพิสูจน์ว่า output ขับด้วย mel จริง
- `runDeterminismMaxDiff = 0` · `nonFinite = 0` · `distinctLe8 = 8` (ไม่ใช่ค่าคงที่) ทั้งสองรอบ
- ความยาว 32,768 = 64×512 และ 49,664 = 97×512 ตรงตามที่อ้าง

### 1.4 มี hardcode / mock / ข้ามขั้นไหม (อ่านโค้ดเต็ม)

อ่านเต็ม: `web/vocoder-test.html` (6.7 KB) · `web/run-vocoder.sh` (2.9 KB) · `web/vendor/fetch-ort.sh` (1.3 KB) · `web/bench-server.py`

| พบ | ระดับ | หมายเหตุ |
|---|---|---|
| `R.hop = 512` **เขียนตายตัวในหน้าเว็บ** และสคริปต์ก็ใช้ literal 512 | ⚠️ ปานกลาง | **hop ไม่ได้ derive ตอนรัน** — ค่าที่เขียนถูกต้อง (ผมยืนยัน 512 จาก ConvTranspose strides ของไฟล์เอง) แต่กลไกที่โฆษณา ("from ConvTranspose strides") เป็นการวิเคราะห์คงที่ ไม่ใช่การวัด |
| `R.verdict = "PASS"` ตั้งโดยหน้าเว็บ + สคริปต์เชื่อ | ⚠️ ต่ำ | verdict ไม่ใช่การตรวจอิสระ **แต่**สคริปต์ตรวจเงื่อนไขจริงเพิ่มเอง (bytes, sha256, io names, len==n·512, nonFinite, peak, distinct, WAV re-parse) ⇒ ยอมรับได้ |
| เพจรัน ort จริง ไม่มี stub/mock | ✅ | `import` จาก `./vendor/ort/ort.min.mjs` (sha256 ตรง pin) · `InferenceSession.create(new Uint8Array(modelBuf))` จากไฟล์จริง · `session.run(feeds)` จริง |
| ไม่มี lookup table / early-return / คำตอบเตรียมไว้ | ✅ | สถิติทั้งหมดคำนวณจาก `Tensor.data` จริง |
| WAV มาจาก `R.wavB64` ของหน้าเว็บ | ✅ | สคริปต์ re-parse WAV ในเชลล์เอง (fmt/frames/peak/uniq) = ตรวจซ้ำชั้นที่สอง |
| "รันสคริปต์เต็ม **3 รอบ** WAV byte-identical" (§3.2) | ⚠️ | **สคริปต์ไม่ได้ตรวจ determinism ข้ามรอบ** (ตรวจแค่ run1 vs run2 ในโปรเซสเดียว) และไม่ได้สร้าง `prev.wav` ⇒ การเทียบทำมือ **แต่**ผมรันเอง 2 รอบได้ hash เดียวกัน + `wav`/`prev.wav` ของผู้เขียน hash เดียวกัน = 4 การสังเกตคนละโปรเซส ⇒ **จริงเชิงสาระ** |
| เอกสารขัดกันเอง §3.2 "3 รอบ" vs §4 "2 รอบ" | ⚠️ ต่ำ | ควรแก้ให้ตรงกัน |

### 1.5 ข้อกังวลที่ผู้เขียนไม่ได้พูดถึง (T1)

1. **"wasm heap อย่างเดียว ~404 MB" (§3.4) ติดป้ายผิด** — 403.7 MB คือ **RSS ของ process เดียวที่ใหญ่สุด** (renderer) ซึ่งรวม overhead ของ Chromium/V8 ไม่ใช่ wasm linear memory อย่างเดียว → **ปิดให้สมบูรณ์ใน §7**: baseline `about:blank` วิธีเดียวกัน = 1,188.0 MB (tree) / 222.2 MB (biggest process) ⇒ ส่วนของ engine จริงคือ **delta +185–200 MB** (renderer) หรือ **+270–279 MB** (tree) ไม่ใช่ 404 MB และไม่ใช่ 1,457 MB
2. **"รวมโหลด 245 ms" ไม่รวม import ort-web** — 245 = fetch(49)+session(196); ถ้ารวม import(16) เป็น ~261 ms ตารางวาง import เป็นแถวแล้วอ้าง "รวม 245" ทำให้อ่านผิดได้
3. **ตัวเลข §3 เป็นของ Run A เท่านั้น** ไม่บอกความแปรปรวน — ผมวัด run1 ต่าง ~0.6% แต่ RSS ต่างได้ ~13 MB, js heap ต่าง ~0.7 MB
4. **คุณภาพเสียงต่ำมาก** — peak 0.0065 (−43.8 dBFS) · rms 0.0017 · zcr ~9 kHz · dominant autocorr 86.13 Hz = 44100/512 = **frame-rate artifact** ⇒ WAV ฟังไม่รู้เรื่องเชิงดนตรี ผู้เขียนระบุข้อจำกัดนี้เอง แต่บรรทัด "`/tmp/vocoder-poc.wav` เปิดฟังได้" อาจทำให้เข้าใจผิดว่ามีเสียงร้อง
5. **mel ที่ป้อนเป็นของสังเคราะห์** และ f0 = 186 Hz glide — ยังพิสูจน์ไม่ได้ว่า "mel จาก acoustic จริง → เสียง" ใช้ได้ (ผู้เขียนเขียนใน §4 แล้ว)
6. **single-thread เพราะไม่มี COOP/COEP** — 0.64–0.70× realtime อาจดีขึ้นมากถ้าเปิด multi-thread/WebGPU
7. **เลขบรรทัด citation คลาดเคลื่อน** (`render.rs:196`→199 · `r2_engine.rs:186`→191)

---

## 2. T2 — R2/MoE wasm readiness + ตรวจ `.mpk`

### 2.1 artefact และการ parse เอง

| สิ่งที่อ้าง | ตรวจด้วย | ผล |
|---|---|---|
| `.wasm` 16,395,838 B | `ls -la` | **ตรง** |
| sha256 `76398cb32068710d897399252e77cde77bc5722b8bf4e7f662fc39e856bd0b8a` | `sha256sum` | **ตรงทุกหลัก** |
| wasm version 1 | parser WASM ของผม | **ตรง** |
| memory (min) 19 หน้า (~1.2 MB) ไม่มี max | parser | **ตรง** `[(19, None)]` |
| imports 6: `__wbindgen_placeholder__` ×4 + `__wbindgen_externref_xform__` ×2 | parser | **ตรงจำนวน/ที่มา** — ชื่อจริง `__wbg_getRandomValues_436a51d0629d84e1` · `__wbindgen_describe` · `__wbindgen_throw_bb96b2010945f0bc` · `__wbindgen_object_drop_ref` · `__wbindgen_externref_table_grow` · `__wbindgen_externref_table_set_null` |
| exports 1383 (รวม `moe_forward_probe`) | parser | **ตรง** |
| artefact ไม่ stale | เทียบ mtime | **ผ่าน** — `.wasm` 14:00:22 ใหม่กว่าซอร์ส T2 ทุกไฟล์ (ล่าสุด 14:00:09) |

### 2.2 ตรวจ `.mpk` ด้วย msgpack decoder ของตัวเอง (ไม่ใช้ burn)

- 6,177,944 B · sha256 `94c9caac24e413bbc53ab357555f6a889ecaf18dbbb8d9cd643f7720db82298f` ⇒ **ตรงกับที่อ้าง**
- decoder เองอ่าน **ครบทั้ง 6,177,944 ไบต์**
- metadata: `float=f32 · int=i32 · format=burn_core::record::file::NamedMpkFileRecorder<...FullPrecisionSettings> · version=0.21.0 · settings=FullPrecisionSettings` ⇒ **ตรงกับ §3.1 ทุกช่อง**
- โครงสร้าง: `{metadata, item, _b}`; `item` = `emb`, `fc_in`, `hiddens`(list 1), `out`, `dropout=null` ⇒ **ตรง**
- เทนเซอร์เก็บเป็น `{id, param:{bytes,shape,dtype}}` — ผม unpack เองเป็น f32 LE (ทดสอบ BE ด้วย: ได้ค่าเพี้ยน/NaN ⇒ ยืนยัน LE ถูก)

**ตาราง §3.2 — ผมคำนวณใหม่ทั้งหมด ตรงกับเอกสารทุกหลัก:**

| key | shape | n | min | max | mean | rms |
|---|---|---|---|---|---|---|
| `emb.weight` | [512,64] | 32,768 | −0.90192 | 0.86039 | −0.000090 | 0.086390 |
| `fc_in.weight` | [322,1024] | 329,728 | −7.10991 | 0.92826 | −0.000151 | 0.046462 |
| `fc_in.bias` | [1024] | 1,024 | −0.26853 | 0.35085 | −0.015475 | 0.088603 |
| `hiddens[0].weight` | [1024,1024] | 1,048,576 | −0.78331 | 1.02042 | −0.001292 | 0.024555 |
| `hiddens[0].bias` | [1024] | 1,024 | −0.67614 | 0.43542 | 0.153841 | 0.194891 |
| `out.weight` | [1024,128] | 131,072 | −0.40822 | 0.40404 | −0.027862 | 0.065281 |
| `out.bias` | [128] | 128 | −0.81734 | 0.12647 | −0.370355 | 0.417891 |

รวม **1,544,320 พารามิเตอร์** = 6,177,280 ไบต์ · overhead 664 ไบต์ ⇒ **ตรง**
⇒ checkpoint จริง (ค่ากระจายตัว) และ **ไม่มีเลเยอร์ HiFiGAN/NSF** (มีแต่ Embedding+Linear) ⇒ ข้อสรุป §3.3 "ไม่ใช่ vocoder จริง" **ถูกต้องตามหลักฐาน**

### 2.3 ข้ออ้าง getrandom 0.4 (§2.5) — ขัดกับ `web-render-backend.md` §1.3

| ข้ออ้าง | ตรวจจาก | ผล |
|---|---|---|
| `getrandom-0.4.3/Cargo.toml:158` values **ไม่มี** `wasm_js` | อ่านไฟล์จริง | **ตรง** — มี `custom, efi_rng, rdrand, rndr, linux_getrandom, linux_raw, windows_legacy, unsupported, extern_impl` |
| `src/backends.rs:171-184` เลือก backend จาก crate feature | อ่านไฟล์จริง | **ตรง** — `if #[cfg(feature="wasm_js")] { mod wasm_js } else { compile_error!(...) }` |
| ไม่มี `.cargo/config.toml` ซ่อน rustflags | `cat native/.cargo/config.toml` | **ตรง** — มีแค่ `[alias]` 2 ตัว; `RUSTFLAGS` ว่าง |
| wasm-bindgen/js-sys target-gate ที่ wasm | อ่าน Cargo.toml:90-100 | **ตรง** |

⇒ **T2 ถูก และ `web-render-backend.md` §1.3 ผิด**: ไม่ต้องใช้ `RUSTFLAGS='--cfg getrandom_backend="wasm_js"'` — ต้องมีแค่ crate feature `wasm_js`

### 2.4 cargo checks/tests — รันเองทั้งหมด (workspace healthy)

| # | คำสั่ง | ผลของผม | เทียบกับที่อ้าง |
|---|---|---|---|
| b2 | `cargo test -p moe-train` | **exit 0 · 5 passed / 0 failed** (6.93s) | **ตรง** |
| b1 | `cargo check -p moe-train --target wasm32 --no-default-features` | **exit 0** (cached 0.43s) | **ตรง** |
| b3 | build `moe_wasm_smoke` **target dir ใหม่ `/tmp/pv-t2-td`** | **exit 0 · 18.79s from scratch** → 16,395,838 B · sha256 `76398cb3…` | **ตรง + reproducible** |
| b3b | ลบ artefact แล้ว rebuild in-place | **exit 0** · sha256 ตรงกับ backup เดิมเป๊ะ | **ตรง** |
| a1 | `cargo check -p moe-train --target wasm32` (default features) | **exit 101** · `error: ort-sys@2.0.0-rc.13: no prebuilt binaries available for target wasm32-unknown-unknown` + `error: build script logged errors` | **ตรงคำต่อคำ** |
| — | `cargo tree … --no-default-features` หา wgpu/ort | **0 บรรทัด** | **ตรง** |
| — | `cargo check -p synth-cli -p synth-server` (regression) | **exit 0** (20.59s) — feature gate ไม่ทำ crate อื่นพัง | **ตรง** |
| — | `cargo run -p moe-train --example inspect_mpk -- test-data/pc-nsf-hifigan.mpk` | **exit 0** · current load OK → **current forward PANIC** `Dimensions are incompatible for matrix multiplication` (burn-ndarray `matmul.rs:125`) · legacy load OK · keys/shapes/stats ที่พิมพ์ออกมาตรงกับ decoder ของผมทุกค่า | **ตรงคำต่อคำ** |

### 2.5 เกินขอบเขตที่ T2 อ้าง: **ผู้ตรวจรัน wasm จริง** (ปิดช่องว่าง §2.6.1/§5.1)

T2 เขียนเองว่า "ยังไม่ได้รัน wasm — ต้องมี JS host + wasm-bindgen glue" ผมทำจนจบ (reproducible):

```bash
wasm-bindgen --target nodejs --out-dir /tmp/pv-moe-glue \
  native/target/wasm32-unknown-unknown/debug/examples/moe_wasm_smoke.wasm    # exit 0
# glue ไม่ export ฟังก์ชัน (เป็น extern "C" ไม่ใช่ #[wasm_bindgen]) → expose instance ของ glue เอง
node -e 'const m=require("/tmp/pv-moe-glue/moe_wasm_smoke.js"); m.__wasm.moe_forward_probe()'
```

| ครั้ง | `moe_forward_probe()` | finite |
|---|---|---|
| process A · call 0 | 1.2979992628097534 | true |
| process A · call 1 | 1.4312840700149536 | true |
| process A · call 2 | 1.086662769317627 | true |
| **process B** (คนละ process) | 1.1687109470367432 | true |

- **finite และไม่ใช่ศูนย์** ⇒ burn/ndarray forward จริงรันบน wasm32-unknown-unknown ได้ ไม่ใช่แค่ link
- ค่าแกว่งเพราะ `ContextAcoustic::new` สุ่ม init ⇒ **`__wbg_getRandomValues_*` ถูกเรียกใช้จริง** ไม่ใช่ import ค้าง
- **ความเป็นเจ้าของหลักฐาน: การรันนี้ทำโดยผู้ตรวจ (`poc-verifier`) ไม่ใช่ผู้เขียน T2 — เป็นหลักฐานอิสระคนละคน**

### 2.6 ข้อกังวลที่ผู้เขียนไม่ได้พูดถึง (T2)

1. **`native/Cargo.lock` ถูกแก้ (+1 บรรทัด `getrandom 0.4.3`) แต่ไม่อยู่ในตาราง §4 "ไฟล์ที่แตะในงานนี้"** ⇒ ตารางไม่ครบ
2. **ตาราง §0 แถว a2 เขียน "ผ่าน"** ทั้งที่หมายเหตุบอก "ยังไม่ได้รันในรีโปนี้" — ความซื่อตรงอยู่ในวงเล็บ แต่ช่อง "ผล" อ่านเดี่ยวแล้วแรงกว่าหลักฐาน
3. **feature-gate test** — `baseline.rs` เปลี่ยน `#[cfg(test)]` → `#[cfg(all(test, feature="wgpu"))]` ⇒ `cargo test --no-default-features` จะไม่เหลือ test ตัวนั้น (coverage ลด; ควรเขียนในเอกสาร)

---

## 3. เหตุการณ์ประสานงานที่กระทบการตรวจ (ต้องบันทึก)

ชุดตรวจ T2 ครั้งแรกของผม (job `bash-81`) ขั้น 1 (`cargo test`) ผ่าน exit 0 (5/5) แล้ว **ขั้น 2-7 ล้มทั้งหมด exit 101**:

```text
error: failed to load manifest for workspace member native/crates/moe-wasm
Caused by: failed to parse manifest at native/crates/moe-wasm/Cargo.toml
```

สาเหตุ: task-4 เพิ่ม `"crates/moe-wasm"` ใน `native/Cargo.toml` (mtime 14:05:48) ขณะที่ `crates/moe-wasm/Cargo.toml` ยังเขียนค้าง ⇒ **workspace ทั้ง repo รัน cargo ไม่ได้** (ไม่ใช่ความผิดของ T2)
- ผมไม่แก้ `native/Cargo.toml` หรือ `crates/moe-wasm/**` เอง (นอก write scope) — รายงาน Lead และรอ
- หลัง workspace กลับมา parse ผ่าน (`cargo metadata` exit 0) ผมจึงรัน Steps 2-7 ใหม่ทั้งชุด (job `bash-82`)
- **บทเรียนทีม:** เพิ่ม workspace member ต้องสร้าง `crates/moe-wasm/Cargo.toml` ให้ครบก่อน แล้วค่อยเพิ่มชื่อใน `native/Cargo.toml` ไม่งั้นคนอื่นที่รัน cargo พร้อมกันจะพังทั้งหมด

---

## 4. ตาราง verdict รวม (ข้ออ้าง → หลักฐาน → ผ่าน/ไม่ผ่าน)

| # | ข้ออ้าง | หลักฐานอิสระ | verdict |
|---|---|---|---|
| T1-1 | โหลด nsf_hifigan 56.6 MB เข้า browser ด้วย ort-web wasm ได้ | `run-vocoder.sh` PASS 2/2 รอบ · modelSha256 ตรงดิสก์ · session input/output names จากกราฟจริง | **ผ่าน** |
| T1-2 | output ยาว `n×512` ไม่ใช่ค่าคงที่/ศูนย์ | 32,768=64×512 · 49,664=97×512 · nonFinite 0 · distinctLe8 8 · `hop=512` ยืนยันจาก ConvTranspose strides ของไฟล์เอง | **ผ่าน** |
| T1-3 | output เปลี่ยนตาม input จริง | probe mel DC −3→−11: peak 0.30309221→0.00012407 = **2,443×** · ค่าตรงกันทุกหลักทั้ง 2 รอบ | **ผ่าน** |
| T1-4 | ตัวเลข reproducible | รัน 2 รอบ: 1148.3/1147.0 ms · WAV sha256 `de5622a3…` เท่ากันและตรงเอกสาร | **ผ่าน** |
| T1-5 | metadata โมเดล (opset/nodes/shapes/hop) | parser protobuf ของผู้ตรวจ — ตรงทุกข้อ | **ผ่าน** |
| T1-6 | ort-web vendored + pin sha256 ไม่ใช้ CDN | sha256 3 ไฟล์ตรง pin ใน `fetch-ort.sh` | **ผ่าน** |
| T1-7 | hop derive จากกราฟตอนรัน | พบว่า hardcode 512 (ค่าถูก แต่กลไกไม่ตรงคำอธิบาย) | **ผ่านแบบมีเงื่อนไข** |
| T1-8 | WAV byte-identical 3 รอบ | สคริปต์ไม่ตรวจข้ามรอบ; ผมรันเอง 2 รอบ + artefact ผู้เขียน 2 ไฟล์ = hash เดียวกัน 4 ครั้ง | **ผ่าน (เอกสารควรแก้ 3→2 รอบ)** |
| T2-1 | `ort` default features ล้มบน wasm32 | รันเอง exit 101 ข้อความตรงคำต่อคำ | **ผ่าน** |
| T2-2 | `--no-default-features` compile ผ่าน | รันเอง exit 0 · wasm graph ไม่มี wgpu/ort | **ผ่าน** |
| T2-3 | native ไม่พัง (tests) | `cargo test -p moe-train` 5 passed / 0 failed | **ผ่าน** |
| T2-4 | build cdylib → `.wasm` 16,395,838 B sha256 `76398cb3…` | target dir ใหม่ build from scratch (18.79s) + in-place rebuild → **hash เดิมเป๊ะ** | **ผ่าน + reproducible** |
| T2-5 | imports 6 รวม `__wbg_getRandomValues_*`, memory 19 หน้า, exports 1383 | parser WASM ของผู้ตรวจ | **ผ่าน** |
| T2-6 | getrandom 0.4 ไม่ต้องใช้ RUSTFLAGS | source จริง 2 จุด + ไม่มี rustflags ใน config/env | **ผ่าน (แก้ §1.3 ของ web-render-backend.md)** |
| T2-7 | `.mpk` เป็น checkpoint `ContextAcoustic` รุ่นเก่า ไม่ใช่ vocoder | msgpack decoder เอง: metadata + 7 tensors + stats ตรงทุกค่า + ไม่มีเลเยอร์ HiFiGAN | **ผ่าน** |
| T2-8 | ยังไม่ได้รัน wasm (ข้อจำกัดที่ผู้เขียนระบุ) | **ผู้ตรวจรันจริงแล้ว** → finite non-zero 4 ค่า | **เกินข้อจำกัดเดิม** |
| T4-1 | `./web/run-moe.sh` exit 0 + PASS | รันเอง 2 รอบ → `RUNNER_EXIT=0` + `==> PASS` ทั้งคู่ | **ผ่าน** |
| T4-2 | checkpoint 7,226,520 B sha256 `ba37fec6…` | `sha256sum` ตรง · decoder ของผมยืนยัน `fc_in [578,1024]` = WINDOW 9 · 1,806,464 params | **ผ่าน** |
| T4-3 | wasm module 3,015,349 B sha256 `4a91dac7…` | runner rebuild เองแล้วพิมพ์ sha เดิมทั้ง 2 รอบ | **ผ่าน + reproducible** |
| T4-4 | โหลด checkpoint บน wasm จริง | memory ตรงเป๊ะ 2 รอบ · **wasm ไม่มี import ระบบไฟล์ (มีแต่ `__wbindgen_*` 7 ตัว)** ⇒ อ่านไฟล์เองไม่ได้ | **ผ่าน** |
| T4-5 | output จริง + เปลี่ยนตาม input + deterministic | nonFinite 0 · distinct 64 · rms>0 · determinism 0 ทุกขนาด · sensitivity 1.5884 / 1.5884 / 1.69198 | **ผ่าน** |
| T4-6 | wasm ตรงกับ native | rel mean 1.207e-8 · rel rms 4.118e-9 · max\|diff\|first8 1.188e-7 (ตรงเอกสาร) | **ผ่าน** |
| T4-7 | ตัวเลขเวลา | wasm 16.8/67.3/513.7 vs เอกสาร 8.1/40.5/285.8 ms (ช้ากว่า ~1.6–1.8×) | **ผ่านแบบมีเงื่อนไข** (§6.4) |
| RAM-1 | RAM ที่ควรอ้างของ T1 | baseline 1,188.0 MB → delta **+270–279 MB** | **แก้การอ้าง** (§7) |
| RAM-2 | RAM ที่ควรอ้างของ T4 | linear memory 26.4–34.9 MB + JS heap 10–11 MB | **แก้การอ้าง** (§7) |

---

## 5. รายละเอียดคำสั่ง+exit code รอบสุดท้าย

```text
# job bash-82 (workspace healthy)
PRE_EXIT=0                      cargo metadata
TEST_EXIT=0                     running 5 tests / 5 passed; 0 failed
WASMCHECK_EXIT=0                cargo check -p moe-train --target wasm32 --no-default-features
WASMBUILD_EXIT=0                fresh target dir /tmp/pv-t2-td, 18.79s
  16395838 B  sha256 76398cb32068710d897399252e77cde77bc5722b8bf4e7f662fc39e856bd0b8a
REBUILD_EXIT=0                  in-place rebuild → sha256 ตรงกับ backup เดิม
WASMDEFAULT_EXIT=101            error: ort-sys@2.0.0-rc.13: no prebuilt binaries available for
                                target wasm32-unknown-unknown / build script logged errors
tree wgpu|ort count = 0
```

REGRESS_EXIT=0                  cargo check -p synth-cli -p synth-server (20.59s) — ไม่พัง
MPK_EXIT=0                      inspect_mpk on test-data/pc-nsf-hifigan.mpk
  current load        : OK  (burn 0.21 does NOT validate shapes on load)
  current forward     : PANIC (shape mismatch - the file is NOT the current 9x architecture)
    thread 'main' panicked at burn-ndarray-0.21.0/src/ops/matmul.rs:125:9:
      Dimensions are incompatible for matrix multiplication.
  legacy load         : OK (real burn NamedMpk checkpoint)
  total parameters    : 1544320   f32 payload: 6177280 bytes
```

**Cross-check อิสระ 2 เครื่องมือ:** สถิติ tensor ที่ `inspect_mpk` (ใช้ burn) พิมพ์ออกมา ตรงกับที่ msgpack decoder ของผู้ตรวจคำนวณเอง **ทุกค่าทั้ง min/max/mean/rms** และ shapes ตรงกัน ⇒ หลักฐาน §3.2 ของ T2 ถูก corroborate จากสองเส้นทางที่เป็นอิสระต่อกัน

**สรุปปิดท้าย:** T1 และ T2 ผ่านทั้งคู่ตามเกณฑ์ที่วางไว้ โดยมีข้อควรแก้ในเอกสาร (ไม่ใช่ข้อผิดพลาดของผลการทดลอง) ดังระบุใน §1.4, §1.5, §2.6

---

## 6. T4 — MoE/burn รัน wasm จริง + เทียบ native

### 6.1 artefact ที่ตรวจ (static)

| สิ่งที่อ้าง | ตรวจด้วย | ผล |
|---|---|---|
| checkpoint \`test/poc/bigset/w9-2048-model-g0.mpk\` 7,226,520 B sha256 \`ba37fec652cf1e1e…\` | \`sha256sum\` | **ตรงทุกหลัก** |
| wasm module (cargo release) 3,015,349 B sha256 \`4a91dac7e4273965…\` | \`sha256sum\` + runner rebuild เอง | **ตรง** — runner พิมพ์ sha เดิมทั้ง 2 รอบ ⇒ artefact ที่ทดสอบคือของที่ build สด |
| wasm module หลัง wasm-bindgen \`moe_wasm_bg.wasm\` sha256 \`3c0713e8…\` | \`sha256sum\` | **ตรง** (2,587,792 B) |
| \`www/pkg/\` เป็น generated ไม่ commit | \`git check-ignore\` + \`git ls-files\` | **ตรง** — \`.gitignore:53\` ignore และไม่มีไฟล์ใน index เลย |
| \`native/Cargo.toml\` +1 บรรทัด | \`git diff\` | **ตรง** — เพิ่ม \`"crates/moe-wasm"\` บรรทัดเดียว |
| checkpoint เป็น WINDOW=9 และเล็กสุดที่ใช้ได้ | msgpack decoder ของผมเอง | **ตรง** — \`fc_in.weight [578,1024]\` (=9·64+2) · รวม **1,806,464 params** · ไฟล์ 7,225,856 B payload + 664 overhead |
| ไฟล์ 6,177,944 B ใน bigset ใช้ไม่ได้ | decoder ของผม (\`big1842-model-g0.mpk\`) | **ตรง** — \`fc_in.weight [322,1024]\` = รุ่น WINDOW=5 เก่า (1,544,320 params) |
| \`test/poc/bigset/\` มีไฟล์ใหญ่กว่า 7,226,520 B ที่ WINDOW=9 ทั้งหมด | \`ls -S\` ทั้งไดเรกทอรี | **ตรง** — ที่ขนาด 7,226,520 B มี 10 ไฟล์ (เล็กสุดในกลุ่ม w9) ถัดไป 8,144,024 B และ 39–485 MB |

### 6.2 รันซ้ำเอง 2 รอบ (คำสั่งเดิม ไม่แก้)

\`\`\`bash
./web/run-moe.sh          # × 2 ครั้งอิสระ
\`\`\`
(สำรอง artefact เดิมของ T4 ไป \`/tmp/pv-backup-t4/\` ก่อน เพราะสคริปต์ \`rm -f\` ผลลัพธ์เดิม)

| ตัวชี้วัด | ผลของผม Run 1 | ผลของผม Run 2 | เอกสาร |
|---|---|---|---|
| \`RUNNER_EXIT\` | **0** | **0** | 0 |
| verdict | **PASS** | **PASS** | PASS |
| model bytes / sha256 | 7,226,520 · \`ba37fec6…\` | 7,226,520 · \`ba37fec6…\` | ตรง |
| wasm sha256 | \`4a91dac7…\` | \`4a91dac7…\` | ตรง |
| linear memory init → load → forward | 1,114,112 → 26,411,008 → 34,930,688 | **เท่ากันเป๊ะ** | ตรงเป๊ะ |
| js heap | 10 MB | 11 MB | 10 MB |
| determinism (1/128/1024) | 0 / 0 / 0 | 0 / 0 / 0 | 0 |
| input sensitivity | 1.5884 / 1.5884 / 1.69198 | **เท่ากันเป๊ะ** | ตรง |
| rows=128 mean (wasm) | 0.08286152899989929 | 0.08286152899989929 | 0.08286153 |
| rows=128 rms (wasm) | 0.23534576903079432 | 0.23534576903079432 | 0.23534577 |
| rel mean vs native | **1.207e-8** | **1.207e-8** | 1.21e-8 |
| rel rms vs native | **4.118e-9** | **4.118e-9** | 4.12e-9 |
| max\\|diff\\| first8 | **1.188e-7** | **1.188e-7** | 1.19e-7 |
| \`first8\` ฝั่ง native | \`-0.143587247, -0.174468338, …\` | เท่ากัน + เท่ากันทุกขนาด rows | — |

⇒ **ค่าทั้งหมด (ไม่ใช่เวลา) reproducible เป๊ะ** และ **ตรงกับเอกสารทุกตัว** — ข้ออ้าง §3 ของ T4 ยืนยันได้

### 6.3 assert ใน \`web/run-moe.sh\` ตรวจอะไรจริง (ผู้เขียนเรียก "9 ข้อ")

นับจริง: มี **19 \`need()\` call site** จัดเป็น **7 กลุ่ม** (ไม่ใช่ 9) — ไม่มีข้อใดเป็น decorative:

1. \`verdict == "PASS"\` ของหน้าเว็บ
2. \`modelSha256\` ของหน้าเว็บ == sha ของไฟล์บนดิสก์
3. \`modelBytes\` ของหน้าเว็บ == ของ native
4. \`memBytesAfterInit > 0\` (อ่าน memory ได้)
5. \`memBytesAfterLoad > memBytesAfterInit\` (memory โตตอนโหลด)
6. \`memBytesAfterLoad >= modelBytes\` (memory ≥ ขนาด checkpoint)
7. \`loadMs > 0\` และ \`instantiateMs >= 0\`
8. ต่อ run ทุกตัว: \`n == rows*128\` · \`nonFinite == 0\` · \`distinct > 1\` · \`rms > 0\` · \`ms > 0\`
9. determinism: \`maxDiff == 0\` ทุกขนาด
10. sensitivity: \`maxDiff > 1e-4\`
11. มี native case ตรงกับ rows ของ run label A
12. เทียบ native: \`rel_mean < 1e-4\` · \`rel_rms < 1e-4\` · \`max|diff|first8 < 1e-3\`

ข้อสังเกต: **threshold ข้อ 12 หลวมกว่าค่าที่วัดได้ ~10,000 เท่า** (1e-4 เทียบ 1.2e-8 · 1e-3 เทียบ 1.2e-7) ⇒ ถ้า wasm เพี้ยนขึ้น 10,000× ก็ยัง PASS — ค่าที่วัดได้ดีมาก แต่ assert เองไม่ใช่หลักประกันระดับนั้น **ควรแน่นขึ้น** (เช่น rel < 1e-6 · d8 < 1e-5)

### 6.4 "wasm vs native เป็นรอบเดียวกันจริงไหม" — ตรวจแล้วได้คำตอบละเอียด

**สิ่งที่เหมือนกันจริง (ยืนยันจากโค้ด):**
- **checkpoint bytes เดียวกัน** — ทั้งสองฝั่งเรียก \`Engine::from_mpk_bytes(bytes)\` (\`src/engine.rs:27\`)
- **โค้ด engine เดียวกัน** — \`engine.rs\` ถูก compile เข้า wasm module และเข้า binary \`moe-ref\` จาก source เดียวกัน
- **input generator เดียวกัน** — ฝั่งเบราว์เซอร์เรียก \`wasm_synth_ids/wasm_synth_feats\` ซึ่งเป็นฟังก์ชัน Rust ที่ export ออกมา (\`src/lib.rs:60-78\`) ไม่มีสูตรซ้ำใน JS; ฝั่ง native เรียก \`engine::synth_ids/synth_feats\` ตัวเดียวกัน ⇒ **input ตรงกันจริง**
- ลำดับการรันเหมือนกัน (A → A2 input เดิม → B input อีกชุด) ที่ sizes \`1,128,1024\` เท่ากัน

**สิ่งที่ไม่เหมือน (ต้องพูดให้ตรง):**
- **ไม่ใช่รอบเดียวกัน/โปรเซสเดียวกัน** — \`moe-ref\` รันในสเต็ป 2 (โปรเซสแยก) **ก่อน** เบราว์เซอร์จะรัน; การเทียบเป็นการเทียบข้ามโปรเซส ข้ามเวลา
- การเทียบนี้ **ถูกต้องสำหรับค่า** (ทั้งคู่เป็น deterministic function ของ weights+input ชุดเดียวกัน) แต่ **ไม่ถูกต้องสำหรับเวลา** (ดู §6.4.1)
- ฝั่ง wasm วัด \`performance.now()\` ครอบ \`forward()\` ซึ่ง **รวมต้นทุน marshal ข้าม JS↔wasm** (ids/feats เข้า, mel ออก) ส่วนฝั่ง native วัดเฉพาะ Rust forward ⇒ อัตราส่วนที่รายงานเป็น "wasm call ทั้งก้อน vs native compute" ไม่ใช่ compute ต่อ compute (ซึ่งทำให้ฝั่ง wasm ดูแย่กว่า — เป็นการอนุรักษ์นิยม)

#### 6.4.1 ตัวเลขเวลา **ไม่ reproducible** — ต่างจากเอกสาร ~1.6–1.8×

| งาน | เอกสาร | ของผม Run 1 | ของผม Run 2 | spread ของผม |
|---|---|---|---|---|
| wasm rows=1 | 8.1 ms | 16.8 ms (repeat 6.4) | 14.9 ms (repeat 6.8) | 13% |
| wasm rows=128 | 40.5 ms | 67.3 ms (repeat 61.1) | 66.6 ms (repeat 58.3) | 1% |
| wasm rows=1024 | 285.8 ms | 513.7 ms (repeat 474.2) | 436.8 ms (repeat 466.7) | 18% |
| native rows=1 | 4.9 ms | 6.083 ms (repeat 1.929) | 9.814 ms (repeat 2.263) | 61% |
| native rows=128 | 4.6 ms | 7.569 ms (repeat 4.603) | 6.084 ms (repeat 4.154) | 24% |
| native rows=1024 | 18.5 ms | 37.605 ms (repeat 25.842) | 27.687 ms (repeat 30.546) | 36% |
| ratio wasm/native | 1.7 / 8.8 / 15.4 | 2.76 / 8.89 / 13.66 | 1.52 / 10.95 / 15.78 | — |

- **ทิศทางและข้อสรุปหลักยังจริง**: wasm ช้ากว่า native หลายเท่าที่งานขนาดกลาง/ใหญ่ และอัตราส่วนที่ผมวัด (8.9–11.0× ที่ 128 เฟรม · 13.7–15.8× ที่ 1024 เฟรม) ตรงกับที่อ้าง (8.8× / 15.4×)
- **แต่ค่าสัมบูรณ์ไม่เสถียร**: native 1024 เฟรม 37.6 vs 27.7 ms (36%), wasm 1024 เฟรม 513.7 vs 436.8 ms (18%) ⇒ เอกสารที่รายงานทศนิยม 1 ตำแหน่ง (285.8 ms) สื่อความแม่นยำเกินจริง
- **ratio ที่ rows=1 (1.5–2.8×) ไม่ควรใช้เป็นตัวแทน compute**: native rows=1 ≈ rows=128 (6.1 vs 7.6 ms) เพราะจมอยู่กับ fixed per-call overhead; ตัวเลข compute ที่มีความหมายคือขนาด ≥1024 เฟรม
- ฝั่ง native ค่าที่รายงานเป็น **first call (cold)**; ค่า repeat ต่ำกว่ามาก (1.93 vs 6.08 ที่ rows=1) ⇒ ถ้าใช้ค่า steady-state อัตราส่วน wasm/native จะ**สูงกว่า**ที่รายงาน (คือข้อสรุปเดิมยังเข้าข้าง wasm อยู่แล้ว)

### 6.5 ข้อกังวลที่ผู้เขียนไม่ได้พูดถึง (T4)

1. **เวลาไม่ reproducible** (ข้อ 7.4.1) — เอกสารรายงานตัวเลขรอบเดียวเป็นตัวเลขหลัก และให้ทศนิยม 1 ตำแหน่งทั้งที่ spread ถึง 18–61%
2. **assert เทียบ native หลวมเกินไป** — threshold 1e-4/1e-3 vs ค่าจริง 1e-8/1e-7 ⇒ ตรวจไม่จับ regression ได้จริงในระดับที่ควร
3. **"linear memory โต ≥ ขนาดโมเดล" ไม่ใช่หลักฐานเดียวที่พอ** — memory โตอย่างเดียวไม่พิสูจน์ว่า weights อยู่ใน wasm; หลักฐานที่ชี้ขาดคือ (ก) **wasm ไม่มี import ระบบไฟล์เลย** (ผม parse เอง: มีแต่ \`__wbindgen_*\` 7 ตัว) ⇒ อ่าน .mpk จากดิสก์เองไม่ได้ (ข) \`load_checkpoint(bytes)\` สร้าง tensor ด้วย burn ใน wasm (ค) output ตรง native ถึง 1e-8 บน input เดียวกัน — ควรเขียนให้ครบสามข้อ
4. **การเทียบข้ามโปรเซส** — ควรระบุชัดว่าไม่ใช่ "รอบเดียวกัน" และการเทียบเวลาข้ามโปรเซสข้ามเวลาไม่ควรถูกอ้างเป็น benchmark ที่เทียบได้
5. **ratio ปนต้นทุน JS↔wasm marshalling** — ตัวเลข wasm ฝั่ง call ไม่ใช่ compute ล้วน
6. **เอกสาร §0 แถว 6 พูดปนกัน** ระหว่างค่า (1.21e-8 / 4.12e-9) กับ threshold (≤1.4e-8 / ≤1.5e-8) ทำให้อ่านเหมือนเป็นเกณฑ์ผ่าน
7. **จำนวน assert ไม่ตรงที่สรุปไว้** — brief ว่า "9 ข้อ" ของจริง 19 call site / 7 กลุ่ม (§6.3)
8. **POC ยังไม่มีเสียง** และ**ไม่มี vocoder บน wasm** — ผู้เขียนระบุใน `docs/planning/wasm-moe-run.md` §7 ข้อ 1 เองแล้ว (นับเป็นความซื่อตรง) แต่ชื่อเรื่อง "MoE/burn รัน wasm จริง" อาจถูกอ่านเป็น "เสียงร้องทำงานแล้ว" ได้

---

## 7. RAM — baseline + delta (ปิดช่องว่างที่ T1/T4 อ้างเลข tree ดิบ)

### 7.1 วิธีวัดไม่เหมือนกัน (สำคัญ)

| runner | วิธีนับ RSS |
|---|---|
| \`web/run-vocoder.sh\` (T1) | roots = โปรเซสที่ args มี token ของ \`--user-data-dir\` แล้ว **เดิน tree (BFS) รวม RSS ของลูกหลานทั้งหมด** → peak tree sum + biggest process |
| \`web/run-moe.sh\` (T4) | **ผลรวม flat** ของโปรเซสที่ args มี token (ไม่เดิน tree) |

⇒ ตัวเลข 1,457 MB (T1) กับ 1,045.8 MB (T4) **วัดคนละวิธี ⇒ เทียบกันตรง ๆ ไม่ได้**

### 7.2 baseline ที่ผมวัดเอง (\`about:blank\`, flag เดียวกับ T1, sampler คำนวณทั้งสองวิธี)

\`\`\`text
baseline tree_peak  = 1188.0 MB     (วิธีของ T1: BFS จาก roots)
baseline flat_peak  = 1075.0 MB     (วิธีของ T4: flat sum)
baseline biggest    =  222.2 MB     (โปรเซสเดียวที่ใหญ่สุด)
max roots           = 11
\`\`\`
- เทียบ baseline ที่ task-4 รายงาน **1,078.4 MB (flat)** → ของผม 1,075.0 MB ⇒ **ต่างกัน 3.4 MB (0.3%)** ยืนยันว่า independent measurement ตรงกัน

### 7.3 delta ที่เป็นตัวเลขจริง

| งาน | ค่าที่เอกสารอ้าง | baseline วิธีเดียวกัน | **delta (ของ engine จริง)** |
|---|---|---|---|
| **T1 vocoder** (tree) | 1,458.4 / 1,466.9 MB | 1,188.0 MB | **+270.4 / +278.9 MB** |
| **T1 vocoder** (biggest process) | 421.7 / 407.5 MB | 222.2 MB | **+199.5 / +185.3 MB** |
| **T4 MoE** (flat) | 1,045.8 MB | 1,075.0 MB | **−29.2 MB** (อยู่ใน noise) |
| **T4 MoE** (linear memory) | 26.4 → 34.9 MB | init 1.11 MB | **+23.8 MB** (แคบสุด/แม่นสุด) |
| **T4 MoE** (JS heap) | 10 / 11 MB | — | ~10 MB |

**อ่านตัวเลข:**
- T1: โมเดล 54.0 MB + ORT-web runtime 13.6 MB = payload 67.6 MB ⇒ delta tree ~270–279 MB ≈ **4× payload** และ renderer-process delta ~185–200 MB ≈ **2.7–3× payload** ⇒ ข้อสรุปเชิงคุณภาพของ T1 ("RAM เป็นกำแพงจริง") **ยังถูก** แต่ตัวเลขที่ควรอ้างคือ delta ไม่ใช่ 1,457 MB
- T4: flat delta ติดลบ ⇒ **RSS ทั้งก้อนถูกกลบด้วยตัวเบราว์เซอร์ทั้งหมด** ตัวเลขเดียวที่มีความหมายคือ **linear memory 26–35 MB + JS heap ~10 MB**

### 7.4 ⚠️ คำเตือนการอ้างอิง (ต้องใช้แทนตัวเลขเดิม)

> **ห้ามอ้าง "POC นี้ใช้ RAM ~1.0–1.5 GB"** — ตัวเลข peak RSS ของ Chromium process tree (ทั้งวิธี tree และ flat) **ส่วนใหญ่คือตัวเบราว์เซอร์เปล่า** (baseline \`about:blank\` = 1,188.0 MB tree / 1,075.0 MB flat)
> - **T1** ให้อ้าง: **delta ≈ +270–279 MB** (tree) หรือ **+185–200 MB** (renderer process) สำหรับโมเดล 54 MB + runtime 13.6 MB
> - **T4** ให้อ้าง: **wasm linear memory 26.4 → 34.9 MB** + **JS heap ~10 MB** (flat RSS delta ≈ 0)
> - ถ้าจะเทียบข้ามงาน **ต้องใช้วีธีวัดเดียวกัน** และรายงาน **baseline ของหน้านั้น** ควบด้วยเสมอ
> - ทั้งหมดวัดบน Chromium 153 headless บนเครื่องนี้เท่านั้น — ยังไม่มีตัวเลขแท็บเล็ต/มือถือ

---

## 8. คำสั่งรันซ้ำทั้งหมด (สำหรับผู้ตรวจคนต่อไป)

```bash
# T1
bash web/run-vocoder.sh                     # → ==> PASS ; WAV sha256 de5622a36af22c17…
sha256sum /tmp/vocoder-poc.wav

# T2 (ต้องให้ workspace parse ผ่านก่อน)
cd native
cargo test -p moe-train                     # → 5 passed
cargo check -p moe-train --target wasm32-unknown-unknown --no-default-features
cargo build -p moe-train --example moe_wasm_smoke --target wasm32-unknown-unknown --no-default-features
sha256sum target/wasm32-unknown-unknown/debug/examples/moe_wasm_smoke.wasm   # → 76398cb3…
cargo check -p moe-train --target wasm32-unknown-unknown                     # → ต้องล้มด้วย ort-sys
cargo check -p synth-cli -p synth-server    # regression
cargo run -p moe-train --example inspect_mpk -- test-data/pc-nsf-hifigan.mpk

# T4 (MoE/burn บน wasm จริง) — รัน 2 ครั้งอิสระ
cd /media/DATA/project/android-voice-synth
./web/run-moe.sh                            # → RUNNER_EXIT=0 + ==> PASS
sha256sum test/poc/bigset/w9-2048-model-g0.mpk            # → ba37fec6…
sha256sum native/target/wasm32-unknown-unknown/release/moe_wasm.wasm   # → 4a91dac7…

# RAM baseline (about:blank, sampler เดียวกับ T1 = BFS tree + flat + biggest)
prof=$(mktemp -d /tmp/pvbase-XXXXXX)
python3 /tmp/pv-baseline-sampler.py "$prof" /tmp/pv-baseline.txt 14 &
chromium-browser --headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage \
  --no-zygote --no-first-run --no-default-browser-check --user-data-dir="$prof" about:blank &
sleep 14; pkill -f -- "--user-data-dir=$prof"
cat /tmp/pv-baseline.txt                    # → tree 1,188.0 MB · flat 1,075.0 MB · biggest 222.2 MB

# ตรวจ artefact แบบไม่พึ่งเครื่องมือผู้เขียน
python3 /tmp/pv-wasminfo-4a1.py native/target/wasm32-unknown-unknown/debug/examples/moe_wasm_smoke.wasm
python3 /tmp/pv-wasminfo-4a1.py native/target/wasm32-unknown-unknown/release/moe_wasm.wasm   # → ไม่มี import ระบบไฟล์
python3 /tmp/pv-mpkstats-3c9.py native/test-data/pc-nsf-hifigan.mpk
python3 /tmp/pv-mpkstats-3c9.py test/poc/bigset/w9-2048-model-g0.mpk                          # → fc_in [578,1024] · 1,806,464 params
python3 /tmp/pv-onnxgraph-8d2.py test/golden/diffsinger-kiritan/nsf_hifigan/model.onnx
```

*ไฟล์นี้เป็นรายงานเดียวที่ผู้ตรวจเขียน · เครื่องมือชั่วคราวทั้งหมดอยู่ใต้ `/tmp` ไม่ใช่ deliverable และไม่แตะ `web/**` หรือ `native/**` (นอกจาก build output ใน `native/target` ที่ restore hash เดิมแล้ว)*

---

## 9. T9 (task-10) — root cause "โน้ตช่วงต้นผิด" + การแก้

> ผู้เขียน: r2-wasm (task-9) · เอกสาร: [pipeline-note-onset-bug.md](pipeline-note-onset-bug.md) · ผู้ตรวจ: \`poc-verifier\` (task-10)
> วิธีตรวจ: รันคำสั่งของผู้เขียนซ้ำเอง · วัด envelope/corr ด้วยสคริปต์ของผมเอง · **ปิด fix ในสำเนาที่ \`/tmp\`** เพื่อทดสอบว่า regression test จับ bug ได้จริง (ไม่แตะโค้ดของผู้เขียน)

### 9.1 ข้ออ้างหลัก → หลักฐานอิสระ → ผล

| # | ข้ออ้าง | หลักฐานของผม | ผล |
|---|---|---|---|
| 1 | **corr@0 กับ golden phrase = 1.000000** | \`/tmp/our-english.wav\` (72,132) vs \`/tmp/g-note-r1.wav\` (72,193): **Pearson 0.99999997** · cosine 0.99999997 · **best lag = 0** (สแกน ±3,000 samples) · max\\|A−B\\| = 35 จาก peak 19,800 (0.18%) · rms ratio 1.0005 | **จริง** ⇒ renderer/stretch ถูกต้องตามที่อ้าง |
| 2 | phrase renderer ถูก, bug อยู่ที่ mixdown | render สดด้วย CLI: **phrase ของ universal = 72,132 samples = ความยาวเดียวกับ mixdown ก่อนแก้เป๊ะ** ⇒ ก่อนแก้ mixer วางทั้ง phrase buffer (รวม pre-roll) ที่ 0 | **จริง** |
| 3 | root cause #1: japanese แบ่งโน้ตครึ่งต่อครึ่ง | ปิด fix ในสำเนา → test ได้ \`[s 0..720, "- a" 720..1440]\` = แบ่ง 50/50 จริง | **จริง** |
| 4 | root cause #2: mixer หัก leading ซ้ำ | \`mixer.rs:348\` test \`alignment_uses_position_minus_leading\` มีอยู่ก่อนแล้ว ⇒ convention คือ \`position − leading\` และ \`trim_chunk_audio\` เป็นฝ่ายคืนค่าผิด | **จริง** |
| 5 | onset 150 → 50 ms, ยาว 1636 → 1500 ms | \`ab_metrics.py\`: pre-fix 1636 ms/onset 150 ms · post-fix (ผม render เอง) **1500.0 ms/onset 50 ms** · golden demo 1501 ms/onset 50 ms | **จริง** |
| 6 | japanese หลังแก้ 66,150 / corr −0.108 | ของผม: 66,150 samples · corr@0 **−0.1077** | **จริง** |
| 7 | universal หลังแก้ corr +0.3797 | ของผม: **+0.3797** | **จริง** |
| 8 | envelope Pearson −0.17 (ไม่ใช่ pass) | ของผม: **−0.1702** · cosine ที่ไม่ลบ mean = **+0.9618** (คำเตือนในเอกสารถูกต้อง) · waveform best-lag 0.5212 | **จริง และไม่ได้ถูกนำเสนอเป็น "ผ่าน"** — อยู่ใน §6 ข้อจำกัด + test พิมพ์ "diagnostic only" และ**ไม่ assert** envelope shape (\`note_onset.rs:187-189\`) |
| 9 | 2 เทสต์พังใน \`rusted_utau_dict.rs\` เป็นของเดิม | ไฟล์ hardcode \`/home/seal/project/...\` · \`git log\` = "init" · ไม่อยู่ในรายการไฟล์ที่ถูกแก้ · รันได้ error \`NotFound("/home/seal/...")\` | **จริง — ของเดิม ไม่ใช่งานนี้** |
| 10 | regression test ใหม่จับ bug ได้จริง | ปิด fix ในสำเนา \`/tmp/pv-t10-src\` แล้วรัน: mixer **FAIL** \`pre-roll must be dropped: left 1000, right 559\` · japanese **FAIL** \`left 2, right 1\` (เห็นการแบ่ง 720/720) · fallback test ยัง **PASS** | **จริง** |

### 9.2 การแก้ไม่ทำให้ของเดิมพัง (รันเองทั้งหมด)

| suite | ผล |
|---|---|
| \`cargo test -p runtime\` | **exit 0** · 56 + 1 + 1 passed |
| \`cargo test -p phonemizer\` | 27 + 5 + 6 passed · **exit 101** เพราะ rusted_utau_dict 2 ตัว (ของเดิม) |
| \`cargo test -p synth-cli --features cli\` | **exit 0** · 22 passed / 11 ignored |
| \`cargo test -p synth-cli --test render_real -- --ignored\` (golden/เสียงจริง) | **exit 0 · 8 passed** (36.9 s) |
| \`cargo test -p worldline-plugin --test integration -- --ignored\` | **exit 0 · 2 passed** |
| \`cargo test -p engine-wasm\` (browser path ใช้ \`mixer::mix\` ร่วมกัน) | **exit 0 · 11 passed** |
| \`cargo test -p synth-cli --test note_onset -- --ignored\` | **exit 0 · 3 passed** |

⇒ \`mixer.rs\` ถูกใช้ร่วมกับ browser path (\`engine-wasm/src/lib.rs:551\`) และ golden/เสียงจริง — ทั้งหมดยังผ่าน

### 9.3 บทเรียน \`--features cli\` (ยืนยันเชิงประจักษ์)

ผมลบ binary แล้ว build ใหม่: \`cargo build --release -p synth-cli\` (ไม่ใส่ feature) **ไม่สร้าง binary เลย** (ถูก gate ด้วย \`[[bin]] required-features = ["cli"]\`) ⇒ ถ้ามี binary เก่าค้างอยู่ มันจะถูกใช้ต่อโดยไม่ relink — **ตรงกับคำเตือนของผู้เขียน** · เมื่อใส่ \`--features cli\` build ผ่าน exit 0 และ sha256 ตรงกับสำเนาสำรอง

### 9.4 ข้อควรแก้ / ข้อกังวล (T9)

1. **คำสั่งในเอกสาร \`cargo test -p runtime -p phonemizer  # 56 + 27 passed\` ไม่ reproduce ตามที่เขียน** — จริง ๆ ได้ **exit 101** และ (ถ้าไม่ใส่ \`--no-fail-fast\`) cargo จะ abort ที่ \`rusted_utau_dict\` ของ phonemizer ก่อน ⇒ **เทสต์ของ runtime ไม่ได้รันเลย** ตัวเลข 56/27 ถูกต้องแยกกัน แต่ต้องรันแยกหรือเติม \`--no-fail-fast\` (แล้วจะได้ 56+27 passed พร้อม 2 failures)
2. **พบ doctest พังใหม่ใน \`phonemizer/src/default.rs\` (ไฟล์ของ task-12 ไม่ใช่ T9)** — โค้ด C# ใน doc comment ถูก rustdoc คอมไพล์เป็น doctest: \`error: expected one of '!' or '::', found 'alias'\` ⇒ \`cargo test -p phonemizer\` ล้มเพิ่มอีกเหตุผลหนึ่ง ควรแจ้ง T12 (ผมรายงาน Lead แล้ว)
3. **path อ้างอิงในเอกสารผิด** — \`ref-openutau-clean/core/OpenUtau.Plugin.Builtin/JapaneseVCVPhonemizer.cs\` ไม่มีอยู่จริง · ไฟล์จริงอยู่ที่ \`ref(openutau+openutau mobile)/desktop-ref/OpenUtau.Plugin.Builtin/JapaneseVCVPhonemizer.cs\` (checkout \`ref-openutau-clean\` มีแค่ \`JapaneseCVVCPhonemizer.cs\`) — **เนื้อหาที่อ้างถูกต้อง** (ผมอ่านไฟล์จริงแล้ว: candidates \`["{vow} {lyric}", "* {lyric}", "{lyric}", "- {lyric}"]\` เมื่อมี prev neighbour และ \`["- {lyric}", "{lyric}"]\` เมื่อไม่มี ⇒ ตรงกับที่โค้ดแก้)
4. **เบี่ยงจาก parity โดยเจตนา (เอกสารระบุใน §4 แล้ว แต่ควรเน้น)** — เมื่อไม่เจอ alias ใดเลย OpenUtau คืน **1 phoneme = raw lyric**; โค้ด T9 fallback ไป **mora-split เดิม** ⇒ แบงก์ที่ไม่มี alias ทั้งคำจะยังได้หลาย phoneme (แลกกับการไม่ทำแบงก์เดิมพัง) ไม่ใช่ OpenUtau เป๊ะ 100%
5. **นิยาม onset ต่างกัน 2 ที่** — \`ab_metrics.py\` นับ 2 หน้าต่างติดกัน → **50 ms**; \`note_onset.rs::onset_ms\` นับหน้าต่างแรก → **0 ms** เอกสารอ้าง 50 ms แต่ test พิมพ์ 0 ms — ถูกทั้งคู่ แต่ควรระบุกันสับสน
6. **onset ระดับ phrase ยังเป็น 150 ms โดยธรรมชาติ** (phrase buffer ยังมี pre-roll) — มีแต่ mixdown ที่เป็น 50 ms ควรเขียนให้ชัดเพื่อไม่ให้มีคนเปิด bug ซ้ำ
7. **ยังไม่ได้ทดสอบความเทียบเท่า vowel-lookup ครบทุกกรณี** — reference หา VCV prefix จาก unicode ตัวสุดท้ายของโน้ตก่อนหน้าผ่านตารางสระ; T9 หาจาก \`plan_morae(...).tail\` — ตรงกันในกรณี "sa" (tail "a") แต่ผมยังไม่ได้ทดสอบ 拗音 (きゃ) / ん / ตัวลงท้ายที่ไม่ใช่สระ
8. **ไม่พบการ "แก้ให้ตัวเลขสวย"** — fix เป็นเชิงโครงสร้างทั้ง 3 จุด; test assert **ค่าตัวอย่างจริง** (\`samples[0] == samples[441]\`) ไม่ใช่แค่ความยาว; และการตัด sample ที่ target<0 จำเป็นเพราะเขียน index ติดลบไม่ได้ — พร้อมหลักฐานว่า crossfade ยังถูก (หลังตัด buffer เริ่มที่ \`t = i/overlap = 1.0\` ⇒ full gain ไม่ใช่กลาง crossfade)

---

## 10. T12 (DefaultPhonemizer) + T13 (note stretch) — task-14

> ผู้เขียน: r2-wasm · เอกสาร: [default-phonemizer-parity.md](default-phonemizer-parity.md) (T12) · [envelope-note-stretch-check.md](envelope-note-stretch-check.md) (T13) · ผู้ตรวจ: \`poc-verifier\` (task-14)
> หลักฐานของผม: รันเทสต์เอง · render เอง · **re-render golden phrase เองด้วย dotnet** · วัด corr/envelope/gain ด้วยสคริปต์ของผมเอง

### 10.1 T12 — DefaultPhonemizer: ผ่าน

| # | ข้อ | หลักฐานอิสระ | ผล |
|---|---|---|---|
| 1 | เทสต์ 11 ตัวผ่าน | \`phonemizer --lib default::\` **4 passed** · \`default_phonemizer_parity\` **4 passed** · \`phrase_segmentation_parity\` **3 passed** (exit 0 ทั้งหมด) | **ตรงเป๊ะ** |
| 2 | segmentation ตรง reference | printout เหมือนเอกสารทุกตัวอักษร: \`dropped 1 note(s): ["note 7 lyric \"ya\": no oto alias"]\` · 3 phrase \`@0.0 leading 135.7 dur 1250.0 → sa ku sa ku sa\` / \`@1500.0 70.6 500.0 → ku no\` / \`@2250.0 85.5 1250.0 → ma mo sa to ni\` | **ตรง** |
| 3 | sakura default = 3 phrase, 0 skipped, 154,349 samples, onset 50 ms | render เอง (engine r2, เคลียร์ cache): **3 phrase · 0 skipped · 154,349 samples (3500.0 ms) · peak 0.7880** → \`ab_metrics\`: onset **50 ms** · silence **13.1%** · clip 0 | **ตรง** |
| 4 | sakura universal = 2 phrase, silence 8.6% | render เอง: **2 phrase · 154,349 samples · peak 0.7880** → silence **8.6%** | **ตรง** |
| 5 | scale-demo default = 4 phrase, 14,500 ms, silence 0.0%, peak 0.991 | render เอง: **4 phrase · 639,449 samples (14,500.0 ms) · peak 0.9913** → onset **50 ms** · silence **0.0%** · clip 0 · (universal: peak 0.9075, silence 5.6%) | **ตรง** |
| 6 | silence ที่เพิ่มขึ้น = ทิ้งโน้ต \`ya\` ตาม reference ไม่ใช่ regression | **ตรวจเอง**: กลุ่มความเงียบ (<−45 dBFS, 10 ms) ของ default = \`(1240–1500)\` + **(2000–2180)** ส่วน universal มีแค่ \`(1240–1500)\` · ช่วง 2000–2250 ms: default mean **−162 dB** vs universal mean **−13.4 dB** · และ \`grep -ic ya oto.ini\` = **0** (ไม่มี alias ใดมี "ya") | **จริง** |
| 7 | โค้ด \`default.rs\` ตรงกฎ | อ่านเต็ม: 1 โน้ต→1 phoneme · lyric ดิบ · ตัด \`[hint]\` · ไม่มี alias → **ไม่ emit** + รายงาน · extender \`+\`/\`+N\` ยืด phoneme ก่อนหน้า · ไม่มี singer → emit unresolved | **ตรง** |
| 8 | \`from_track\` ไม่เปลี่ยน default เงียบ ๆ | \`None → Universal\` · \`"default" → Default\` · \`"japanese" → Japanese\` (test + อ่านโค้ด) | **ตรง** |
| 9 | เอกสารไม่ใช้ silence เป็นเกณฑ์หลัก | §4 เตือนว่า silence ขึ้นกับระดับเสียง (golden ดังกว่า **5.9 dB** ที่วัดเอง + clip 12,403) และให้ใช้ onset/phrase count/duration เป็นหลัก | **ตรง (มีเงื่อนไข — ดู 10.3.1)** |

### 10.2 T13 — note stretch: **ไม่พัง** (ยืนยันแล้ว)

ผม **re-render golden phrase เอง** ด้วย dotnet (\`WORLDLINE_RENDERER=1 TETO_DIR=... ./run.sh phrase\`) — ไม่ได้ใช้ไฟล์ของผู้เขียนอย่างเดียว:

| | golden phrase (ผม render เอง) | ตรงกับไฟล์เดิมของ T13 | ของเรา (ผม render เอง) |
|---|---|---|---|
| 1x (1440 ticks) | **72,193 samples** (1637.0 ms) | **byte-identical (max\\|diff\\| = 0)** | 66,150 (1500.0 ms) |
| 2x (2880 ticks) | **138,241 samples** (3134.7 ms) | **byte-identical (max\\|diff\\| = 0)** | 132,259 (2999.1 ms) |

ความเท่ากันของเนื้อเสียงที่ **offset = leading 5,982 samples** (ผมหา lag เองด้วย full-resolution scan):

| การเทียบ | waveform corr | envelope Pearson | ต่าง (samples) |
|---|---|---|---|
| ของเรา 1x vs golden 1x | **0.99999998** | **1.000000** | 6,043 = 5,982 + 61 ✓ |
| ของเรา 2x vs golden 2x | **0.99999999** | **1.000000** | **5,982 = leading เป๊ะ** ✓ |

- ยืนยันเพิ่ม: **mixdown ของเรา == phrase buffer ของเรา [5,982:] เป๊ะ** (max\\|diff\\| = **0**) ⇒ เป็นการตัด pre-roll จริง ไม่ใช่การยืด/บีบ
- ⇒ **stretch/region math ไม่พัง** ตามที่เอกสารสรุป · offset คงที่ (ไม่สะสม) ที่ 2x

### 10.3 ข้อควรแก้ / ข้อกังวล

**10.3.1 (T12) มีเส้นทางที่ 3 ที่เอกสารไม่ได้บอก — env var \`RUSTED_UTAU_PHONEMIZER\`**
\`PhonemizerKind::from_track\` มี override: ถ้า \`RUSTED_UTAU_PHONEMIZER=default\` (ไม่สนตัวพิมพ์) จะคืน \`Default\` **ก่อน** อ่าน \`track.phonemizer\` เลย — เอกสาร §4 เขียนว่า "เลือกได้ 2 ทาง" (\`--phonemizer default\` หรือ \`track.phonemizer\` มีคำว่า default) ⇒ **ไม่ครบ** และ env var ทำให้ผลการ render เปลี่ยนโดยไม่ปรากฏในโปรเจกต์ ควรเขียนในเอกสาร (หรือพิจารณาว่าต้องมีจริงไหม)

**10.3.2 (T13) ข้อความ "5 บล็อกแรกเท่ากัน" ไม่ถูกต้อง**
ผมวัด 50 ms RMS ของ golden 1x vs 2x เอง: บล็อก 0–3 ต่างกัน ~2e-5 ถึง 1.7e-4 (ไม่ bit-equal) และ **บล็อก 4 (200–250 ms) ต่าง 4.2e-3** (0.209711 vs 0.213912) — ตัวเลขในเอกสารเองก็พิมพ์แบบนั้นแต่ไปเขียนกำกับว่า "5 บล็อกแรกเท่ากัน" ⇒ สาระ (consonant/attack ไม่ถูกยืด) **ถูก** แต่ควรเขียนว่า "~4 บล็อกแรก (0–200 ms) ใกล้กันมาก แล้วเริ่ม diverge ที่ 200–250 ms"

**10.3.3 (T13) ยืนยันข้อผิดของ Lead ตามที่ขอ**
- คู่ที่ถูก: **phrase r1 (−12.8 dB) → demo (−12.2 dB) = +0.56 dB** (≈ +0.6 ที่เอกสารใช้) ✓
- ตัวเลข **−3.02 dB เดิมของ Lead ผิด** เพราะเทียบ phrase **RENDERER=2** กับ demo = คนละ renderer ✓ (ผมวัดคู่ผิดได้ **−2.61 dB** — ตัวเลขต่างเล็กน้อยจาก −3.02 แต่ **ธรรมชาติของข้อผิดตรงกัน**: เป็นการเทียบข้าม renderer)
- r1 → r2 = **+3.17 dB** ✓ (เอกสาร +3.2) · และ 72,704 vs 72,193 = คนละไฟล์จริง

**10.3.4 (T12/T13) เรื่องระดับเสียงกับผลของ task-16 — ไม่ขัดกัน แต่ T12 ยังไม่ระบุกลไก**
- T13 §3.4 **ไม่เดา**: คงสถานะกลไกที่ทำให้ demo env แกว่งคาบ ~325 ms ว่า **"ยังไม่ทราบ"** และตัดออกแล้วว่าไม่ใช่ (1) pure gain (2) alias/region (3) **การเลือก renderer ตาม \`WORLDLINE_RENDERER\`** เพราะ demo r1/r2 เหมือนกันเป๊ะ — ตรงกับข้อสรุปของ T16 พอดี ✓
- T12 §4 อธิบายส่วนต่างระดับว่า "มาจาก gain/stereo ของ harness .NET" — **กว้างเกินไป** ไม่ได้ระบุ pan law 0.7071 (−3.01 dB) ที่ T16 สรุปไว้ · ไม่ได้ขัดกับ T16 แต่ควรอัปเดตให้ชี้กลไกจริง (แจ้ง Lead แล้ว)

**10.3.5 หมายเหตุวิธีวัด (ของผมเอง)**
การหา lag ด้วยสัญญาณที่ decimate (step 4) ให้ค่า lag ผิด (6,784) — **full-resolution scan ให้ 5,982 ซึ่งถูกต้อง** ⇒ บทเรียน: อย่า decimate ตอน align เสียง (บันทึกไว้เพื่อความโปร่งใส)

---

## 11. สรุปสุดท้าย

| งาน | verdict | ตัวเลขที่ควรอ้าง |
|---|---|---|
| T1 vocoder wasm POC | **ผ่าน** | โหลด 245 ms · 0.64–0.70× realtime · hop 512 · WAV `de5622a3…` · **RAM delta +270–279 MB (tree) / +185–200 MB (renderer)** |
| T2 R2/MoE wasm readiness | **ผ่าน** | `cargo test` 5/5 · wasm 16,395,838 B `76398cb3…` (rebuild จากศูนย์ได้ hash เดิม) · getrandom = crate feature เท่านั้น |
| T4 MoE/burn รัน wasm จริง | **ผ่าน** | memory 1,114,112→26,411,008→34,930,688 B · determinism 0 · wasm/native rel 1.2e-8 · **เวลา ±40–80% อย่าอ้างเป็น benchmark** · **RAM: linear 26–35 MB** |
| ผู้ตรวจรัน wasm เอง (T2/T4) | **หลักฐานอิสระ** | T2: `moe_forward_probe()` = 1.2980/1.4313/1.0867/1.1687 · T4: PASS 2/2 รอบ + ค่าตรงเอกสารทุกตัว |
| **T9 onset bug + fix** | **ผ่าน** | corr@0 กับ golden phrase **0.99999997 · best lag 0** · onset 150→50 ms · ยาว 1636→1500 ms · ปิด fix แล้ว regression test FAIL จริง · golden 8/8 + browser 11/11 ยังผ่าน |
| **T12 DefaultPhonemizer** | **ผ่าน** | เทสต์ 11/11 · sakura default 3 phrase/154,349/onset 50/silence 13.1% · scale default 4 phrase/639,449/14,500 ms/peak 0.9913/silence 0.0% · silence ที่เพิ่ม = ทิ้งโน้ต `ya` (ไม่มี alias "ya" จริง) |
| **T13 note stretch** | **ผ่าน (stretch ไม่พัง)** | **ผม re-render golden เอง** (byte-identical) · corr@lag5982 = **0.99999998 / 0.99999999** · envelope Pearson **1.000000** ทั้ง 1x/2x · mixdown == phrase[5982:] เป๊ะ |

**ข้อควรแก้ที่รวมแล้ว:** T1 7 ข้อ (§1.4, §1.5) · T2 3 ข้อ (§2.6) · T4 8 ข้อ (§6.5) · RAM 1 เรื่องใหญ่ (§7) · T9 8 ข้อ (§9.4) · T12/T13 5 ข้อ (§10.3) — ทั้งหมดเป็นเรื่อง**การอ้าง/การวัด/การรันคำสั่ง** ไม่มีข้อใดทำให้ผลการทดลองหลักเป็นเท็จ

*อัปเดต: §9 (T9/task-10) ตรวจ root cause "โน้ตช่วงต้นผิด" + การแก้ (รันซ้ำ, วัด corr/envelope เอง, ทดลองปิด fix ในสำเนา `/tmp`) · §10 (T12+T13/task-14) ตรวจ DefaultPhonemizer + note stretch โดย **re-render golden phrase เองด้วย dotnet** และยืนยันข้อผิดของ Lead เรื่อง −3.02 dB*
