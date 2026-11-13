# gain-level-parity — ระดับเสียงต่าง 2–7 dB มาจากฝั่งไหน (T16)

> task: `task-16` (r2-wasm) · วันที่: 2026-09-23 · สถานะ: **ได้คำตอบ — (ข) by-design/representation ไม่มี gain ของเราที่ผิด**
> บริบท: หลัง Lead แก้ harness (`Array.Clear`) โครงสร้างตรงกันหมด เหลือ "ของเราดังกว่า 2–7 dB"

## 0. คำตอบ

**ระดับเสียงของเราตรงกับ reference เป๊ะ เมื่อเทียบแบบ like-for-like (phrase, renderer เดียวกัน):**

| คู่ที่ถูกต้อง | rms (ไฟล์เต็ม) | **peak** | corr | waveform gain |
|---|---|---|---|---|
| ของเรา r1 note (universal) | −12.40 dB | **0.6037** | **1.000000** | **+0.00 dB** |
| golden r1 **phrase** (mono) | −12.77 dB (รวม pre-roll) | **0.6037** | — | — |
| ของเรา r2 note (universal) | −9.19 dB | **0.7889** | 0.982 | −0.16 dB |
| golden r2 **phrase** (mono) | −9.60 dB (รวม pre-roll) | **0.7888** | — | — |

peak ตรงกันถึงทศนิยม 4 ตำแหน่ง (r1) และต่าง 0.001 dB (r2) ⇒ **ไม่มี gain error**
(rms ต่างกันเพราะไฟล์ golden เป็น *phrase ดิบ* ที่รวม pre-roll 135.7 ms ซึ่งเงียบ; เทียบแบบตัด leading แล้ว gain = 0.00 dB)

**2–7 dB ที่เห็น = ผลของการเทียบกับไฟล์ golden *demo* ซึ่งมีสองอย่างที่ไม่ใช่ของเรา:**

| แหล่ง | ขนาด | หลักฐาน |
|---|---|---|
| (1) **Fader pan ของ OpenUtau** — demo เป็น stereo ที่ผ่าน `Fader` ของ track; track center ⇒ แต่ละ channel × cos(π/4) = 0.7071 = **−3.01 dB** | −3.01 dB (ทุกคู่) | §2 |
| (2) **demo ไม่สลับ renderer ตาม `WORLDLINE_RENDERER`** ⇒ ไฟล์ "golden r2" จริง ๆ คือ **r1**; ของเราใช้ r2 ⇒ เทียบข้าม engine | +3.2 dB (คู่ r2) | §3 |

⇒ **ไม่ต้องแก้โค้ด** · ถ้าจะเทียบ rms/peak ให้ใช้ golden **phrase** (mono, renderer เดียวกัน) หรือบวก 3.01 dB เมื่อเทียบกับ demo channel

## 1. ตารางของ Lead แยกที่มาได้ครบทุกแถว

| คู่ | ที่ Lead วัด | ที่มา | ตรวจกับตัวเลข |
|---|---|---|---|
| note r1 | +2.1 dB | +3.01 (pan) −0.9 (ไฟล์ที่ Lead ใช้เป็น *japanese path* alias `- sa` ไม่ใช่ universal) | universal จริง: 0.6037/0.4268 = **+3.02 dB** |
| note r2 | +5.3 dB | +3.01 (pan) +2.32 (r2/r1 ของ peak) | 0.7889/0.4268 = **+5.33 dB** ✓ |
| sakura r2 | +6.5 dB | +3.01 +~3.2 + residual (gap/rest) | ดู §5 (ยังไม่ได้ rebuild reference r2) |
| scale-demo r2 | +7.3 dB | +3.01 +~3.2 + residual | ดู §5 |

หมายเหตุไฟล์: `out/ab/ours-r1-note-FIXED.wav` และ `-r2-note-FIXED.wav` (15:24) เป็น **japanese path** (alias `- sa`, peak 0.688)
ไม่ใช่ universal/default — คู่ like-for-like สำหรับโน้ตต้องใช้ `final-universal.wav` (peak 0.6037)

## 2. ที่มาของ −3.01 dB: Fader ของ OpenUtau (path:line)

`ref-openutau-clean/core/OpenUtau.Core/Render/RenderEngine.cs:97-107` — ทุก track ถูกห่อด้วย `Fader`:

```csharp
var trackMix = new WaveMix(trackSources);
var fader = new Fader(trackMix);
fader.Scale = PlaybackManager.DecibelToVolume(track.Muted ? -24 : track.Volume);   // default 0 dB -> 1.0
fader.Pan = (float)track.Pan;                                                      // default 0 = center
fader.SetScaleToTarget();
```

`SignalChain/Fader.cs:40-48` ใช้ `MusicMath.PanToChannelVolumes(pan)` และ `Util/MusicMath.cs:247-249`:

```csharp
float angle = (Math.Clamp(pan, -100f, 100f) + 100f) / 200f * (float)(Math.PI / 2);
return ((float)Math.Cos(angle), (float)Math.Sin(angle));   // pan=0 -> (cos pi/4, sin pi/4) = 0.7071
```

⇒ center ⇒ **L = R = 0.7071 × source** = −3.0103 dB ต่อ channel (equal-power; พลังงานรวมเท่าเดิม)

**วัดยืนยัน (corr 1.000000):**

```text
golden demo L  (66,211) vs golden phrase[5982:] (66,211):  corr 1.000000  gain 0.7071  (-3.01 dB)
golden demo R  == golden demo L  (ไม่มี pan asymmetry)
```

⇒ demo = phrase × 1/√2 **เป๊ะ** ⇒ −3.01 dB มาจาก Fader ไม่ใช่จาก content

## 3. ที่มาของ +3.2 dB ในคู่ r2: demo ไม่สลับ renderer

```text
cmp out/ab/golden-openutau-r1-sakura-VALID.wav out/ab/golden-openutau-r2-sakura-VALID.wav  -> identical
cmp out/ab/golden-openutau-r1-note-VALID.wav   out/ab/golden-openutau-r2-note-VALID.wav    -> identical
```

⇒ ไฟล์ golden "r2" ทั้งหมดของ demo คือ **ผลจาก r1** (env ไม่มีผลกับโหมด demo) ⇒ การเอา r2 ของเราไปเทียบ = เทียบข้าม engine
แต่ r2 ของเราถูกต้อง — เทียบกับ **golden r2 phrase** (ซึ่ง env มีผล):

```text
ของเรา r2/r1 = +3.21 dB   (rms -9.19 vs -12.40)
golden r2/r1 = +3.17 dB   (rms -9.60 vs -12.77)
ของเรา r2 vs golden r2 phrase: corr 0.982  gain -0.16 dB
```

⇒ R2 chain ของเรามีระดับเสียงตรงกับ R2 ของ reference (ผลต่าง 0.04 dB ใน ratio r2/r1)

## 4. ฝั่งเราตั้งใจให้เป็น mono unity (โดยดีไซน์)

`native/crates/runtime/src/mixer.rs:95-101` — pan ของเราเป็น **mono fold-down**:

```rust
/// Stereo gains are `L = cos((pan+1)·π/4)`, `R = sin((pan+1)·π/4)`; the
/// mono fold-down `(L+R)/√2` simplifies to `cos(pan·π/4)`: unity at
/// center, `-3 dB` at hard left/right.
pub fn pan_gain(pan: f32) -> f32 { (pan.clamp(-1.0, 1.0) * FRAC_PI_4).cos() }
```

- ของเรา: center = **1.0** ในสตรีม mono (แล้วผู้เล่นค่อยกระจายเป็น stereo)
- OpenUtau: center = **0.7071 ต่อ channel** (`(0.7071+0.7071)/√2 = 1.0` ภายใต้ fold-down เดียวกัน)
⇒ สองแบบเทียบเท่ากันทางพลังงาน — ต่างกันแค่ *การแทนค่า* (mono master vs stereo pair)

**ข้อสังเกต (ไม่ใช่บั๊กของ task นี้):** ถ้าแอพของเราเล่นไฟล์ mono นี้ออกลำโพงทั้งสองข้างด้วย gain 1.0 ต่อข้าง
พลังงานรวมจะเป็น **+3 dB** เมื่อเทียบกับ track center ของ OpenUtau (ซึ่งตั้งใจลด 0.7071 ต่อข้าง)
⇒ ถ้าต้องการ parity ด้าน *ความดังที่ได้ยิน* ควรใช้ pan law เดียวกันตอนกระจายออก stereo (ควรคุยกับ Lead ก่อน — นอกขอบเขตงานนี้)

## 5. ข้อจำกัด / ยังไม่ได้ยืนยัน

1. **sakura และ scale-demo: ยังไม่ได้ยืนยัน r2 parity แบบเต็มเพลง** — ต้อง rebuild reference จาก golden **r2 phrase**
   (รัน `WORLDLINE_RENDERER=2` ใน phrase mode แล้ววางที่ offset จริง) ซึ่งยังไม่ได้ทำในรอบนี้
   ที่ยืนยันแล้วคือ **note r2** (gain −0.16 dB) และ **note/sakura r1** (§1)
2. `corr` ของ r2 = 0.982 (ไม่ใช่ 1.0 เหมือน r1) — คาดว่ามาจาก ONNX runtime/EP หรือความต่างเล็กน้อยของ feature
   ยังไม่ได้ไล่ในงานนี้ (ไม่กระทบระดับเสียง)
3. การเทียบ rms ของไฟล์ที่มี rest/gap ต่างกัน **ไม่ใช่ pure gain** — ต้องใช้ waveform gain/corr (ตามที่ทำในเอกสารนี้)
4. ตัวเลข rms ของ Lead ("golden −15.4 dB") เป็นค่า *ต่อ channel* ของไฟล์ stereo; rms ของ mono phrase คือ −12.8 dB

## 6. เกณฑ์ที่แนะนำต่อจากนี้

| เทียบกับ | ใช้ได้ไหม | ต้องปรับอะไร |
|---|---|---|
| golden **phrase** (mono, renderer เดียวกัน) | ✅ ดีที่สุด | ไม่ต้องปรับ — corr 1.0, gain 0.00 dB |
| golden **demo** channel (stereo) | ⚠️ ได้แต่ระวัง | **+3.01 dB** (Fader center pan) **และ** ต้องบังคับ renderer ให้ตรง (demo ไม่สลับเอง) |
| golden **demo** power (L²+R²) | ✅ | เทียบกับ mono power ของเราได้ตรง (พลังงานเท่ากัน) |

## 7. ไฟล์ที่แตะ

| ไฟล์ | สถานะ |
|---|---|
| `docs/planning/gain-level-parity.md` | เอกสารนี้ (ใหม่) |

**ไม่ได้แก้โค้ดใด ๆ** (ไม่พบ gain error ฝั่งเรา) — ไม่แตะ `mixer.rs`, `feed/**`, `synth-cli/**`, `default.rs`
(ระวังข้อห้ามของ task-14 ด้วย)

ไฟล์/คำสั่งที่ใช้: `out/ab/golden-openutau-*-VALID.wav` (ใหม่หลังแก้ harness), `/tmp/g-note1440.wav`, `/tmp/g-note-r2.wav`,
`/tmp/final-universal.wav`, `/tmp/our-note-r2-universal.wav`, สคริปต์วัดใน `/tmp/r2w-gain*.py`
