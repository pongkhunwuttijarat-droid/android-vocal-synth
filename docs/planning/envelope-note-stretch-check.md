# envelope-note-stretch-check — "note stretch" พังจริงไหม + golden demo ทำอะไรเพิ่ม (T13)

> task: `task-13` (r2-wasm) · วันที่: 2026-09-23 · สถานะ: **ตรวจเสร็จ — สรุป: stretch ไม่พัง**
> คำถามผู้ใช้: "เหมือน note stretch ของเราไม่เป็น" (ต่อจาก T9 ที่แก้ onset/duration ไปแล้ว)
> เกณฑ์ตัดสิน: เทียบกับ **golden phrase (raw = `WorldlineRenderer.RenderPhrase` ตรง ๆ)** ซึ่ง deterministic
> ไม่ใช่ golden **demo** (mixdown) เพราะ reference เองสองโหมดนี้ก็ไม่ตรงกัน (ยืนยันใน §3)

## 0. คำตอบ

| สมมติฐาน | ผล | หลักฐาน |
|---|---|---|
| (ก) คนละ alias | **จริง แต่เฉพาะเส้นทาง `--phonemizer japanese`** (`- sa` vs `sa`) — เส้นทาง universal/default ใช้ `sa` เหมือน golden | §2 |
| (ข) mixdown/demo mode | **จริง — และเป็นคำอธิบายของ "envelope ไม่ตรง" ทั้งหมด** เป็นพฤติกรรมของ **OpenUtau เอง** (raw→demo ของเขาไม่ตรงกันเอง) | §3 |
| (ค) **stretch/region math พัง** | **ไม่จริง (refuted)** — ที่ 1x และ 2x ความยาวโน้ต ผลลัพธ์ของเราเท่ากับ golden phrase ที่ `corr = 1.000000` และ envelope Pearson = **1.0000** | §1 |

**สรุปให้ผู้ใช้:** เสียงที่ได้จาก pipeline ของเรา "ยืดโน้ต" เหมือน OpenUtau ทุกจุดที่วัดได้ (ทั้งโน้ต 1.5 วิ และ 3.0 วิ)
— สิ่งที่ผู้ใช้ได้ยินว่าต่างคือ **envelope ของโหมด demo/mixdown ของ OpenUtau** ซึ่งตัว OpenUtau เองก็ไม่ได้ใช้ตอน render phrase

## 1. ตัดสิน "stretch" ด้วย golden phrase (1x และ 2x)

**วิธี:** เทียบ output ของ pipeline เรา (`--engine r1-rs --phonemizer universal`, alias `sa` เหมือน golden)
กับ golden phrase ที่ render ตรงด้วย `WORLDLINE_RENDERER=1 ./run.sh phrase` — ที่ความยาวโน้ต 1440 และ 2880 ticks
(สร้าง `/tmp/one-note-2880.ustx` จาก `test/golden/one-note.ustx` โดยแก้ `duration: 1440 → 2880`)

```bash
cd test/golden/golden-renderer
export TETO_DIR=/media/DATA/project/android-voice-synth/test/golden/teto-english   # Program.cs hardcode /home/seal/...
WORLDLINE_RENDERER=1 ./run.sh phrase /tmp/g-note2880.wav /tmp/one-note-2880.ustx
```

| | golden phrase (raw) | ของเรา (pipeline) | ต่าง |
|---|---|---|---|
| โน้ต 1x (1440 ticks) | 72,193 samples (1637.0 ms) | 66,150 (1500.0 ms) | 6,043 = leading 5,982 + 61 |
| โน้ต 2x (2880 ticks) | 138,241 samples (3134.7 ms) | 132,259 (2999.1 ms) | 5,982 = leading |

ความเท่ากันของ **เนื้อเสียง** (ทั้งไฟล์, ที่ offset ของ leading = 5,982 samples):

| การเทียบ | waveform corr (ทั้งไฟล์) | envelope Pearson |
|---|---|---|
| ของเรา 1x vs golden 1x | **1.000000** | **1.0000** |
| ของเรา 2x vs golden 2x | **1.000000** | **1.0000** |

⇒ **โน้ตที่ยืดเป็น 2 เท่า ยังตรงกับ reference เป๊ะ** และ offset คงที่ (ไม่มีความคลาดเคลื่อนสะสม)
⇒ region math (consonant คงที่ + vowel ยืด), oto offset/cutoff และตัว resampler ของเรา **ไม่พัง**

หลักฐานเสริม: per-window (100 ms) local alignment ของเคส 2x ทุกหน้าต่างให้ corr 1.0000 (และที่ 1x ให้ lag คงที่ +5,981)

**หลักฐานว่า consonant ไม่ถูกยืดไปกับโน้ต** — 50 ms RMS ของ golden phrase ที่ 1x กับ 2x ช่วงต้น *ใกล้กันมาก*
(ช่วง consonant/attack ขึ้นกับ oto ไม่ขึ้นกับความยาวโน้ต) แล้วค่อย diverge กันชัดหลัง ~200 ms:

| block (50 ms) | golden 1x | golden 2x | delta |
|---|---|---|---|
| 0 (0–50) | 0.022987 | 0.023009 | **+0.000022** |
| 1 (50–100) | 0.027722 | 0.027745 | **+0.000023** |
| 2 (100–150) | 0.106867 | 0.106885 | **+0.000018** |
| 3 (150–200) | 0.237755 | 0.237922 | **+0.000167** |
| 4 (200–250) | 0.209711 | 0.213912 | **+0.004201** |
| 5–9 | … | … | +0.0027 … **−0.0107** (diverge ต่อเนื่อง) |

เกณฑ์ที่ใช้สรุป (แก้จาก *"เท่ากันเป๊ะ"* ซึ่ง **ไม่ถูก**): **4 บล็อกแรก (0–200 ms) ต่าง ≤ 1.7e-4 แม้โน้ตยาวขึ้น 2 เท่า**
⇒ consonant/attack คงที่ ไม่ถูกยืดไปกับโน้ต · ตั้งแต่บล็อก 4 (200 ms) เริ่มต่าง 4.2e-3 แล้ว diverge ต่อ (บล็อก 9 ต่าง −0.0107)

(ตัวเลขเดิมที่พิมพ์ 3 ตำแหน่ง `0.023 0.028 0.107 0.238 0.214/0.210` ทำให้อ่านได้ว่าเท่ากันเป๊ะ — ค่าจริงต่างกันที่หลัก 3–4)
ผู้ตรวจ (task-14) วัดซ้ำได้ตรงกันทุกหลัก: บล็อก 0–3 ต่าง 2e-5..1.7e-4 · บล็อก 4 ต่าง 4.2e-3 (0.209711 vs 0.213912)

> ⚠️ **กับดักการหา lag (บันทึกตามที่ผู้ตรวจพบ):** การหา lag จากสัญญาณที่ decimate อาจได้ค่าผิด —
> ผู้ตรวจรายงานว่าครั้งแรกได้ **6,784** แต่ full-resolution scan ได้ **5,982** (ค่าที่ถูก, corr 1.000000)
> อาการเดียวกันเกิดในฉบับแรกของเอกสารนี้: per-window search (stride 8, ช่วง ±1,764 รอบ 5,983) รายงาน **+4,229 corr 1.0000**
> ซึ่งเป็น peak ปลอม (aliasing ของสัญญาณกึ่งคาบ — มีหลาย lag ที่ corr สูงใกล้ 1)
> ⇒ **ยืนยัน lag ด้วย whole-file correlation แบบ stride 1 เสมอ** (เอกสารนี้ใช้ 5,982: corr 0.99999998 ที่ 1x และ 0.99999999 ที่ 2x)

## 2. สมมติฐาน (ก): alias

- `--phonemizer universal` / default → alias **`sa`** (offset 736.336 ms, preutter 135.666) = alias เดียวกับ golden ทุกโหมด
- `--phonemizer japanese` (หลัง T9) → alias **`- sa`** (offset 80.0, preutter 201.665) ตาม reference `JapaneseVCVPhonemizer`
  ⇒ sample region คนละช่วง ⇒ timbre/envelope คนละแบบ (แต่ onset/duration ถูกแล้วหลัง T9)
- **ผลต่อ task นี้:** การเทียบ "ของเรา vs golden demo" ต้องใช้เส้นทาง universal/default เท่านั้น

## 3. สมมติฐาน (ข): golden demo ทำอะไรเพิ่มจาก raw

> **NOTE (เพิ่ม 2026-09-23 หลังงาน T15–T17):** ข้อสรุปใน §3.1–3.3 วัดบนไฟล์ demo **ก่อนที่ harness จะถูกแก้** จึงใช้เป็นข้อเท็จจริงปัจจุบันไม่ได้ทั้งหมด:
> - สาเหตุจริงของ "demo ≠ raw × gain / residual 98%" คือ **บั๊ก harness** (`Program.cs` demo loop ไม่ clear buffer →
>   stale audio ในช่องว่าง + สะสม) — ดู `phrase-join-gap.md` (T15)
> - หลัง Lead แก้ `Array.Clear`: **demo = raw phrase × 0.7071 เป๊ะ (corr 1.000000)** = pan law ของ `Fader`
>   (`PanToChannelVolumes(0) = cos π/4`) — ดู `gain-level-parity.md` (T16)
> - ไฟล์ demo ที่ติดป้าย `-r2-` **เป็น r1** (demo ไม่สลับ renderer) — แก้แล้วใน `harness-demo-renderer.md` (T17)
>
> สาระของ **§1 (stretch ไม่พัง)** ซึ่งเป็นข้อสรุปหลักของเอกสารนี้ **ยังถูกต้องและถูกยืนยันซ้ำโดยผู้ตรวจ (task-14)**

### 3.1 ข้อมูลไฟล์ (ต้องใช้คู่ที่ถูกต้อง)

| ไฟล์ | ที่มา | samples | rms |
|---|---|---|---|
| `/tmp/g-note1440.wav` | golden **phrase** `WORLDLINE_RENDERER=1` | 72,193 (mono) | −12.8 dB |
| `/tmp/g-note-r2.wav` | golden **phrase** `WORLDLINE_RENDERER=2` | 72,704 (mono) | **−9.6 dB (+3.2 dB)** |
| `/tmp/g-demo.wav` = `out/ab/golden-openutau-*-note.wav` | golden **demo** (mixdown) | 66,211 (stereo) | −12.2 dB |

⚠️ **แก้ตัวเลขของ Lead:** คู่ที่ควรเทียบคือ **phrase r1 → demo** = **+0.6 dB** (ไม่ใช่ −3.02 dB)
เพราะ −3.02 dB มาจากการเทียบ `g-note-r2` (คนละ renderer) กับ `g-demo` ⇒ ไม่ใช่ apples-to-apples
และความยาว 72,704 vs 72,193 (ต่าง 511 samples) ก็ยืนยันว่าเป็นคนละเส้นทาง render

### 3.2 demo กับ raw ต่างกันแค่ gain ไหม → **ไม่ใช่**

fit แบบ pure-gain ต่อหน้าต่าง 50 ms (`demo_w ≈ a·phrase_w` ที่ align offset 5,982) บนคู่ที่ถูกต้อง:

```text
gains  : +0.72 +0.95 +0.61 +0.35 +0.12 -0.23 +0.14 +0.75 +0.71 +0.56 +0.43 -0.08 -0.14 +0.43 +0.62 +0.65 ...
resid% :  0.68  0.61  0.70  0.70  0.78  1.15  1.78  0.65  0.71  0.73  0.73  0.93  1.58  1.27  0.82  0.74 ...
gain min -0.23  max +0.95  mean +0.394   |  residual mean 0.976 (98% ของพลังงาน demo)  max 2.097
```

⇒ เหลือ residual ~98% ของพลังงาน และค่า gain ที่ fit ออกมาติดลบ ⇒ **ไม่ใช่ raw × gain**
(global waveform corr ที่ align ดีที่สุดก็ได้แค่ ~0.38–0.52) ⇒ demo mode มี processing/time-domain ต่างจาก raw จริง

### 3.3 demo envelope แกว่ง ส่วน raw เรียบ

| | 50 ms RMS (20 บล็อกแรก) | autocorrelation ของ envelope |
|---|---|---|
| golden **raw phrase** | 0.023 0.028 0.107 0.238 0.210 0.206 0.208 0.212 0.215 ... (เรียบ) | ซีกลดแบบ monotonic |
| golden **demo** | 0.168 0.263 0.295 0.295 0.268 0.179 0.120 0.209 0.270 0.300 ... | **lag 3–4 บล็อก (150–200 ms) = −0.74/−0.70 · lag 6–7 (300–350 ms) = +0.58/+0.68** ⇒ แกว่งคาบ ~325 ms |
| ของเรา (universal, หลัง T9) | 0.230 0.213 0.205 0.208 0.211 0.214 0.218 0.225 0.232 ... (เรียบ เหมือน raw) | ซีกลดแบบ monotonic |

⇒ ของเราเหมือน **raw** ไม่เหมือน demo; ความต่างที่เหลือทั้งหมดเป็นของ **mixdown ฝั่ง OpenUtau**

### 3.4 demo envelope ที่ OpenUtau ประกาศเอง (จาก diagnostics ของ golden-renderer)

```text
$ TETO_DIR=... ./run.sh demo /tmp/g-demo-repro.wav test/golden/one-note.ustx
  phrase: posMs=0.0 leadingMs=135.7 durMs=1500.0 phones=sa
    phone sa: posMs=0.0 leadingMs=135.7 durMs=1500.0 env=[-136:0.00, -131:100.00, 0:100.00, 1465:100.00, 1500:0.00]
Wrote /tmp/g-demo-repro.wav: 66211 frames (1501.4 ms stereo), peak 0.6422
$ cmp /tmp/g-demo-repro.wav /tmp/g-demo.wav   ->  IDENTICAL
```

- ผม reproduce `g-demo.wav` ของ Lead ได้ **byte-identical** ⇒ demo path deterministic และคำอธิบายนี้ใช้ได้กับไฟล์ที่ Lead ใช้วัด
- envelope ที่ OpenUtau ใช้กับโน้ตนี้คือ **trapezoid fade in 5 ms / fade out 35 ms** เท่านั้น
  ⇒ **ไม่ใช่** สาเหตุของ envelope ที่แกว่งคาบ ~325 ms ⇒ แหล่งที่มาอยู่ใน `RenderEngine.RenderMixdown` (chunked re-render / playback stage)

**สิ่งที่ยังไม่ได้ระบุ (ไม่เดา):** กลไกที่ทำให้ demo แกว่งคาบ ~325 ms ยังไม่ทราบจากหลักฐานชุดนี้
สิ่งที่ตัดออกแล้ว: (1) ไม่ใช่ pure gain (§3.2) (2) ไม่ใช่ alias/region (§1) (3) ไม่ใช่การเลือก renderer ตาม `WORLDLINE_RENDERER`
เพราะ demo r1 กับ r2 ที่ Lead เก็บไว้ **เหมือนกันเป๊ะ** (corr 1.000000) ทั้งที่ phrase mode ให้ไฟล์ต่างกัน
⇒ ต้องอ่าน `OpenUtau.Core/Render/RenderEngine.cs` + `Playback` ต่อ (ยังไม่ได้ทำในงานนี้)

## 4. เกณฑ์ที่ควรใช้ต่อไป

| ใช้เทียบกับ | เหมาะไหม | เหตุผล |
|---|---|---|
| golden **phrase** (`WORLDLINE_RENDERER=1`) | ✅ **ใช่** | deterministic, ตรงกับ engine v1 ของเรา, corr 1.000000 (1x/2x) |
| golden **demo** (mixdown) | ⚠️ ระวัง | มี processing ของ mixdown ที่ตัว OpenUtau เองก็ไม่ได้ใช้ตอน render phrase (raw→demo corr ~0.38–0.52) |
| golden phrase `WORLDLINE_RENDERER=2` | ❌ ไม่ใช่ target ของ v1 | คนละไฟล์: 72,704 samples, rms +3.2 dB เทียบ r1 |

## 5. ข้อจำกัด / งานค้าง

1. ห่าง 61 samples (1.4 ms) ที่ **ปลายเสียง** เมื่อเทียบ golden phrase 1x (ของเราสั้นกว่า) — ไม่กระทบ onset/envelope
   (corr ที่ align แล้ว = 1.000000); ยังไม่ได้ไล่ว่ามาจากการปัดเศษท้ายเฟรมของใคร
2. กลไกของ demo modulation (§3.4) ยังไม่ระบุ — ต้องอ่านโค้ด OpenUtau ต่อ
3. `Program.cs` hardcode `TETO_DIR=/home/seal/project/...` (override ด้วย env `TETO_DIR`) — และ repo นี้อยู่ที่ `/media/DATA`
   ⇒ คำสั่งในเอกสารต้อง export `TETO_DIR` ก่อนเสมอ (ไม่งั้นได้ "no voicebank found")
4. **ไม่ได้แก้โค้ดใด ๆ** ในงานนี้ เพราะไม่พบว่า stretch/region math พัง (ตามเกณฑ์ "ถ้าพังจริงให้แก้")

## 6. ไฟล์ที่แตะ

| ไฟล์ | สถานะ |
|---|---|
| `docs/planning/envelope-note-stretch-check.md` | เอกสารนี้ (ใหม่) |

ไม่ได้แตะโค้ดเลย — โดยเฉพาะ `native/crates/runtime/src/mixer.rs` (T9), `native/crates/phonemizer/**` (T12),
`native/tools/synth-cli/src/pipeline.rs` และเทสต์ของ T9 (ระหว่างที่ poc-verifier ตรวจ task-10)

ไฟล์ชั่วคราวที่ใช้ (ไม่ใช่ deliverable): `/tmp/one-note-2880.ustx`, `/tmp/g-note1440.wav`, `/tmp/g-note2880.wav`,
`/tmp/our-note2880.wav`, `/tmp/g-demo-repro.wav`, สคริปต์วัดใน `/tmp/r2w-*.py`
