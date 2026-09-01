# Golden Comparison Report — Rust vs C# (WORLDLINE) Render Fidelity

**Date:** 2026-08-08 (อัปเดต) · **Fixture:** scale-demo.ustx (s/l/3/la, C4-D4-E4-F4, 120 BPM)
**Engine:** Rust `synth-cli` (worldline-plugin + vendored C++ libworldline.so) vs C# golden-renderer (OpenUtau ref)

---

## 1. สรุป (TL;DR)

| คู่เทียบ | RMS ratio (s/l/3/la) | สถานะ |
|---|---|---|
| **v1 ↔ r1** (WORLD resynthesis) | **1.01 / 1.06 / 1.13 / 1.04** | ✅ ตรง |
| **v2 ↔ r2** (neural: mel → HiFiGAN) | **1.00 / 1.03 / 0.94 / 1.02** | ✅ ตรง (mel powers identical กับ C#) |

- **apples-to-apples:** golden v1 ใช้ raw C++ PhraseSynth (path เดียวกับ r1);
  golden v2 ใช้ SynthFeatures → mel.onnx → pc-nsf-hifigan (path เดียวกับ r2)
- **mel spectrogram ของ r2 ตรง C# เป๊ะ** (frame powers 167.08 / 29.36 / 32.46 … เท่ากันทุกค่า)
- 430 workspace tests, 0 FAILED

## 2. สถาปัตยกรรม — ใครทำอะไร ("ทำไมไม่ใช้ C++ เลย")

```
.ustx → phonemizer (Rust) → phrase timing (Rust)
     → convert.rs (Rust)  ── layout ──▶ SynthRequest / features
     → libworldline.so (C++) ── WORLD synthesis ──▶ PCM
     → (r2) mel.onnx + pc-nsf-hifigan.onnx ──▶ PCM
```

C++ (libworldline.so) คือ synthesis engine จริง — ใช้อยู่แล้วทั้ง 2 engines.
จุดที่ Rust ต้อง match คือ **layout layer** (`convert.rs` ↔ C# `ResamplerItem.cs`
/ `WorldlineRenderer.cs`) + feature/neural chain (r2) — ไม่ใช่ synthesis เอง.

**Engine matrix (ตาม user: "ใช้ทั้งคู่เลย"):**
- **r1** = C++ `PhraseSynth` (PhraseSynthNew/AddRequest/SetCurves/Synth) = path ของ **OpenUtau Mobile** — frame 10ms official (build-linux-v1) หรือ 11.61ms fork (build-linux)
- **r2** = `SynthSegment` (SynthFeatures port) → mel.onnx → pc-nsf-hifigan = **Desktop R2**
- golden harness เลือกผ่าน `WORLDLINE_RENDERER=1|2`; server เลือกผ่าน `--mel-onnx/--vocoder-onnx`

## 3. Root causes ที่เจอและแก้ (2026-08-06 → 08-08)

### 3.1 phoneme boundary — direct oto alias ✅
Rust letter-split "la" → l+A แต่ bank มี alias `la` → `UniversalPhonemizer`
ขั้นตอน 0: direct alias lookup → phoneme timing/env identical กับ golden.

### 3.2 golden v1 path ผิด (path ที่สาม) ✅
C# `WorldlineRenderer(1)` เดิมเรียก `PhraseSynthV2.Synth()` =
SynthFeatures → **WorldSynthesis (path ที่สาม)** — ไม่ใช่ C++ PhraseSynth.
แก้: v1 ใช้ `Worldline.PhraseSynth` (raw C++ — path เดียวกับ r1).

### 3.3 r1 volume: ×0.44 calibration ผิด ✅
convert.rs ส่ง VOL×0.44 (calibrate สำหรับ C# v2 path) แต่ C++ PhraseSynth
ใช้ `gain = volume×0.01` ตรง → r1 เบา 2.27×. แก้: ส่ง raw VOL (100).
r2 ใช้ `request.volume×0.01×autoGain` (ตรง C# item.volume×0.01).

### 3.4 C++ AutoGain fork bug ✅
fork `effects.cpp` ใช้ `(out_max·gain)·w + src_max·(1-w)` — C# เรียก
`GetAutoGain` **ก่อน** samples×gain (segMax unscaled) → revert เป็น official
`out_max·w + src_max·(1-w)`.

### 3.5 r2 time-stretch: srcStartFrame absolute ✅
r2.rs ใช้ `src_start` (63) เข้า `f0_trimmed` (เริ่ม 61) → offset 2 frames +
clamp tail เข้า phoneme ถัดไป (s ดึง f0 ของ vowel 3 → voiced 44/44).
แก้: `src_start_rel = src_start - trim_start` + `src_len_ms = (src_end-src_start)×frame_ms`.

### 3.6 r2 durRequired double-round ✅
convert.rs `required_length()` round 50ms แล้ว (550) — r2.rs ไม่ round ซ้ำ
(เดิม patch round ซ้ำ → 600/52 frames — revert; ใช้ `ceil(550/11.61)=48` ตรง C#).

### 3.7 **r2 auto_gain logistic sign — s gain 1.86×** ✅ (ตัวสุดท้าย)
C# `GetAutoGain`: `weight = 1/(1+Exp(5−10·ratio))` — Rust เขียน
`(-(5-10r)).exp()` = `Exp(10r−5)` — **กลับข้าง**: สำหรับ s (ratio 0.19)
weight 0.958 (max≈segMax → autoGain 1.04) แทน 0.042 (max≈wavMax → 0.56)
→ s gain ต่าง 1.86× → s RMS ratio 0.55. แก้เป็น `(5.0 - 10.0*ratio).exp()`.

### 3.8 frq parse offset + f0_floor ✅
- `parse_frq` length @ offset 36 (ไม่ใช่ 28 — กลาง blank 16 bytes) — ใช้ voicebank crate
- f0_floor = `GetF0FloorForCheapTrick(44100,2048)` = 64.69 (ไม่ใช่ 40)
- frq ว่าง (unvoiced) → zero grid (C# method -1 semantics) — ห้าม fallback DIO

### 3.9 cache ต่าง ๆ ✅
- golden harness **cache hit** (wdl-v{ver}-{hash}.wav) → ข้าม v2 branch — ลบ cache ก่อน render
- Rust cache salt ต่อ engine (`0x726c31…` r1 / `0x726c32…` r2) — R1/R2 ไม่ปน cache
- `"(cached)"` ใน verbose = `cache.is_some()` ไม่ใช่ cache hit

### 3.10 chunk pitch-curve rebase — scale demo ตอนลง pitch ผิด ✅ (2026-08-08)
chunking rebase `phrase.position_ms` ไปที่ chunk origin แต่ **`pitches_cents`/`curves`
ยังเป็น full-phrase arrays** → ทุก chunk หลังแรก sample pitch ผิดตำแหน่ง
(scale demo: chunk 2 ขึ้นไป render pitches ของ notes 0-7 ซ้ำ — ตอนลง "เด้งกลับขึ้น").
fix: `pipeline.rs` slice arrays ตาม `(origin_new−origin_old)/interval` (floor —
grid 5-tick; sub-interval remainder เท่ากับ whole-phrase path).
verify: 4 chunks ของ scale-demo-app.ustx (29 notes) — f0 sequence ตรง note targets
ทุก chunk (chunk2 = B5 C6 B5 A5 G5… ถูกต้อง) — `WORLD_DUMP_F0` env dump เป็น diagnostic.

### 3.11 server chunk cache เปิดจริง + app pre-render ✅ (2026-08-08)
- `render_service.rs` คำนวณ `cache_dir` แต่ส่ง `cache = None` (dead code — comment
  ล้าสมัย) → เปิด `RenderCache::new_in_dir(cache_dir, 64MB)` → device render
  6.6s → **20-37ms** (disk hit ข้าม restart)
- app: pre-render ตอน load (initState post-frame → `_scheduleReRender`) + engine
  switch → rerender (`_audioDirty` — เล่น audio เก่าหลัง edit ไม่ได้อีก) — device
  log ยืนยัน: edit → full re-render (5-6s) → play → 20-30ms cache hit

## 4. สถานะสุดท้าย (metrics 2026-08-08)

| Metric | v1↔r1 | v2↔r2 |
|---|---|---|
| RMS ratio s/l/3/la | 1.01 / 1.06 / 1.13 / 1.04 | **1.00 / 1.03 / 0.94 / 1.02** |
| mel powers (10 frames) | — | **identical กับ C#** |
| f0 / timing / phoneme | identical | identical |
| autoGain (s) | 0.56 | 0.56 (ตรง C#) |

**ยังไม่ bit-identical** (correlation ต่ำ ~0.1-0.2 — WORLD/neural micro-detail,
phase, resampler detail) — ฟังแยกไม่ออกในระดับ POC; โครงสร้าง/ความดัง/pitch ตรง.

## 5. Golden harness หมายเหตุ (build/run)

- **ใช้ `ref-openutau-clean/`** (copy ของ ref — ลบ `<data>` ออกจาก 4 resx:
  resgen `/useSourcePath` เขียน temp ใน source dir → MSB3554 "Access denied"
  กับ binary data เช่น tiny.onnx; ref เดิม build ไม่ผ่าน)
- build: `OPENUTAU_REF=…/ref-openutau-clean ~/dotnet/dotnet build -c Release -m:1`
  (VBCSCompiler ค้างทำให้ deterministic dll ไม่มี code ใหม่ — `pkill VBCSCompiler`
  + ลบ obj/bin ทั้ง 2 โปรเจกต์ ก่อน rebuild)
- run: `WORLDLINE_RENDERER=1|2 run.sh phrase <out.wav> <ustx>` — **ลบ
  /tmp/golden-renderer-cache ก่อน** (cache hit ข้าม render จริง)
- `.so` ใน bin ต้อง copy จาก native/build/build-linux (md5 ตรง r2)
- mel.onnx + pc-nsf-hifigan ต้องอยู่ (bin/mel.onnx + golden-renderer-data)

## 6. Device (Android)

- jniLibs: libsynthserver.so (Rust server — r2 wired), libworldline.so,
  libmixerfx.so; assets/engine/models/{mel.onnx, pc_nsf_hifigan.onnx}
- server args: `--mel-onnx --vocoder-onnx` (ทั้งคู่ → R2; ขาด → R1)
- ตรวจสอบ APK bundles stripped fresh .so (llvm-strip เปลี่ยน md5 — เทียบกับ
  stripped artifact ไม่ใช่ raw)

## 7. Verification

- 430 workspace tests 0 FAILED (สด)
- ad-hoc scripts (hermes-verify-*) รันแล้วลบ — final: 5/5 (auto_gain sign,
  diagnostics env-guarded, v1↔r1, v2↔r2, workspace)
