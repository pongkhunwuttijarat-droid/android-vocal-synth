# wasm-moe-run — รัน MoE/burn จริงบน wasm ในเบราว์เซอร์

> task: `task-4` → แก้ตามผลตรวจ `task-6` ใน `task-7` · วันที่: 2026-09-23 · สถานะ: **PASS**
> ต่อจาก: [wasm-r2-poc.md](wasm-r2-poc.md) (T2 = compile/link ผ่าน แต่ยังไม่เคยรัน)
> วิธีรัน: `./web/run-moe.sh`
> หลักการ: ทุกตัวเลขมาจาก output ของรอบจริง · **ตัวเลขเวลาเป็น "indicative" ไม่ใช่ benchmark** (ดู §3.1)

## 0. สรุป (ผ่าน/ไม่ผ่าน)

| # | ข้อ | ผล | หลักฐาน |
|---|---|---|---|
| 1 | โหลด MoE student checkpoint จริง (.mpk) **บน wasm** | **ผ่าน** | 7,226,520 B เข้า wasm: `paramCount 1,806,464` ตรงกับนับอิสระจากไฟล์ + linear memory 1,114,112 → 26,411,008 B (§4) |
| 2 | รัน forward จริงบน wasm (ไม่ใช่ native fallback) | **ผ่าน** | หลักฐาน 3 ขาใน §4 |
| 3 | output เป็นค่าจริง | **ผ่าน** | nonFinite 0 · distinct 64 · rms > 0 ทุก run |
| 4 | output เปลี่ยนตาม input | **ผ่าน** | max\|A−B\| = 1.5884 (rows 1/128), 1.69198 (rows 1024) |
| 5 | deterministic | **ผ่าน** | max\|run1−run2\| = **0** (bit-identical) ทุกขนาด |
| 6 | wasm ตรงกับ native — **ค่า** | **ผ่าน** | rel mean ≤1.4e-8 · rel rms ≤1.5e-8 · max\|diff\| 8 ค่าแรก 1.19e-7 · threshold ที่ใช้ 1e-6 / 1e-5 |
| 7 | ตัวเลขเวลา/RAM จริง | **ผ่าน** | §3 (เวลา — indicative) · §5 (RAM) |
| 8 | `./web/run-moe.sh` คืน exit 0 + PASS | **ผ่าน** | `RUNNER_EXIT=0` · asserts **25 call sites / 7 กลุ่ม** (สคริปต์นับเองและพิมพ์ออกมา) |

## 1. วิธีรัน

```bash
cd /media/DATA/project/android-voice-synth
./web/run-moe.sh                        # default: test/poc/bigset/w9-2048-model-g0.mpk, rows=1,128,1024
MODEL=test/poc/bigset/universal-all-w9-2048-vocab4096-model-g0.mpk ROWS=64 ./web/run-moe.sh
```

สิ่งที่สคริปต์ทำ (5 ขั้น):

1. build wasm release + wasm-bindgen glue + **วิเคราะห์ import/export section ของ .wasm เอง**
   (ด้วย `tools/wasm_imports.py` — ไม่พึ่ง external tool):
   ```bash
   cargo build --manifest-path native/Cargo.toml -p moe-wasm --lib --target wasm32-unknown-unknown --release
   wasm-bindgen --target web --out-dir native/crates/moe-wasm/www/pkg \
     native/target/wasm32-unknown-unknown/release/moe_wasm.wasm
   ```
2. build+รัน **native reference** (`--bin moe-ref`, คนละโปรเซส/คนละ host) → `/tmp/moe-native-ref.json`
   และนับพารามิเตอร์ของ `.mpk` แบบอิสระ (`tools/mpk_param_count.py`, ถอด MessagePack เอง)
3. เสิร์ฟ repo root ด้วย `web/bench-server.py` (ของโปรเจกต์) แล้วเปิดหน้าด้วย headless Chromium
   (ไม่ใช้ `--virtual-time-budget` เพราะมันแช่ `performance.now()` — วัดเวลาจริงผ่านการ POST กลับ)
4. รอหน้าเว็บ POST ผลไปที่ `/__result` แล้วเทียบกับ native reference
5. assert 7 กลุ่ม (ถ้าไม่ผ่านข้อใด → exit 1)

## 2. ของที่รัน

| รายการ | ค่า |
|---|---|
| checkpoint | `test/poc/bigset/w9-2048-model-g0.mpk` · 7,226,520 B · sha256 `ba37fec652cf1e1e6be1a447aa4add72a181bf9c0771d3ddece8b6d908eab624` |
| wasm module (cargo output) | 3,018,928 B · sha256 `dcfdb9ade6c2fafdefefdb2137be92b7a18d6a97f4620e86022a9f17436c9ff6` |
| wasm module (หลัง wasm-bindgen) | `moe_wasm_bg.wasm` 2,591,163 B · sha256 `2f2201ba8e411f03c172f529b6f2aa84b2886def96557d47d466246ec068e84f` |
| glue | `native/crates/moe-wasm/www/pkg/moe_wasm.js` · sha256 `d5e8df9e4efdc3aa59f0ee12006dd0d4c192232afb24cae9bfc45dc1f14beea1` |
| independent count | 7 tensors / **1,806,464 parameter elements** (`tools/mpk_param_count.py`) |

(hash ของ wasm เปลี่ยนได้เมื่อแก้โค้ด Rust — ตัวเลขข้างบนคือรอบ PASS ล่าสุด · ตรวจซ้ำด้วย `sha256sum native/target/wasm32-unknown-unknown/release/moe_wasm.wasm`)

**ทำไมเลือก checkpoint ตัวนี้:** เป็น `.mpk` ที่เล็กที่สุดใน `test/poc/bigset/` ที่ architecture ตรงกับ
`ContextAcoustic` ปัจจุบัน (`WINDOW = 9` ⇒ `fc_in.weight = [578, 1024]`):

| ไฟล์ | bytes | fc_in | ใช้ได้กับ WINDOW=9? |
|---|---|---|---|
| `train-set-model-general.mpk` | 6,177,944 | `[322, 1024]` | ❌ (WINDOW 5 รุ่นเก่า) |
| **`w9-2048-model-g0.mpk`** | **7,226,520** | **`[578, 1024]`** | ✅ เล็กสุดที่ใช้ได้ |
| `universal-all-w9-2048-vocab4096-model-g0.mpk` | 8,144,024 | `[578, 1024]` | ✅ |
| `universal-grid3x3-w9-2048-vocab4096-model-general.mpk` | 40,412,438 | `[578, 2048]` | ✅ (general, ใหญ่กว่า) |

shapes จริง: `emb.weight [512,64]` · `fc_in.weight [578,1024]` + bias `[1024]` · `hiddens[0].weight [1024,1024]` + bias `[1024]` ·
`out.weight [1024,128]` + bias `[128]` = 1,806,464 พารามิเตอร์ (นับจากไฟล์ด้วยเครื่องมืออิสระ)

หมายเหตุ: `w9-2048-model-g0.mpk` เป็น **group expert (g0)** ของชุด w9-2048 ไม่ใช่ general expert
— แต่เป็น checkpoint จริงที่โหลดผ่าน `moe_train::baseline::ContextAcoustic` ตัวจริง

## 3. ตัวเลขเวลา (indicative — ไม่ใช่ benchmark)

### 3.1 คำเตือนเรื่องการวัด (สำคัญ)

1. **สองฝั่งไม่ใช่โปรเซส/รอบเดียวกัน** — native `moe-ref` รันจบ *ก่อน* Chromium เปิด; ทั้งคู่ใช้ checkpoint bytes,
   `engine.rs` และ input generator ตัวเดียวกัน ⇒ **เทียบ "ค่า" ได้ แต่เทียบ "เวลา" ไม่ได้**
2. ฝั่ง wasm **รวมเวลา marshal JS↔wasm** (คัดลอก input/output ข้ามพรมแดน) ไม่ใช่เวลา kernel ล้วน
3. ตัวเลขต่อรอบแกว่งมาก: รอบของผู้ตรวจได้ **16.8 / 67.3 / 513.7 ms** เทียบรอบผู้เขียน **8.1 / 40.5 / 285.8 ms**
   = ต่างกัน **1.6–1.8×**; และ native ของผู้ตรวจแกว่งถึง **61%** (6.08 vs 9.81 ms)
   ⇒ ให้ใช้ตัวเลขเหล่านี้เป็น **"ลำดับขนาด"** เท่านั้น และข้อสรุปที่มั่นคงคือข้อ (4) ด้านล่าง
4. ข้อสรุปเชิงทิศทางที่คงอยู่ทุกรอบ: **wasm ช้ากว่า native ราว 8.9–15.8×** ที่งานขนาดกลาง/ใหญ่

### 3.2 รอบ PASS ล่าสุด (indicative)

```text
import glue      : 14.6 ms
wasm instantiate : 7.2 ms
checkpoint fetch : 10.0 ms
load inside wasm : 9.9 ms   <- NamedMpkBytesRecorder + tensor build, ใน wasm
```

| งาน | wasm ใน Chromium | native release | อัตราส่วน |
|---|---|---|---|
| forward 1 เฟรม (mel 128) | 7.0 ms (run ซ้ำ 4.4 ms = 0.63×) | 4.64 ms | ~1.5× |
| forward 128 เฟรม (mel 16,384) | 40.3 ms (run ซ้ำ 37.7 ms = 0.94×) | 3.83 ms | ~10.5× |
| forward 1024 เฟรม (mel 131,072) | 283.7 ms (run ซ้ำ 282.9 ms = 1.00×) | 19.18 ms | ~14.8× |

ค่าที่ตรงกัน (rows=128): wasm `mean 0.08286153 · rms 0.23534577` · native `mean 0.082861528 · rms 0.235345770`
→ rel mean 1.21e-8 · rel rms 4.12e-9 · max\|diff\| 8 ค่าแรก 1.19e-7

## 4. ทำไมเชื่อว่านี่คือ "รันบน wasm" จริง — หลักฐาน 3 ขา

**(ก) wasm module ไม่มีทางเข้าถึงระบบไฟล์เลย** — `tools/wasm_imports.py` อ่าน import section เองได้:

| module | importCount | modules ที่ import |
|---|---|---|
| `moe_wasm.wasm` (cargo) | 7 | `__wbindgen_placeholder__`, `__wbindgen_externref_xform__` |
| `moe_wasm_bg.wasm` (หลัง wasm-bindgen) | 4 | `./moe_wasm_bg.js` |

ทุก symbol ขึ้นต้นด้วย `__wbg_`/`__wbindgen_` และ **ไม่มี** module ที่เป็น `wasi*`, `env` หรือชื่อเกี่ยวกับ fs
⇒ โมดูลไม่มี WASI/ไฟล์ระบบให้ fallback ไปอ่านไฟล์บนดิสก์ได้ — runner assert ข้อนี้กับ **ทั้งสองไฟล์**

**(ข) `load_checkpoint(bytes)` สร้าง tensor จริงใน wasm** —
`wasm-bindgen` export `loaded_param_count()` ที่คืน `Module::num_params()` ของโมดูลที่โหลดแล้ว:

```text
wasm weights     : paramCount 1806464 (wasm) == 1806464 (native) == 1806464 (independent .mpk count)
wasm linear mem  : init 1114112 B -> load 26411008 B -> forward 34930688 B
                   (delta +25296896 B ตอนโหลด, +8519680 B ระหว่าง forward)
```

จำนวน 1,806,464 มาจาก `tools/mpk_param_count.py` ที่ถอด MessagePack ของ `.mpk` เอง (ไม่ใช้ burn/cargo)
⇒ ไม่ใช่การเชื่อค่าที่ wasm รายงานเอง · ส่วน linear memory delta เป็น **ข้อมูลเสริม** ไม่ใช่หลักฐานหลัก

**(ค) output ตรงกับ native** — rel mean ≤1.4e-8 / rel rms ≤1.5e-8 / max\|diff\| 1.19e-7 (threshold 1e-6 / 1e-5)

## 5. RAM (วัดจริง + ข้อควรระวัง)

| ตัวชี้วัด | ค่า | หมายเหตุ |
|---|---|---|
| wasm linear memory หลัง init | 1,114,112 B (1.06 MB) | ก่อนโหลดโมเดล |
| wasm linear memory หลังโหลดโมเดล | 26,411,008 B (25.2 MiB) | **delta +24.1 MiB** = น้ำหนัก + buffer ของ msgpack |
| wasm linear memory หลัง forward (1024 เฟรม) | 34,930,688 B (33.3 MiB) | **delta +8.1 MiB** = activation |
| JS heap (used / limit) | 10 MB / 4,192 MB | `--enable-precise-memory-info` |
| peak RSS ของ Chromium process tree | 1,082.9 MB (รอบก่อน 1,045.8 MB) | **ไม่ใช่ RAM ของ wasm** — ดูด้านล่าง |

⚠️ **ข้อควรระวังเรื่องการเทียบ RAM ข้ามงาน:**

- T4 วัด RSS ด้วย **flat sum** ของ RSS ทุกโปรเซสที่ args มี `--user-data-dir` ของเรา
- T1 (`run-vocoder.sh`) วัดด้วย **BFS tree walk** (นับเฉพาะ subtree ของ root ที่ match)
- ⇒ ตัวเลข **1,457 MB (T1) กับ 1,082.9 MB (T4) เทียบกันตรง ๆ ไม่ได้** — วิธีนับต่างกัน (page ที่ shared ถูกนับซ้ำไม่เท่ากัน)
- และ Chromium baseline (`about:blank`, วัดด้วยวิธีเดียวกับ T4) = **1,078.4 MB** ⇒ RSS แทบไม่ต่างจาก baseline
  ⇒ **ห้ามอ้างว่า POC นี้ใช้ RAM ~1 GB** — RAM ที่เกี่ยวกับงานนี้คือ linear memory 25–33 MiB + JS heap ~10 MB

## 6. เกณฑ์ที่ `web/run-moe.sh` assert (25 call sites / 7 กลุ่ม)

สคริปต์นับเองด้วย `grep -cE "^[[:space:]]*need\\("` แล้วพิมพ์เป็นบรรทัด `asserts : 25 need() call sites in 7 groups`
⇒ ตัวเลขในเอกสารนี้ตรงกับที่รันจริง ไม่ใช่การนับมือ · **ไม่มีข้อไหน decorative** (ทุกข้ออ้างอิงค่าที่วัดจริง)

| กลุ่ม | สิ่งที่บังคับ | จำนวน call sites |
|---|---|---|
| G1 | page verdict = PASS · sha256 โมเดลตรงดิสก์ · ขนาดตรงกับ native | 3 |
| G2 | wasm ถือ weights จริง: import ทั้ง 2 ไฟล์เป็น wasm-bindgen shim ล้วน (ไม่มี WASI/fs) · `paramCount` (wasm) = `paramCount` (native) = นับอิสระจาก `.mpk` · linear memory โตและ ≥ ขนาดโมเดล | 9 |
| G3 | บันทึกเวลา load/instantiate จริง | 2 |
| G4 | ทุก forward: `mel len == rows*128` · finite · ไม่คงที่ · rms > 0 · มีเวลา | 5 |
| G5 | รันซ้ำได้ค่าเดิม (`maxDiff == 0`) | 1 |
| G6 | output เปลี่ยนตาม input (`maxDiff > 1e-2`; ค่าจริง 1.59) | 1 |
| G7 | wasm = native: มี case ของ native · `rel mean < 1e-6` · `rel rms < 1e-6` · `max\|diff\| 8 ค่าแรก < 1e-5` | 4 |

(threshold G7 แน่นจาก 1e-4/1e-3 → **1e-6/1e-5** หลังผลตรวจ task-6; ค่าจริงห่างจาก threshold ~100× จึงยังผ่าน)

## 7. ข้อจำกัด / ยังไม่ผ่าน

1. **ไม่มีเสียง** — POC พิสูจน์ถึง mel (acoustic) เท่านั้น ยังไม่มี vocoder บน wasm
   (vocoder จริงคือ ONNX ที่ต้องใช้ ort-web แบบ async — [wasm-r2-poc.md](wasm-r2-poc.md) §1.4)
2. **wasm ช้ากว่า native ~8.9–15.8×** ที่งานกลาง/ใหญ่ และตัวเลขยังแกว่ง 1.6–1.8× ระหว่างรอบ
   สาเหตุที่ตรวจได้: wasm build ใช้ burn-ndarray **single-thread และไม่เปิด `simd`/`multi-threads`**
   (features = `["ndarray","std"]`) ขณะที่ native ใช้ default ของ burn (มี rayon) — ยังไม่ได้ลองปิดช่องนี้
3. ทดสอบเฉพาะ Chromium 153 headless บนเครื่องนี้ — ไม่ได้ทดสอบ Firefox/Safari/มือถือ
4. `native/crates/moe-wasm/www/pkg/` เป็นไฟล์ **generated** (ไม่ commit) — `web/run-moe.sh` สร้างใหม่ทุกครั้ง
5. รอบแรกต้อง compile release ทั้ง wasm และ native (เครื่องนี้ ~20 s + ~1.5 นาที เมื่อ cache อุ่น)
6. burn 0.21 **ไม่ตรวจ shape ตอน load** ⇒ โค้ดเลือก checkpoint ด้วย `fc_in` dim ให้ตรง `WINDOW=9` เอง
   (ผิดรุ่นจะ panic ตอน forward ไม่ใช่ตอน load)
7. ยังไม่ได้วัดบนเครื่องช้า/มือถือ และยังไม่ได้ทดสอบ checkpoint ขนาด 40 MB (general)

## 8. ไฟล์ที่แตะ

| ไฟล์ | สถานะ |
|---|---|
| `native/crates/moe-wasm/Cargo.toml` | crate ใหม่ (cdylib+rlib, wasm-bindgen, getrandom wasm_js) |
| `native/crates/moe-wasm/src/lib.rs` | wasm-bindgen API: `load_checkpoint`, `forward`, `loaded_param_count`, `wasm_memory_bytes`, `wasm_synth_*` |
| `native/crates/moe-wasm/src/engine.rs` | engine ร่วม (native + wasm): `NamedMpkBytesRecorder` จาก bytes, forward, `param_count`, stats |
| `native/crates/moe-wasm/src/bin/moe_ref.rs` | native reference (โค้ดเดียวกัน คนละโปรเซส/host) |
| `native/crates/moe-wasm/tools/mpk_param_count.py` | นับพารามิเตอร์จาก `.mpk` แบบอิสระ (ถอด MessagePack เอง) |
| `native/crates/moe-wasm/tools/wasm_imports.py` | อ่าน import/export section ของ `.wasm` เอง (พิสูจน์ว่าไม่มี WASI/fs) |
| `native/Cargo.toml` | +1 บรรทัด: `"crates/moe-wasm"` ใน workspace members (Lead อนุมัติ) |
| `web/moe-test.html` | หน้า harness (วัดเวลา/linear memory/JS heap/paramCount, POST ผลเป็น JSON) |
| `web/run-moe.sh` | runner headless + assert 25 call sites / 7 กลุ่ม |
| `docs/planning/wasm-moe-run.md` | เอกสารนี้ (แก้ตามผลตรวจ task-6 ใน task-7) |

ไม่ได้แตะ: `native/crates/moe-train/**` (รวม examples), `docs/planning/vocoder-wasm-verification.md`, `web/r2-*`, `models/**`
