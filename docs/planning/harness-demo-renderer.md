# harness-demo-renderer — ให้ `demo` เคารพ `WORLDLINE_RENDERER` (T17)

> task: `task-17` (r2-wasm) · วันที่: 2026-09-23 · สถานะ: **แก้แล้ว + พิสูจน์แล้ว**
> ปัญหา (เจอใน task-16): `./run.sh demo` ให้ไฟล์เหมือนกันเป๊ะทั้ง `WORLDLINE_RENDERER=1` และ `=2`
> ⇒ ไฟล์ที่ติดป้าย "golden r2" จริง ๆ คือ **r1** ทำให้เทียบข้าม engine และเพี้ยน ~+2–3 dB

## 0. สรุป

| ข้อ | ก่อน | หลัง |
|---|---|---|
| `cmp` demo r1 vs r2 (note) | **identical** | **differ** ✅ |
| `cmp` demo r1 vs r2 (sakura) | **identical** | **differ** ✅ |
| demo r1 (โหมดเดิม) | — | **byte-identical ของเดิม** ✅ (default ไม่เปลี่ยน) |
| `phrase` mode | เคารพ env อยู่แล้ว | ยังเคารพเหมือนเดิม ✅ |
| ours r1 / demo r1 (peak) | +3.01 dB | **+3.01 dB** (คงเหลือ = pan law อย่างเดียว) |
| ours r2 / demo **r2 ใหม่** (peak) | +5.33 dB (เทียบ r1) | **+3.01 dB** ✅ |
| ours r2 sakura / demo r2 ใหม่ | +6.33 dB | **+2.99 dB** ✅ |

## 1. สาเหตุ (ทำไม demo ไม่สลับ renderer)

1. `RenderPhrase` **ฝัง renderer ไว้ตอนสร้าง phrase** — `ref-openutau-clean/core/OpenUtau.Core/Render/RenderPhrase.cs:222`:
   ```csharp
   renderer = track.RendererSettings.Renderer;
   ```
2. `track.RendererSettings.Renderer` ถูกเลือกใน `URenderSettings.Validate()` (`Ustx/UTrack.cs:20-35`)
   ซึ่งถูกเรียกตอน `project.ValidateFull()` — โดยค่าเริ่มต้นคือ `Renderers.GetDefaultRenderer(singerType)` = v1
3. ฝั่ง harness อ่าน env `WORLDLINE_RENDERER` **เฉพาะใน `RenderPhrasesDirect`** (โหมด `phrase`) เท่านั้น:
   `Program.cs` เดิมบรรทัด ~336-340 → `new WorldlineRenderer(ver)`

⇒ `RenderMixdown` (โหมด `demo`) render ผ่าน `phrase.renderer` ที่ ValidateFull ตั้งไว้ ⇒ **ได้ v1 เสมอ** ไม่ว่า env จะเป็นอะไร

## 2. การแก้ (แก้เฉพาะ `test/golden/golden-renderer/Program.cs`)

**2.1 ตัดสิน renderer ก่อนสร้าง phrase** — แทรกก่อน `project.ValidateFull()` (`Program.cs:163-181`):

```csharp
if (!diffSingerMode) {
    string rendererName = RendererVersion() == 2 ? Renderers.WORLDLINE_R2 : Renderers.WORLDLINE_R;
    foreach (var track in project.tracks) {
        track.RendererSettings.renderer = rendererName;
        track.RendererSettings.Renderer = null;   // ให้ Validate() สร้างใหม่จากชื่อที่ตั้ง
    }
    Console.WriteLine($"  renderer override -> {rendererName} (WORLDLINE_RENDERER)");
}
```

เหตุผลที่ตั้งผ่าน **ชื่อ** (`renderer`) แทนการ assign `Renderer` ตรง ๆ: `URenderSettings.Validate()`
จะเทียบ `renderer != Renderer?.ToString()` แล้วสร้างใหม่จากชื่อ ⇒ ถ้าไม่ตั้งชื่อ มันจะทับค่าที่เราใส่
(และ `WorldlineRenderer.ToString()` คืน `WORLDLINE-R`/`WORLDLINE-R2` ตาม version — `Classic/WorldlineRenderer.cs:331`)

**2.2 แชร์ env parser ให้สองโหมด** — เพิ่ม helper (`Program.cs:341-352`) และให้ `RenderPhrasesDirect` เรียกใช้แทนโค้ดเดิม
(`Program.cs:367`):

```csharp
static int RendererVersion() {
    int ver = 1;
    var verStr = Environment.GetEnvironmentVariable("WORLDLINE_RENDERER");
    if (!string.IsNullOrEmpty(verStr) && int.TryParse(verStr, out int parsed)) ver = parsed;
    return ver;
}
```

ไม่แตะ `ref-openutau-clean/**` / `ref(openutau+openutau mobile)/**` เลย (อ่านอย่างเดียว)

## 3. พิสูจน์ (คำสั่ง + ผลจริง)

```bash
cd test/golden/golden-renderer
export OPENUTAU_REF=/media/DATA/project/android-voice-synth/ref-openutau-clean   # repo นี้ไม่อยู่ที่ /home/seal
export WORLDLINE_SO=/media/DATA/project/android-voice-synth/native/build/build-linux/libworldline.so
./build.sh
export TETO_DIR=/media/DATA/project/android-voice-synth/test/golden/teto-english
WORLDLINE_RENDERER=1 ./run.sh demo /tmp/t17-note-r1.wav .../one-note.ustx
WORLDLINE_RENDERER=2 ./run.sh demo /tmp/t17-note-r2.wav .../one-note.ustx
cmp /tmp/t17-note-r1.wav /tmp/t17-note-r2.wav
```

```text
-- env=1 --> "renderer override -> WORLDLINE-R"   Wrote 66211 frames (1501.4 ms stereo), peak 0.4269
-- env=2 --> "renderer override -> WORLDLINE-R2"  Wrote 66722 frames (1513.0 ms stereo), peak 0.5578
cmp: differ  (ก่อนแก้: identical)
cmp /tmp/t17-note-r1.wav out/ab/golden-openutau-r1-note-VALID.wav  -> identical  (default path ไม่เปลี่ยน)

sakura: env=1 -> 154336 frames peak 0.4420 | env=2 -> 154847 frames peak 0.5584 | cmp: differ
cmp /tmp/t17-sakura-r1.wav out/ab/golden-openutau-r1-sakura-VALID.wav -> identical

phrase mode ยังปกติ:  env=1 -> 72193 samples peak 0.6037 | env=2 -> 72704 samples peak 0.7888
```

## 4. A/B ซ้ำหลังแก้ (เกณฑ์ข้อ 4 ของ task)

| คู่ (peak, L channel) | ค่า | gain |
|---|---|---|
| OURS r1 note (universal) 0.6037 / demo **r1** 0.4268 | | **+3.01 dB** |
| OURS r2 note (universal) 0.7889 / demo **r2** 0.5578 | | **+3.01 dB** |
| OURS r2 sakura (DEFAULT) 0.7880 / demo **r2** 0.5584 | | **+2.99 dB** |
| demo r2 / demo r1 (note) | 0.5578/0.4268 | +2.32 dB (= R2 vs R1) |

⇒ **หลังแก้ gain ที่เหลือคือ +3.01 dB ซึ่งคือ pan law ของ `Fader` (task-16) อย่างเดียว** ✅
(ก่อนแก้คู่ r2 วัดได้ +5.33 dB เพราะไฟล์ "r2" เป็น r1)

ค่าคาดหวังจาก task-16: `r2/r1` ของ phrase = peak 0.7888/0.6037 = +2.32 dB ✓ ตรงกับ demo r2/r1 หลังแก้

## 5. หมายเหตุการใช้งาน

1. **ไฟล์ golden demo เก่าที่ติดป้าย `-r2-` ใช้ไม่ได้** — เป็น r1 (ควร regenerate ด้วย harness ที่แก้แล้ว).
   ไฟล์ที่ผม render ใหม่ (ยังไม่ย้ายเข้า `out/ab/`): `/tmp/t17-note-r1.wav`, `/tmp/t17-note-r2.wav`,
   `/tmp/t17-sakura-r1.wav`, `/tmp/t17-sakura-r2.wav`
2. **cache ของ renderer** ตั้งชื่อ `wdl-v{version}-{hash}.wav` (`Classic/WorldlineRenderer.cs:73`)
   ⇒ v1/v2 ไม่ชนกัน ไม่ต้องล้าง cache
3. `demo` r2 ยาวกว่า r1: note 66,722 vs 66,211 (+511), sakura 154,847 vs 154,336 (+511)
   — เท่ากับผลต่างของ phrase mode (72,704 vs 72,193) ⇒ เป็นความยาวของตัว R2 render เอง ไม่ใช่บั๊ก harness
4. env ที่ไม่ใช่ตัวเลข (เช่น `abc`) → ใช้ค่า default 1 (พฤติกรรมเดิม) · env ไม่ตั้ง → 1 (ไม่เปลี่ยน default)
5. **ข้อจำกัดที่ยังเหลือ:** ไม่ได้แตะ `native/**` (ตามคำสั่ง — verifier ตรวจ T12/T13) และไม่ได้ regenerate `out/ab/*`
   (เป็น scope ของ Lead) · corr ของ r2 กับ reference = 0.982 (จาก task-16) ยังไม่ได้ไล่สาเหตุ

## 6. ไฟล์ที่แตะ

| ไฟล์ | สถานะ |
|---|---|
| `test/golden/golden-renderer/Program.cs` | เพิ่ม renderer override ก่อน `ValidateFull` (บรรทัด 163-181) + helper `RendererVersion()` (341-352) + ให้ phrase mode ใช้ helper (367) |
| `docs/planning/harness-demo-renderer.md` | เอกสารนี้ (ใหม่) |

ไม่ได้แตะ: `ref-openutau-clean/**`, `ref(openutau+openutau mobile)/**`, `native/**`, `web/**`, `models/**`
