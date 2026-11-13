# Known Limitations — ข้อจำกัดที่รู้แล้วและตั้งใจไม่แก้ตอนนี้

> อัปเดต: 2026-10-04 · ผู้ดูแล: task record ใน `docs/work/BOARD.md`
>
> เอกสารนี้คือ **ทะเบียนข้อจำกัด** ของ engine: อะไรที่ "ยังทำไม่ได้" และ
> **ทำไม** (ไม่ใช่แค่ว่าไม่ได้) ทุกข้อมีวิธีตรวจสอบหลักฐาน เพื่อไม่ให้มีการอ้าง
> ความสามารถที่ host หนึ่งไม่มีจริง ถ้าจะแก้ข้อจำกัดข้อใด ต้องเพิ่ม architecture
> decision ก่อน แล้วค่อยลบข้อนั้นออกจากที่นี่

ขอบเขต: ข้อจำกัดของ **engine** (Rust) และ host ที่มันรันอยู่
(`engine-wasm` = browser, `synth-server`/`synth-cli` = native)
ไม่รวมข้อจำกัดของ UI ที่ยังไม่ได้ทำ — ดู board/task records แทน

---

## L1 — ONNX บน wasm: engine รัน neural engine ไม่ได้ (ตั้งใจ)

**ข้อเท็จจริง:** `engine-wasm` ไม่มี ONNX runtime เลย ไม่ใช่เพราะ "ยังไม่ได้
ต่อ" แต่เพราะ **ไม่ได้ compile เข้าไป**

- `native/tools/synth-cli/Cargo.toml`:
  `default = ["ffi", "r2", "parallel"]` และ `r2 = ["dep:ort", "dep:diffsinger", "dep:moe-train", "dep:burn", "worldline-plugin/r2"]`
- `native/crates/engine-wasm/Cargo.toml`: พึ่ง `synth-cli` ด้วย `default-features = false`
  ⇒ `r2` ไม่เปิด ⇒ `ort` / `diffsinger` / `moe-train` / `burn` ไม่ถูก compile เข้า wasm

**ผลกระทบ:** บน browser **engine รัน WORLDLINE-R2, DiffSinger และ MoE ไม่ได้**
renderer ที่ `engine-wasm` รันได้จริงคือ WORLDLINE-R ผ่าน pure-Rust kernel
(`worldline-rs`) — ทั้งแบบ `r1-rs` และ Classic

**ทำไมถึงเป็นแบบนี้ (เหตุผลเชิงสถาปัตยกรรม ไม่ใช่แค่ความยาก):**
browser ไม่มี `dlopen` (`.so` ใช้ไม่ได้), ไม่มี thread pool (`parallel` ปิดไป),
และ `ort` ไม่ build เป็น `wasm32-unknown-unknown` ในแบบที่เราใช้
→ ถ้าใส่ ONNX ใน wasm ตอนนี้ เราจะได้ "renderer ที่ประกาศว่ามี แต่รันไม่ได้"
ซึ่งแย่กว่าไม่มีเลย

**สิ่งที่มีอยู่จริงแต่ไม่ใช่ product path:** POC ชุด ORT-Web
(`web/run-vocoder.sh`, `web/run-r2.sh`, `web/run-moe.sh`, สรุปที่
[wasm-neural-poc-summary.md](../planning/wasm-neural-poc-summary.md)) รัน
vocoder/mel/MoE ในเบราว์เซอร์ได้จริงและผ่านการตรวจ — แต่**เป็น inference ที่ทำใน JS**
ไม่ใช่ใน Rust engine ตามนโยบาย engine-owned state ([../AGENTS.md](../../AGENTS.md))
หน้าเว็บที่เรียก ONNX runtime เอง = frontend มี authority ตัวที่สอง จึง
**คงเป็น POC เท่านั้น** จนกว่าจะมี architecture decision

**ขอบเขตที่ engine-wasm ยังทำได้** — ครึ่งแรกของสาย R2:
`Engine::render_r2_features_chunk()` สกัด WORLD feature tensor (f0 / sp_env / ap)
ด้วย pure-Rust kernel แล้วส่งออก ให้ host เอาไปป้อน ORT-Web worker
⇒ **feature extraction อยู่ใน Rust ได้ · inference ไม่ได้**

**หลักฐานที่กันไว้:** `native/crates/engine-wasm/tests/host_capability_limits.rs`
(4 tests) — เช็ก manifest ตอนรันเทสต์ ถ้ามีคนเปิด `r2` ให้ wasm หรือใส่ `ort`
เข้า `engine-wasm` เทสต์จะ fail พร้อมชี้ doc นี้

---

## L2 — Host ไม่ได้ประกาศความสามารถของ renderer ตัวเอง

**สถานะจริง:** `project_session::v2::capabilities()` รายงาน
`features = [stable_ids, partial_reads, group_transactions, curve_apply_stroke,
curve_clear]` และ `render = {async_jobs: false, per_job_cancel: false,
progress_transports: []}`

- ข่าวดี: มัน **ไม่ over-claim** — ไม่มีรายการ renderer เลย จึงไม่ได้บอกผิดว่า
  "r2/diffsinger พร้อม"
- ข้อเสีย: มันจึงเป็น **ค่าคงที่เดียวกันทุก host** และไม่บอกว่า host นี้ render
  อะไรได้จริง → host ที่แตกต่างกัน (wasm กับ native) รายงาน capability
  ชุดเดียวกัน ซึ่งทำให้ UI ที่อ่านค่านี้ออกแบบฟีเจอร์ผิดได้

**ทำไมยังไม่แก้ที่นี่:** `CONTRACT-v1` §6 (รวม payload ของ capabilities) ถูก
freeze ไว้แล้ว → การเพิ่ม field `renderers` เป็น **การเปลี่ยน contract** ต้องเข้า
review/versioning ก่อน ไม่ใช่งานแก้เอกสาร

**ทางแก้ที่ถูกตามนโยบาย:** รายงาน capability ของ renderer ที่ **engine ตัวเอง**
(ไม่ใช่ session contract) เช่น `engine-wasm` ตอบ `renderers_json()` จาก
registry ที่ compile จริง และ host ที่มี `.so` ตอบชุดที่กว้างกว่า — คนอ่านค่านี้
ต้องรู้ว่ามันผูกกับ build ไม่ใช่กับ source

---

## L3 — Audio track (wave part): **อ่านได้ + mix ได้แล้ว** (ยังขาดบางส่วน)

> อัปเดต 2026-10-04 (รอบที่สอง) — เดิมข้อนี้เป็น “อ่านได้ ยัง mix ไม่ได้” ตอนนี้มี
> render path ทั้ง mixdown และ **ต่อหนึ่ง track** แล้ว ดูหัวข้อย่อยด้านล่าง

**ที่ทำได้แล้ว (native / CLI):**
- crate ใหม่ `native/crates/media` — decode ใน engine เป็น pure Rust ด้วย
  `symphonia` (MP3/WAV/FLAC/OGG) เลือกตัวนี้เพราะ **browser ต้องใช้ตัวเดียวกัน**
  ถ้าใช้ตัวถอดเฉพาะ native จะกลายเป็นฟีเจอร์ที่มีเฉพาะ host เดียว
- seam `MediaSource` (`FileMediaSource` / `MemoryMediaSource`): engine ขอ
  *project-relative reference* → host ตอบด้วย bytes เหมือน `SampleSource` ของ
  `worldline-rs` ทำให้ native (โฟลเดอร์) กับ browser (OPFS) เข้ากันได้โดย engine
  ไม่ต้องรู้ว่า host เก็บ media ยังไง
- port ศาสตร์ของ OpenUtau ตรง ๆ: `UWavePart.UpdateDuration` (`wave_part_duration_ticks`),
  `GetSkipMs`/`GetTrimMs`/`TrimSamples` (`wave_trim_frames` + `apply_wave_trim`)
- `pipeline::render_mixdown` — OpenUtau's *Export mixdown*: ทุก track ผ่าน fader
  (`openutau_fader_scale` + `pan_gain`, mute = -24 dB = เงียบ) แล้วรวมกับ wave
  part ทุกอัน; `synth-cli mixdown` เรียกจากบรรทัดคำสั่งได้
- เทสต์: `media` 14 เคส (10 unit + 4 integration), `mixdown_wave_parts` **10 เคส** (media หาย →
  รายงานเหตุผลไม่ใช่ทิ้งเงียบ, track ที่มีแต่ wave ไม่ error, skip/trim ตัดจริง, mute เงียบจริง,
  **track ที่มีแต่ wave เรนเดอร์ได้ sample-exact ผ่าน per-track path**, และ
  `an_untouched_voice_track_renders_exactly_as_before` ที่พิสูจน์ว่า path ใหม่ไม่เปลี่ยนผลของ
  voice track เดิมแม้แต่ไบต์)
- **render ต่อหนึ่ง track ก็มี wave part แล้ว** (`pipeline::render_track_with_media`):
  เดิม path นี้ error ว่า `no voice part found on track N` เมื่อ track เป็น audio track
  และ **เงียบ ๆ ข้าม** wave part ที่อยู่บน voice track — ตอนนี้ track ที่ไม่มี voice part
  เลยถึงจะ error (และบอกเหตุผล) ส่วน voice part ที่ skip ทั้งหมดยังคงเป็น render ว่างที่สำเร็จ
  ตาม export path เดิม; `RenderReport` มี `wave_parts_mixed` และ skip reason
- **host ต้องบอกที่เก็บ media**: `synth-server` มี **`--media <dir>`** แล้ว
  (สร้าง `FileMediaSource::new` — resolve แบบ `Path.GetFullPath(relative)` ของ OpenUtau
  คือข้ามขึ้นนอกโฟลเดอร์ได้ เพราะ `.ustx` จริงชี้ไปที่ไฟล์นอกโฟลเดอร์โปรเจกต์) ถ้าไม่ให้
  `--media` worker จะใช้ root ที่ไม่มีอยู่จริง เพื่อให้ wave part ถูก**รายงานว่า skipped**
  ไม่ใช่หายไปเงียบ ๆ
- **ตรวจกับของจริง:** `golden-the-end-of-hutsune-miku.ustx` มี wave part จริง
  (MP3 288.4 s ที่ `../../../../../home/<user>/…` ซึ่ง **มีอยู่จริง** บนเครื่องนี้)
  → mixdown ได้ 288.4 s ครบ 4 track เสียงร้อง 153 phrase + 1 audio track

**ที่ยังขาด (จริง ๆ):**
- **ยังไม่มีบน wasm/host อื่น:** `engine-wasm` ผูก `MemoryMediaSource` (ยังว่างเปล่า)
  เข้ากับ `render_track_with_media` แล้ว ⇒ **Direct WASM รายงาน audio part ว่า skipped**
  ทุกตัว ไม่ error (ก่อนแก้มันจะ error `no voice part found`) แต่ยังไม่มีที่เก็บ media
  บน OPFS จริง; `project-session` ยังไม่มี command เพิ่ม/ลบ wave part — session รู้จักอ่าน
  (`wave_part_view`) และปฏิเสธการสั่ง curve บน wave part ถูกต้อง แต่ยังแก้
  document ไม่ได้
- **`media_ref` ยังเป็น path ดิบจากไฟล์ `.ustx`** (`part.relative_path`) ซึ่งเป็น
  path ของเครื่องที่เขียนไฟล์ ไม่ใช่ reference เชิงตรรกะที่ host เป็นเจ้าของ ตาม
  [CONTRACT-v1 §6](../CONTRACT-v1.md) ควรเป็น id ที่ host map เอง — ตอนนี้ engine
  resolve path นั้นเองผ่าน `MediaSource` ซึ่งถูกทางสถาปัตยกรรม แต่ยังไม่ใช่รูปแบบที่
  contract จะรับ
- **fader ของ render ต่อ track ไม่ตรงกับ preview ของ OpenUtau** — ดู L9
- **stereo ถูก fold เป็น mono** ตอน decode (เหมือน `ExportAdapter.ToMono(1,0)` ของ
  OpenUtau) เพราะ mixer ของ engine เป็น mono ตาม data contract
- **resample เป็น linear** ถ้า sample rate ไม่ตรง 44100 (เคสนี้ MP3 เป็น 44100
  พอดีจึงไม่โดน) — ไม่ใช่ windowed-sinc อย่าง NWaves ของ OpenUtau
- **ไม่มี limiter บน mix:** โปรเจกต์จริงนี้รวมแล้ว peak +6.4 dBFS → clip
  16,132 เซมเปิล (0.127%) ตอนเขียนเป็น WAV 16-bit ต้องใช้ `--mixer
  libmixerfx.so` (soft clip) ถ้าต้องการไม่ให้ clip
- `MediaSource` ยังไม่มี cache: decode ทุกครั้งที่ mixdown (MP3 288 s ใช้ ~10 s)
- ยังไม่มี waveform/peaks **จาก engine** สำหรับวาดใน editor — ตอนนี้ editor วาดจาก
  **render preview ของ engine เอง** (เรนเดอร์แล้วสร้าง min/max envelope ฝั่ง UI)
  ซึ่งได้ภาพเดียวกันแต่เป็นการผ่าน render ทุกครั้ง OpenUtau ใช้ cache `Peaks`

---

## L4 — Voicebank resolution ยังอยู่นอก Rust

`UTrack.singer` เป็น **ชื่อ** ไม่ใช่ path และการ map ชื่อ → โฟลเดอร์เสียงทำโดย host
ทำให้ `.ustx` เดิมเปิดข้าม host ไม่ได้ (wasm ใช้ OPFS, server ใช้
`--voicebanks <dir>`) เป็นรากเดียวกับ L3 — audio track จะสืบทอด gap นี้ถ้าไม่
แก้ media resolution ก่อน

หมายเหตุ: `web/samples.js` + `web/zip.js` ยัง parse `oto.ini` และถอด Shift-JIS
เองใน JS ซึ่งเป็นการทำซ้ำของฝั่ง Rust — เป็น transitional debt ที่ยังไม่ได้ย้าย

---

## L5 — `UTrack.volume` / `pan` ยังไม่ถูกใช้ตอน mix (เดิม) → **แก้แล้วใน mixdown**

> **อัปเดต 2026-10-04:** mixdown ใหม่ (`pipeline::render_mixdown`) ใช้ fader จริงแล้ว
> ดู L3 ส่วนที่แก้ ส่วนนี้เหลือเฉพาะ path เดิม `render --track N` (per-track export)
> ที่ยังใส่ `TrackSpec::default()` ตามเดิม — ตรงกับ OpenUtau ซึ่ง per-track
> export ไม่ใช้ fader เช่นกัน

`runtime::TrackSpec` มี `volume_db` + `pan` และ mixer รองรับแล้ว แต่
`pipeline::render_project_with_progress_and_pronunciations` ใส่
`TrackSpec::default()` ทุก chunk ⇒ โปรเจกต์ที่ตั้ง track volume/pan จะได้ผลเหมือน
ไม่ได้ตั้ง **ตั้งใจไม่แก้ตอนนี้** เพราะการแตะค่านี้จะเปลี่ยน golden output ของ
ทุกเทสต์ที่เทียบกับ reference — ต้องจัดการกับ baseline ก่อน

---

## L6 — ยังไม่มี threads ใน engine บน wasm

`parallel` feature ปิดสำหรับ wasm เพราะไม่มี thread pool ชดเชยได้ด้วยการให้
host fan chunk ออก Web Worker (4 workers → 2.66x, bit-identical — ดู
[wasm-parallel.md](wasm-parallel.md)) แต่ **engine เองไม่ขนาน** บน wasm

---

## L7 — `CONTRACT-v1` §6 ยังไม่ implement ครบ

`track.add` / `remove` / `reorder`, `note.set_vibrato` / `tuning` /
`phoneme_expression` และ bounded reads ยังตอบ `unsupported_operation`
(`project-session/src/v2.rs`) — ข้อเสนอ extension อยู่ที่
[contract-v2-extension-t014.md](../planning/contract-v2-extension-t014.md) และ
**ยังไม่ผ่าน review** ห้ามอ้างว่า route/schema ใช้งานได้แล้ว

---

## L8 — ช่องว่าง parity กับ OpenUtau ที่ยัง localize ไม่ได้ (เดิม: "เสียงผิดโน้ต")

> **อัปเดต 2026-10-04 (รอบที่สอง) — ข้อสรุปเดิมถอนแล้ว.** เดิมบันทึกว่า
> "ช่วง 219.76–223.1 s เสียงผิดโน้ต +499…−801 เซนต์" และบอกว่าตัดสินไม่ได้
> เพราะไม่มี `dotnet` ตอนนี้ **มี** (`~/dotnet`, SDK 8.0.425) และเรนเดอร์
> reference จริงจาก OpenUtau ได้แล้ว ผลคือ **หลักฐานเดิมไม่ใช่หลักฐานว่า engine
> ผิด** — ดูด้านล่าง

**เครื่องมือที่ได้มาใหม่** — `test/golden/golden-renderer` (C#, อ้าง
`ref-openutau-clean/core/OpenUtau.Core`) เรนเดอร์ด้วย RenderEngine ตัวจริง
ของ OpenUtau และโหลด `OpenUtau.Plugin.Builtin.dll` ที่มี
`JapanesePresampPhonemizer` ตัวจริง (นี่คือ "DLL phonemizer" — ต้องวางไฟล์ DLL
ไว้ข้าง binary ด้วย เพราะ `DocManager.SearchAllPlugins` โหลดจาก
`AppContext.BaseDirectory` ก่อนเสมอ)

**บั๊ก 4 จุดที่เจอระหว่างทำ และเป็นของเดิมของ harness ไม่ใช่ของเรา** — ทั้งหมด
โผล่เฉพาะโปรเจกต์หลาย track ซึ่งรันก่อนหน้านี้เป็น single-track จึงไม่เจอ:

1. ผูก `Singer` ไว้แค่ `tracks[0]` → track อื่นหา singer จาก *ชื่อ* ไม่เจอ (ชื่อที่
   loader รายงานมี infix `（かさねてと）` ที่ไฟล์ `.ustx` ไม่มี) →
   `UPhoneme.ValidateOto` ตั้ง `Error = "Singer is not loaded."` ทุกตัว →
   `RenderPhrase.FromPart` (กรอง `!phoneme.Error`) ได้ 0 phrase → **per-track
   export เป็นไฟล์ 0 เฟรม**
2. รอ phonemizer แค่ part แรก ส่วนที่เหลือยังไม่เสร็จ → `renderPhrases` ว่าง
3. `UPart.Validate` รันบนสองเธรดพร้อมกัน (ฝั่ง harness กับ
   `PhonemizerRunner.SendResponse` ที่ schedule `Project.Validate` บน main
   scheduler) → ตายด้วย `Collection was modified` — แก้ด้วย single-concurrency
   `TaskScheduler` ที่ใช้เป็นทั้ง `PostOnUIThread` และ main scheduler
4. ตัว export ที่เพิ่งเพิ่มเขียน float stereo-interleaved ลงไฟล์ที่ประกาศว่า mono
   → ยาว 2 เท่าและเสียงต่ำ octave — ต้อง fold ตาม `ExportAdapter.ToMono(1, 0)`

**ผลเทียบจริง (reference = OpenUtau headless, ต่อ track ต่อ track):**

| track | ข้อสังเกตของ reference จากโน้ต | ของเรา | ความต่างสูงสุด (median) |
|---|---|---|---|
| 0 (319 โน้ต) | median 7.5¢ · p90 300¢ | median 1.8¢ | **8.4¢** |
| 1 (103) | median 5.2¢ · p90 126¢ | median 1.2¢ | **5.5¢** |
| 2 (770) | median 7.3¢ · p90 491¢ | median 1.9¢ | **7.7¢** |
| 3 (510) | median 4.0¢ · p90 294¢ | median 1.5¢ | **4.1¢** |

**สิ่งที่ถอน:** ข้อความเดิมว่า "ช่วง 219.76–223.1 s เสียงผิดโน้ต +499…−801 เซนต์"
เป็น**ข้อสรุปที่ผิดวิธี** — วัดที่ onset+50 ms ของโน้ตที่ยาว 83 ms ซึ่งมี glide
แล้วเทียบกับ `tone` ที่เขียนไว้ ซึ่งเสียงร้องจริงไม่ได้ตรงเป๊ะ เมื่อเทียบกับ
**reference ตัวจริงของ OpenUtau ในช่วงเดียวกัน** พบว่า OpenUtau เองก็หลุดจากโน้ต
ถึง −800…+640 เซนต์ที่ตำแหน่งเดียวกัน ดังนั้น "หลุดจากโน้ต" **ไม่ใช่หลักฐาน
ว่า engine ผิด** ของเราตรงกับ OpenUtau ที่จุดนั้นจริง ๆ

**สิ่งที่ยังเปิดอยู่จริง:** ความต่างระหว่างสอง render มี p90 = 179–384 เซนต์
(สูงกว่าช่วงปกติมาก — ช่วง 240–245 s ที่ควบคุมได้ median 4.3¢) และกระจุกตัวที่
จุดเริ่มโน้ต แต่ **วิธีวัดปัจจุบันยังตอบไม่ได้ว่าเป็นบั๊กหรือไม่** เพราะหน้าต่าง 40 ms
บนโน้ตที่สั้นและมี glide ให้ค่าที่ไม่เสถียร (เช่นอ่านได้ 875 Hz ในโน้ตที่ควร
เป็น 523 Hz) ต้องเปลี่ยนวิธีวัดก่อน — เช่น เทียบทีละ *phrase* ตรงตำแหน่ง
`posMs` ที่ OpenUtau พิมพ์มา หรือเทียบ sample ดิบหลัง alignment

**ไฟล์ที่แนบมาจากผู้ใช้ (`test-ustx-example/Export/`) คือ export ต่อ track ของ
OpenUtau ตัวจริง** ยืนยันได้สองทาง: ความยาวตรงกับ render ของเราเป๊ะ
(242.875 / 279.31 / 242.875 s) และตรงกับ headless run ใหม่นี้ภายใน ~200 เฟรม
แต่ไฟล์ `_00.wav` (track 0) **มี 0 เฟรม** — OpenUtau desktop export ออกมาแค่นั้น
ขณะที่ headless render track 0 ได้ 258.88 s ปกติ เรื่องนี้ยังไม่รู้สาเหตุ
(ไม่ใช่ปัญหาของ engine เรา แต่ควรหาสาเหตุเพราะถ้า desktop ทำได้ ก็แปลว่า
มีอะไรบางอย่างพังในเซสชันนั้น)

---

---

## L9 — render ต่อ track ใส่ fader unity (export) ต่างจาก preview ของ OpenUtau

**ข้อเท็จจริง:** `RenderEngine` ของ OpenUtau มี**สองเส้นทางที่ตั้งใจไม่เหมือนกัน**

- `RenderEngine.RenderPreview` (preview/เล่นในโปรแจกต์ —
  `core/OpenUtau.Core/Render/RenderEngine.cs:92-101`) **เพิ่ม wave part ทุกตัวของ
  track เข้าไปใน fader ของ track** และใส่
  `fader.Scale = DecibelToVolume(Muted ? -24 : Volume)`, `fader.Pan = track.Pan`
- `RenderEngine.RenderTracks` (export ไฟล์ต่อไฟล์) **ไม่เอา wave part มาผสม**
  และไม่ใช้ fader ของ track

**ฝั่งเรา:** `pipeline::render_track_with_media` คือ render ต่อหนึ่ง track
(ใช้ได้ทั้งกับ §6.13 render job และ `synth-cli render --track N`) จึงใส่
`TrackSpec::default()` = unity **โดยเจตนา** เพราะมันคือ export stream ที่เอาไปเทียบกับ
per-track WAV ของ OpenUtau ที่ผู้ใช้แนบมา

**ผลกระทบที่ยังไม่จัดการ:** เส้นทางนี้จึง **ไม่ใช่** ภาพของ preview — track ที่ตั้ง
volume/pan ไว้ หรือเป็น audio track จะได้เสียงคนละระดับกับที่ผู้ใช้ได้ยินตอนฟัง
ถ้าวันหนึ่งเอา path นี้ไปทำ playback ต้องแยก "preview" กับ "export" ให้ชัด ไม่ใช่แก้
ค่าเงียบ ๆ เพราะจะทำให้ export เทียบ golden ไม่ผ่าน

**หลักฐาน:** `an_untouched_voice_track_renders_exactly_as_before` ใน
`native/tools/synth-cli/tests/mixdown_wave_parts.rs` ยืนยันว่า path ใหม่ให้ผล
เหมือน export path เดิมทุกไบต์สำหรับ voice track ที่ไม่มี wave part

**ทางแก้เมื่อถึงเวลา:** เพิ่มพารามิเตอร์ระดับ job (ไม่ใช่ระดับ CLI flag) ว่า
`apply_track_fader: bool` แล้วให้ host เลือก — export ปิด, preview เปิด — ตาม
`RenderTracks` / `RenderPreview` ของ OpenUtau
