# world-source-check — ตรวจ "ต้นทาง" (WORLD analysis) เทียบ C++ `.so`

> task: `task-8` (ทีม r2-wasm) · วันที่: 2026-09-23 · สถานะ: **ตรวจเสร็จ**
> คำถามตั้งต้นของผู้ใช้: *"ผล AI model ดูพัง อาจเพราะ input/source พังอยู่แล้ว"*
> เครื่องมือ: `native/tools/world-oracle` (`analysis` / `pyin` / `phrase`) · เกณฑ์: ทุกตัวเลขมาจากคำสั่งที่รันจริง

## 0. คำตอบสั้น (TL;DR)

| คำถาม | คำตอบ | หลักฐาน |
|---|---|---|
| f0 ของเรา (pYIN) ตรงกับ `.so` ไหม | **ตรงทุกเฟรม** — voicing agreement 100%, mean/max \|cents\| = 0.00 | §2.1 |
| sp_env (CheapTrick) ตรงไหม | **ตรงถึง ~1e-4 dB ในย่านที่มีพลังงาน** (floor-independent metric) | §2.2 + §3 |
| ap (D4C) ตรงไหม | **ตรงระดับ 1e-3** (MAE 1.2e-4 … 1.1e-3, max 0.18 ที่ bin เดียว) | §2.3 |
| ต้นทางน่าเชื่อถือไหม | **น่าเชื่อถือ** — ทั้ง 3 stage ของ Rust port ตรงกับ C++ reference | §5 |
| แล้วทำไม "ผล AI ดูพัง"? | **งานนี้ตอบไม่ได้** — และที่สำคัญ: **R2 chain ไม่ได้ใช้ Rust port ของเราเลย** มันเรียก `.so` ตรง ๆ สำหรับ f0/sp_env/ap ⇒ อาการพังไม่ได้มาจาก feature drift ของเรา | §4.1 |

> **ข้อเท็จจริงที่สำคัญที่สุดของงานนี้:** ในเส้นทาง R2 (`--engine r2`) ตัว feature ที่ป้อน `mel.onnx`
> ถูกคำนวณโดย `worldline_sys::Analysis` ซึ่งคือ **`libworldline.so` (C++) เอง** —
> `native/crates/worldline-plugin/src/r2.rs:40,59,69,113` ⇒ "source" ของโมเดลในเส้นทางนี้ *คือ reference*
> ส่วน Rust port (`world-rs`) ถูกใช้ในเส้นทาง **wasm/browser** (`native/crates/engine-wasm/src/lib.rs`)
> ซึ่งงานนี้ยืนยันว่าตรงกับ `.so` เช่นกัน

## 1. วิธีรัน

```bash
cd /media/DATA/project/android-voice-synth
cargo build --release -p world-oracle -p synth-cli      # ใช้ binary ที่สร้างใหม่ (2026-09-23)

SO=native/build/build-linux/libworldline.so
OR=native/target/release/world-oracle

# ต่อ 1 ไฟล์เสียง: f0 (pYIN) และ sp_env+ap (CheapTrick/D4C) — .so vs world-rs บน input เดียวกัน
$OR pyin     <input.wav> --so $SO --json
$OR analysis <input.wav> --so $SO --json

# ทั้งสาย: phrase orchestration + synthesis (.so vs world-rs)
$OR phrase test/golden/scale-demo-app.ustx test/golden/teto-english/library --so $SO --json
$OR phrase test/golden/sakura-test.ustx    test/golden/teto-english/library --so $SO --json
```

| สิ่งที่ใช้ | ค่า |
|---|---|
| `.so` | `native/build/build-linux/libworldline.so` · 343,128 B · sha256 `e326e55d42afe17e82e01ebedbcfac31134efc2e29fca5466adc5b2805dc7e53` |
| oracle | build ใหม่จาก source นี้ (`cargo build --release -p world-oracle`) |
| voicebank | `test/golden/teto-english/library` |
| input ที่ทดสอบ | voicebank sample, เพลง render (`demo-song.wav`), teacher wav, และ output ของ R2 เอง |
| log ดิบ | `/tmp/world-source-check.log` |

## 2. ผลราย stage

### 2.1 Stage f0 — pYIN (`.so` `F0(method=2)` vs `world_rs::pyin`)

| input | เฟรม (so/rs) | voiced so | voiced rs | voicing agreement | mean \|cents\| | max \|cents\| |
|---|---|---|---|---|---|---|
| `_zau+_zau+_z-.wav` (voicebank) | 133 / 133 | 80 | 80 | **100.0%** | **0.00** | **0.00** |
| `reference/demo-song.wav` | 515 / 515 | 184 | 184 | **100.0%** | **0.00** | **0.00** |
| `bigset/teacher-wavs-real/00732.wav` | 29 / 29 | 27 | 27 | **100.0%** | **0.00** | **0.00** |
| R2 output (`/tmp/r2-scale.wav`) | 1247 / 1247 | 1166 | 1166 | **100.0%** | **0.00** | **0.00** |

⇒ f0 ที่ Rust port สร้าง **เหมือน `.so` ทุกเฟรมที่เทียบ** (ทั้งการตัดสิน voiced/unvoiced และค่า pitch)
· หมายเหตุ: `pyin` mode เทียบ `so[i]` กับ `raw[i+1]` ตาม comment ในโค้ด (`.so` ตัดเฟรมแรกทิ้ง)

### 2.2 Stage sp_env — CheapTrick (log-spectrum)

| input | เฟรม | corr | dB MAE (ทั้งก้อน) | dB max (ทั้งก้อน) | **dB MAE (เฉพาะ so>1e-8)** | **dB max (เฉพาะ so>1e-8)** | entries ที่นับ |
|---|---|---|---|---|---|---|---|
| voicebank sample | 135 | 1.000000000 | 0.000001 | 0.000142 | **0.000000** | **0.000008** | 121,501 / 138,375 |
| `demo-song.wav` | 517 | 0.994847197 | **50.414547** | **91.612011** | **0.000000** | **0.000010** | 184,821 / 529,925 |
| teacher wav | 31 | 0.999740650 | 0.167959 | 17.639640 | **0.000000** | **0.000007** | 27,673 / 31,775 |
| R2 output | 1249 | 0.999986731 | 0.004530 | 25.661830 | **0.000000** | **0.000181** | 1,161,374 / 1,280,225 |

linear-domain relative error (`sum|so-rs| / sum|so|`) = **0.000000000** ทุกไฟล์

⚠️ **ตัวเลข dB MAE 50.4 dB ของ `demo-song.wav` ไม่ได้แปลว่า sp_env พัง** — ดู §3

### 2.3 Stage ap — D4C (aperiodicity 0..1)

| input | เฟรม | corr | MAE | max |
|---|---|---|---|---|
| voicebank sample | 135 | 0.999941527 | 0.000837 | 0.050677 |
| `demo-song.wav` | 517 | 0.999989061 | 0.000171 | 0.047046 |
| teacher wav | 31 | 0.999999299 | 0.000122 | 0.002892 |
| R2 output | 1249 | 0.999857656 | 0.001054 | 0.179655 |

⇒ ap ต่างกันระดับ 1e-4…1e-3 โดยเฉลี่ย; ค่าสูงสุด 0.18 (บนสเกล 0..1) เกิดที่บาง bin
· **ข้อสังเกต (ยังไม่สรุป):** ควรมีคนดูว่า max 0.18 เป็น outlier bin เดียวหรือเป็นรูปแบบระบบ — งานนี้ยังไม่ได้เจาะ

### 2.4 Stage phrase — ทั้งสาย (orchestration + synthesis) `.so` vs `world-rs`

| โปรเจกต์ | phrase | samples so/rs | len diff | corr | max\|diff\| | rms(diff) |
|---|---|---|---|---|---|---|
| `scale-demo-app.ustx` | 1 | 639,489 / 639,489 | 0 | 0.999999985 | 5.45e-4 | 3.51e-5 (**-89.1 dBFS**) |
| `sakura-test.ustx` (concat) | 2 | 152,066 / 152,066 | 0 | 0.999999923 | 2.39e-3 | 8.15e-5 (**-81.8 dBFS**) |

⇒ ปลายทาง PCM ของ Rust port ต่างจาก `.so` ที่ระดับ -82…-89 dBFS (ต่ำกว่า noise floor ของ 16-bit มาก)

## 3. กับดักของตัวเลข dB (ต้องอ่านก่อนตีความ)

`analysis` รายงาน dB MAE แบบ "ทุก entry" ซึ่ง **ถูกครอบงำโดย bin ที่เกือบเงียบ**:

- `demo-song.wav`: mean level ฝั่ง `.so` = **-237.98 dB** (แปลว่า bins ส่วนใหญ่เป็น digital silence ~1e-12)
  ⇒ ค่าที่ถูก log แล้วเล็กมาก; ผลต่างสัมพัทธ์นิดเดียวกลายเป็น dB มหาศาล (dB max 91.6)
- 345,104 จาก 529,925 entries (65%) มีค่า `.so` ต่ำกว่า 1e-8 (floor) — ตัดออกแล้ว dB max เหลือ **1e-5 dB**
- มาตรวัดที่ไม่ขึ้นกับ floor: `sum|so-rs|/sum|so|` = **0.000000000** และ dB max เฉพาะย่านดัง = **≤ 1.8e-4 dB**
- voicebank sample ไม่มีช่วงเงียบ ⇒ dB MAE ทั้งก้อนก็ยังเป็น 1e-6 อยู่แล้ว (สอดคล้องกัน)

⇒ สรุปที่ถูกคือ: **sp_env ตรงกันในย่านที่มีพลังงาน** ส่วน dB MAE ทั้งก้อนของไฟล์ที่มี silence ไม่ใช่ตัวชี้วัดที่ใช้ได้

**การแก้เครื่องมือ:** เพิ่ม metric แบบไม่ขึ้นกับ floor ใน `native/tools/world-oracle/src/main.rs`
(`log_spectrum_stats` → `lin_rel_mae`, `db_mae_loud`, `db_max_loud`, `loud_kept/loud_total`)
เป็น **additive** — key เดิมของ JSON ไม่เปลี่ยน, เพิ่มบรรทัด/`{"stage":"analysis_extra",...}` ต่อท้ายเท่านั้น

## 4. R2 chain — mel จริงที่ได้

### 4.1 source ของ R2 คือ `.so` ไม่ใช่ Rust port

`native/crates/worldline-plugin/src/r2.rs` (R2 feature extraction) เรียก:

| บรรทัด | อะไร | มาจาก |
|---|---|---|
| `:59`, `:69` | f0 grid / pYIN (`analysis.f0`) | `worldline_sys::Analysis` = **`.so`** |
| `:113` | sp_env + ap (`analysis.analysis_f0_in`) | `worldline_sys::Analysis` = **`.so`** |

⇒ input ของ `mel.onnx` ในเส้นทาง R2 ถูกสร้างโดย **C++ reference เอง** — ถ้าเสียงที่ได้ดูพัง
ต้องมองที่ `mel.onnx` / vocoder / การประกอบ phrase ไม่ใช่ที่ WORLD feature ของเรา
(Rust port `world-rs` ถูกใช้ในเส้นทาง wasm: `native/crates/engine-wasm/src/lib.rs`)

### 4.2 คำสั่ง + ผลรัน R2

```bash
cd native
WORLD_DUMP_F0=1 WORLD_DUMP_MEL=1 WORLD_DUMP_MEL_FILE=/tmp/r2-scale-mel.bin \
  ./target/release/synth-cli render \
    --project ../test/golden/scale-demo-app.ustx \
    --voicebank ../test/golden/teto-english/library \
    --so build/build-linux/libworldline.so --engine r2 \
    --mel-onnx ../models/r2/mel.onnx \
    --vocoder-onnx ../models/r2/pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx \
    --out /tmp/r2-scale.wav --verbose
# exit 0 · rendered 4 phrase(s), 0 skipped · samples: 639443 (14499.8 ms) · peak: 0.9075
```

**stat ของ dump** (`/tmp/r2-scale-mel.bin`, รูปแบบ `[u32 total][f32 f0 × total][f32 mel × total × 128]` จาก hook ของ T5):

| รายการ | ค่า |
|---|---|
| ไฟล์ | 222,916 B (= 4 + 432×4 + 432×128×4 ✓) |
| จำนวนเฟรม | **432** (5.02 s @ 86.13 fps) |
| f0 | voiced **431/432 (99.8%)** · ช่วง 493.88–1046.50 Hz · mean 764.86 Hz · nonFinite 0 |
| mel | min -12.5508 · max 3.1929 · mean -4.2656 · rms 4.7275 · **nonFinite 0** · distinct f32 = 50,442 |
| frame mean-power | min -10.03 · max -3.53 · mean -4.27 (สอดคล้องกับ `[mel-dump] mean=22.348841 max=102.099701` ที่เป็นค่าเฉลี่ยกำลังสอง) |
| |frame-to-frame delta| ของ frame power | 0.3228 → mel เปลี่ยนตามเวลา ไม่ใช่ค่าคงที่ |
| WAV ที่ได้ | 639,443 samples (14.50 s) · peak 0.9075 · rms 0.2874 · 13,179 ค่าไม่ซ้ำใน 20k samples แรก |

⇒ mel ที่ป้อน vocoder เป็นค่าจริง finite และไม่คงที่ (บทสรุปเชิงคุณภาพของเสียงยังต้องเทียบ reference — งาน Lead)

**ข้อจำกัดของ dump:** hook เขียนทับไฟล์เดิม **ต่อ phrase** ⇒ ไฟล์สุดท้ายเหลือเฉพาะ phrase สุดท้าย (432 เฟรม)
และ dump มีแค่ **f0 + mel** ไม่มี sp_env/ap (ไม่มี env hook สำหรับสองตัวนั้น; การเพิ่มต้องแก้
`worldline-plugin` ซึ่งอยู่นอก write scope ของ task นี้)

## 5. สรุป: "ต้นทางน่าเชื่อถือไหม"

| ข้อ | สรุป |
|---|---|
| f0 (pYIN) ของ Rust port | ✅ **ตรงกับ `.so` ทุกเฟรม** (100% voicing agreement, 0.00 cents, 4 ไฟล์ทดสอบ) |
| sp_env (CheapTrick) | ✅ **ตรงในย่านที่มีพลังงาน** (dB max loud ≤ 1.8e-4, linear rel-err ≈ 0) |
| ap (D4C) | ✅ ตรงระดับ MAE ≤ 1.1e-3 (มี max 0.18 ที่บาง bin — ยังไม่ได้เจาะว่าเป็น outlier หรือระบบ) |
| ทั้งสาย `.so` vs `world-rs` (PCM) | ✅ ต่างกัน -82…-89 dBFS |
| R2 chain ใช้ feature จากไหน | ℹ️ ใช้ **`.so` ตรง ๆ** ⇒ ไม่มี drift จาก Rust port ในเส้นทางนี้ |
| R2 mel ที่ได้ | ✅ finite ทั้งหมด, voiced 99.8%, ไม่คงที่, ความยาว/peak ปกติ |

**คำตอบต่อผู้ใช้:** อาการ "ผล AI ดูพัง" **ไม่สามารถอธิบายได้ด้วย WORLD analysis stage ของเรา** —
stage นี้ (ทั้งในเส้นทาง R2 ที่ใช้ `.so` และใน Rust port ที่ใช้บน wasm) ตรงกับ C++ reference อย่างแน่นหนา

### สิ่งที่งานนี้ "ไม่ได้" พิสูจน์ (ห้ามสรุปเกิน)

1. **ไม่ได้พิสูจน์ว่าโมเดลถูก** — พิสูจน์แค่ว่า input/feature ไม่เพี้ยน
2. **ไม่ได้เทียบกับ OpenUtau ตัวจริง** (golden-renderer / `WORLDLINE_RENDERER=2`) — เป็นงานของ Lead
3. **ไม่ได้เทียบ mel ที่ได้กับ mel ของ reference** ⇒ ยังชี้ไม่ได้ว่า `mel.onnx` หรือ vocoder เป็นตัวปัญหา
4. **ap max 0.18** ยังไม่ได้แยกว่าเป็น outlier ราย bin หรือรูปแบบระบบ
5. ทดสอบบน 4 input / 2 โปรเจกต์บนเครื่องนี้ — ไม่ได้ครอบคลุมทุก phoneme/ทุก voicebank

## 6. ไฟล์ที่แตะ

| ไฟล์ | สถานะ |
|---|---|
| `docs/planning/world-source-check.md` | เอกสารนี้ (ใหม่) |
| `native/tools/world-oracle/src/main.rs` | เพิ่ม metric แบบไม่ขึ้นกับ floor (`lin_rel_mae`, `db_mae_loud`, `db_max_loud`, `loud_kept/loud_total`) — additive, ไม่เปลี่ยน key เดิมของ JSON |

ไม่ได้แตะ: `models/**`, `web/**`, `native/crates/moe-train/**`, `native/crates/moe-wasm/**`, เอกสารของ T1/T2/T4/T5/T6

ไฟล์ชั่วคราว (ไม่ใช่ deliverable): `/tmp/world-source-check.log`, `/tmp/r2-scale-mel.bin`, `/tmp/r2-scale.wav`, `/tmp/r2-scale.log`
