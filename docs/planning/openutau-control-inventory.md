# openutau-control-inventory — ปุ่มทุกตัวในโซนบนของ OpenUtau คืออะไร ทำอะไร และอันไหนควรเอามาใช้ (T004)

> ที่มา: `ref-openutau-clean/desktop-ref/OpenUtau/` — `Views/MainWindow.axaml` (แถบบน),
> `Controls/PianoRoll.axaml` (แถบเครื่องมือ piano roll), `Controls/TrackHeader.axaml` (การ์ด track),
> `Controls/PianoRoll.axaml.cs` + `Views/MainWindow.axaml.cs` (คีย์ลัด), `Strings/Strings.axaml` (ข้อความจริงบนปุ่ม)
> เทียบกับของเราที่ `ui-mock/react-dock/src/openutau/` (File menu + transport + tool rail + panels)
> วันที่สำรวจ: 2026-10-02 · สถานะ: สำรวจเสร็จ ยังไม่ตัดสินใจว่าจะทำตัวไหน

## 0. สรุปคำตอบสั้น

แถบบนของ OpenUtau มี **4 กลุ่ม**: เมนูบาร์ · เวลา/จังหวะ (แก้ได้) · transport 4 ปุ่ม · ตำแหน่งเล่น + แถบ render
ส่วนปุ่มที่มีเยอะจริงคือ **แถบเครื่องมือของ piano roll** (เมนูซ้าย 3 เมนู + tool rail 9 เครื่องมือ + toggle 11 ตัว + snap/key)

ของเรามีแล้ว: transport (เล่น/หยุด/ไปต้น) + skip ตามห้อง/โน้ต (ที่ OpenUtau *ไม่มี*), waveform, lyric/phoneme label, snap, expression lanes, File menu ครบ
ของที่ขาดและ**คุ้มที่จะยกมา** 5 อันแรก: `SeekEnd` (ไปท้ายเพลง) · แถบ zoom (บน=นอน, ขวา=ตั้ง) · Tips (T) · Batch Edit · แก้ BPM/TimeSignature

> **สถานะ T014 ณ 2026-10-15:** รายการด้านล่างเป็น inventory เดิมและคำว่า “ยังไม่มี” ในแถวเก่าอาจล้าสมัยแล้ว
> สัญญาเสริม `project.controls.v1` ใน [CONTRACT v3.1](contract-v3.1-project-session-controls.md)
> ผ่าน Rust Session, Direct-WASM และ native HTTP tests แล้ว: `track.create/remove/reorder`,
> `project.import_tracks`, `project.set_tempo` / `project.preserve_timing`, stable-ID tuning/pitch/vibrato,
> และ expression descriptor/stroke-context reads มี engine path; per-phoneme writes ยังปิดไว้จน T028
> มี resolved-phoneme stable targets และ render consumption. การต่อ UI ยังเป็นงาน T004/T027.

## 1. โซน A — แถบบน (MainWindow)

### 1.1 เมนูบาร์ (`MainWindow.axaml:24–96`)

| เมนู | รายการ (พร้อมคีย์ลัด) |
|---|---|
| **File** | New (Ctrl+N) · New from template ▸ · Save template · **Open (Ctrl+O)** · Open recent ▸ · Open project location · **Save (Ctrl+S)** · **Save As (Ctrl+Shift+S)** · Import Tracks… · Import Audio… · Export audio ▸ (WAV / WAV to… / Mixdown) · Export project ▸ (UST / UST to… / MIDI / DiffSinger ds) · Open export location |
| **Edit** | Undo (Ctrl+Z) · Redo (Ctrl+Y) — มีแค่สองตัวนี้ |
| **Project** | Expressions… · Adjust Tempo (Preserve Timing) |
| **Tools** | Layout ▸ (Reset / V-split 1:1 1:2 1:3 / H-split 1:1 1:2 1:3) · Fullscreen (F11) · Clear cache · Debug window · Phonetic assistant · Singers… · Install singer… · Install wavtool/resampler… · Packages… · Preferences… |
| **Help** | Wiki · Check update · Report issue · Open logs location |

เทียบของเรา: **File ครบแล้ว** (New/Open/Save/Save As/Export USTX + dirty dot) — ที่ยังไม่มีคือ template / recent / Export audio / UST / MIDI และ Tools/Help ทั้งเมนู ซึ่งส่วนใหญ่เป็น host capability ไม่ใช่ UI ของ editor

### 1.2 เวลา/จังหวะ — คลิกแก้ได้ (`MainWindow.axaml:222–243`)

| ปุ่ม | แสดงอะไร | คลิกแล้ว | คำสั่ง §6 ที่ต้องใช้ |
|---|---|---|---|
| **4/4** | `BeatPerBar/BeatUnit` จาก time signature แรก | เปิด `TimeSignatureDialog` | `project.set_time_signature` ✅ มีแล้ว |
| **120.00** | BPM จาก tempo แรก (`#0.00`) | เปิด dialog แก้ BPM | `project.set_tempo` ✅ มีแล้ว |

ของเรา: แสดงเป็น chip เฉย ๆ ยังคลิกแก้ไม่ได้ → **candidate อันดับต้น ๆ** เพราะคำสั่งมีอยู่แล้ว เหลือแค่ dialog (ดู §4.3)

### 1.3 Transport — 4 ปุ่มกลาง (`MainWindow.axaml:244–272`)

| ปุ่ม (จากซ้าย) | ไอคอน | ทำอะไร | ของเรา |
|---|---|---|---|
| **Seek Start** | `▎◀` | `Pause()` แล้วย้าย playhead ไป tick 0 | มี (↺ restart) — แต่ของเรา *เล่นต่อทันที* ต่างจาก OpenUtau ที่หยุด |
| **Play / Pause** | `▶` | `OnPlayOrPause` สลับเล่น/หยุด (คีย์ Space ก็เรียกอันเดียวกัน) | มี (▶/❚❚) |
| **Pause** | `❚❚` | `PlaybackViewModel.Pause()` — **หยุดอย่างเดียว ไม่ย้าย playhead** | มี (■ stop) |
| **Seek End** | `▶▎` | `Pause()` แล้วย้าย playhead ไป `Project.EndTick` | **ยังไม่มี** → §4.1 |

> ข้อสังเกต: OpenUtau **ไม่มีปุ่ม skip ทีละห้อง/ทีละโน้ต** บนแถบบนเลย — การเลื่อน playhead อยู่ใน piano roll ผ่าน `[` `]` (§2.4) ของเราจึง "เกิน" OpenUtau ตรงนี้ (ผู้ใช้ขอมา)

### 1.4 ตำแหน่งเล่น + อย่างอื่นในโซนบน

| องค์ประกอบ | ทำอะไร | ของเรา |
|---|---|---|
| **PlayPosTime** `mm:ss.fff` (`MainWindow.axaml:273–280`) | อ่านค่าอย่างเดียว อัปเดตตาม playhead | มี (`ou-chip-clock`) |
| **ViewScaler** (`MainWindow.axaml:392–397`) | ลาก/สกอลล์ = ความสูงแถว track (vertical zoom) | **ยังไม่มี** → §4.2 |
| **ScrollBar (Classes=music)** 2 แกน | เลื่อนดู timeline; ใน piano roll แถบบนสุดคือ *แถบ zoom แนวนอน* | ยังไม่มี (ของเราใช้ wheel อย่างเดียว) |
| **ProgressText + ProgressBar** (`MainWindow.axaml:405–408`) | ข้อความ/หลอด progress ของการ render ("waiting rendering") | ยังไม่มี → §4.5 (ผูกกับ T011) |
| **PlayPos highlight** (`MainWindow.axaml:337–346`) | แถบไฮไลต์ช่วงที่เลือก + เส้น playhead | มีเส้น playhead |

### 1.5 คีย์ลัดระดับหน้าต่าง (`MainWindow.axaml.cs:787–870`)

| ปุ่ม | ทำอะไร | หมายเหตุ |
|---|---|---|
| `Space` | play/pause | เรามีแล้ว |
| `Home` | playhead → 0 | เรามี (Home = stop + seek 0) |
| `End` | playhead → ท้ายโปรเจกต์ | **ยังไม่มี** |
| `Delete` | ลบ part ที่เลือก | ยังไม่มี |
| `Shift+S` / `Shift+M` | solo / mute track ของ part ที่เลือก | ยังไม่มี (คำสั่งมี) |
| `Ctrl+A/N/O/S/Z/Y/C/X/V`, `Ctrl+Shift+Z/S` | เลือกทั้งหมด / lifecycle / undo-redo / clipboard | lifecycle มี; clipboard ยังไม่มี |
| `F11` | fullscreen | ยังไม่มี |

## 2. โซน B — แถบเครื่องมือ piano roll (`PianoRoll.axaml:220–555`)

### 2.1 เมนูซ้าย 3 เมนู + 2 ปุ่ม (`PianoRoll.axaml:229–377`)

| เมนู | รายการ |
|---|---|
| **Edit** | Undo · Redo · Cut (Ctrl+X) · Copy (Ctrl+C) · Paste (Ctrl+V) · Paste as plain notes (Ctrl+Shift+V) · Delete · Search note (Ctrl+F) · Select All (Ctrl+A) · Singers… · **Lock editing of unselected notes** ▸ (Pitch Points / Vibrato / Expressions) |
| **View** | Show portrait · Show icon · Show ghost notes · Use track color · Fullscreen (F11) · Special keys ▸ (Off / Solfège / Numbered) · Lock start time ▸ (Off / On / Only cursor) · Auto scroll ▸ (Off / Stationary cursor / Page scroll) · Piano roll ▸ (Detach / Hide) |
| **Batch** | Notes ▸ · Lyrics ▸ · Reset ▸ · External ▸ · legacy plugin expressions ▸ (ทั้งหมดเป็น submenu หลายสิบรายการ ดู §4.4) |
| **Edit Lyrics** | เปิด dialog แก้ lyric ทุกโน้ตทีเดียว (`Ctrl`+การเลือกหลายโน้ต + Enter) |
| **Note Defaults** | ค่า default ของโน้ตใหม่ (lyric/tone/duration/expressions) |

### 2.2 Tool rail — 9 เครื่องมือ (`PianoRoll.axaml:397–417`, ชื่อจาก `Strings.axaml:519–550`)

| # | เครื่องมือ | คีย์ | ทำอะไร | ของเรา |
|---|---|---|---|---|
| 1 | **Selection Tool** | `1` | ลากกรอบเลือก, Ctrl เพิ่ม, ↑↓ transpose | ✅ `⌖` Select |
| 2 | **Pen Tool** | `2` | ลากซ้าย = วาดโน้ต, คลิกขวา = ลบ | ✅ `✎` Pen |
| 2+ | **Pen Plus Tool** | `Ctrl+2` | ปากกาที่วาดแล้วตั้งค่าเพิ่ม (submenu ของ Pen) | ❌ |
| 3 | **Eraser Tool** | `3` | ลากลบโน้ต | ✅ `⌫` Eraser |
| 4 | **Knife Tool** | `4` | คลิกตัดโน้ตที่จุดคลิก | ✅ `⌁` Split (`note.resize` + `note.add` เป็นหนึ่ง group) |
| 5 | **Draw Pitch Tool** | `Shift+1` | วาดเส้น pitch อิสระ | ⚠️ ของเรามี `∿` Pitch drag = retune เป็น semitone (คนละอย่าง) |
| 6 | **Pitch Line Tool** | `Shift+2` | ลากเส้นตรง pitch | ❌ |
| 7 | **Pitch S-Curve Tool** | `Shift+3` | ลาก S-curve (portamento) | ❌ |
| 8 | **Pitch Sine Wave Tool** | `Shift+4` | ลาก sine (vibrato) | ❌ |
| 9 | **Pitch Smoothen Tool** | `Shift+5` | ลากเกลี่ยเส้น pitch ให้เรียบ | ❌ |

> ทั้ง 5 ตัวหลัง (5–9) เขียนลง `pitd` ซึ่งตรงกับ `curve.apply_stroke` ที่เรามีอยู่แล้ว → ต้นทุนอยู่ที่ UI gesture + การแสดงเส้น pitch ไม่ใช่ engine

### 2.3 Toggle 11 ตัว + snap/key (`PianoRoll.axaml:380–538`, ป้ายจริงจาก `Strings.axaml:505–518`)

| ปุ่ม | ป้าย (มีคีย์ในวงเล็บ) | ทำอะไร | ของเรา |
|---|---|---|---|
| `?` | **View Tips (T)** | เปิด/ปิดข้อความช่วยเหลือลอยบน canvas | ❌ → §4.1 |
| ✎+ | **Toggle Overwrite Pitch Mode** | วาด pitch ทับของเดิมแทนการแทรก | ❌ |
| 👁 | **View Vibrato (U)** | แสดงเส้น vibrato | ❌ (คำสั่ง Rust มีแล้วใน v3.1; UI integration ยังไม่เสร็จ) |
| 👁 | **View Pitch Bend (I)** | แสดงเส้น pitch bend | ❌ |
| 👁 | **View Final Pitch to Render (R)** | แสดง pitch สุดท้ายที่ render จริง | ❌ ต้องมีข้อมูลจาก render |
| 👁 | **View Waveform (W)** | แสดง waveform | ✅ (overlay ลาก/ย่อ/ซ่อนได้) |
| 👁 | **View Phonemes (O)** | แสดง phoneme เป็นแถบ 60px ล่าง canvas | ❌ ต้องรอ phonemizer (T013) |
| 👁 | **View Expressions (L)** | แสดงเลน expression | ✅ (panel แยก + 7 descriptor) |
| 🏷 | **View Phonemizer Tags** | แสดง tag ของ phonemizer | ❌ |
| 🎵 | **Toggle Note Tone (Y)** | คลิกแล้วเล่นเสียงโน้ต | ❌ ต้องมี audio (T011) |
| 🧲 | **Toggle Snap (P)** + **Snap Division** (ข้อความ เช่น `1/4`, เมนูเลือก) | เปิด/ปิด snap + เลือกหน่วย | ⚠️ ของเราเป็น dropdown 1/4–1/32/Off (ไม่มี toggle แยก) |
| 🎹 | **Key (for scale degree display)** (ข้อความ + เมนู) | เลือกคีย์เพื่อโชว์ degree (Off/Solfège/Numbered) | ❌ ต้องมี `project.key` (`project.set_metadata key` มีใน §6) |

### 2.4 อย่างอื่นใน piano roll ที่ไม่ใช่ปุ่ม

| องค์ประกอบ | ทำอะไร |
|---|---|
| แถบบนของ canvas (`tip.notes.zoomx`) | สกอลล์ = **zoom แนวนอน** |
| แถบขวา (`ViewScaler`, `tip.notes.zoomy`) | ลาก/สกอลล์ = **zoom ตั้ง** (ความสูงแถว) |
| **LyricBox** (กว้าง 360px) | กด `Enter` บนโน้ตที่เลือกเพื่อพิมพ์ lyric, `Tab`/`Shift+Tab` ไปโน้ตถัดไป |
| **SearchBar** (มุมซ้ายล่าง) | ค้นหาโน้ต (Ctrl+F) |
| **ValueTip** | ป้ายตัวเลขลอยบอกค่าที่กำลังลาก |
| **Expression strip** 2 ชั้น | `ExpressionCanvas` ชั้นล่าง = เส้นของ "secondary key" (เงา), ชั้นบน = primary; มี toolbar ย่อยของ curve (Cursor / Pen / Eraser / Line) |
| **คีย์ `[` `]`** | เลื่อน playhead ทีละ snap unit (Ctrl = ไปจุดเริ่ม/จบของ selection, Shift = ไปหัว/ท้าย view) |
| **คีย์ ↑↓ / ←→ / Alt / Ctrl / Shift** | transpose ±1 (±12 ด้วย Ctrl) / เลื่อน cursor / resize / move / ขยาย selection |
| **คีย์ `+` `-`** | ยืด/หดโน้ตที่เลือก |
| **Space, Alt+Space, Enter, Escape, Insert, Delete, Back** | เล่น/เล่นเฉพาะช่วงที่เลือก/แก้ lyric/ยุบ selection/แทรก/ลบ/ลบ |
| **Tips overlay** (`tip.notes.basics`, `tip.exps`) | แผ่นช่วยเหลือบน canvas: อธิบายว่าลากโน้ต = move, ลากปลาย = resize, Alt+ลากปลาย = resize เพื่อนบ้าน, T = ปิด tips |

## 3. โซน C — การ์ด track / arrangement (`TrackHeader.axaml`)

| องค์ประกอบ | ทำอะไร | ของเรา |
|---|---|---|
| เลข track + รูป singer | ป้ายหมายเลข, รูป avatar | ✅ |
| **ชื่อ track** (คลิก) | rename (`track.rename` มีใน §6) | ❌ (แสดงอย่างเดียว) |
| **Singer** / **Phonemizer** / **Renderer** (ปุ่มข้อความ คลิก/คลิกขวา) | เปิดเมนูเลือก singer / phonemizer / renderer | ⚠️ แสดงเป็นข้อความ ไม่คลิกได้ |
| **M / S** | mute / solo (`track.set_mute` / `set_solo`) | ✅ |
| **Volume fader** (−24…+12 dB) | `track.set_volume_db` | ✅ |
| **Pan fader** (−100…100) | `track.set_pan` | ❌ (คำสั่งมี) |
| **fx** (มี flyout) | เปิด dialog FX ของ track | ⚠️ มีปุ่มแต่ no-op |
| **⚙ Track settings** | เปิด `TrackSettingsDialog` (สี track, singer, renderer…) | ⚠️ มีปุ่มแต่ no-op |
| **TrackAdder (+)** | เพิ่ม track ใหม่ | ⚠️ engine รองรับ `track.create` ใน v3.1; connected UI ยังไม่ได้ต่อ |
| เมนูคลิกขวา (จาก `tracks.*`) | Rename · Duplicate · Duplicate settings · Move Up/Down · Remove · Mute all others / this only / Unmute all · Track color | ❌ |

## 4. Candidate — เรียงตามความคุ้ม (คำแนะนำ)

เกณฑ์: (ก) ผู้ใช้เห็นผลทันที (ข) ใช้ของที่มีอยู่แล้วใน §6 ไม่ต้องแตะ Rust (ค) ไม่สร้าง state ซ้อนกับ engine

### 4.1 กลุ่ม UI-only ทำได้เลย — ต้นทุน S

| # | ของที่จะยกมา | ทำไมคุ้ม |
|---|---|---|
| 1 | **Seek End** (`▶▎`) + คีย์ `End` | ปิดช่องว่างสุดท้ายของ transport เทียบ OpenUtau; ใช้ `projectTicks()` ที่มีอยู่ |
| 2 | **Tips (T)** | ช่วยสอน gesture ที่เราซ่อนอยู่ (Alt+ลากปลาย = resize เพื่อนบ้าน ฯลฯ) — ขั้นตอนแรกของ discoverability |
| 3 | **Auto scroll** (Off / Stationary cursor / Page scroll) + **Lock start time** | เป็น view state ล้วน แต่เปลี่ยนความรู้สึกตอนเล่นยาว ๆ มาก |
| 4 | **Special keys** (Off / Solfège / Numbered) | ของโน้ตแบบมี degree |
| 5 | **Lock editing of unselected notes** | กันแก้ผิดตอนเลือกกลุ่ม (เป็น guard ฝั่ง UI เท่านั้น) |
| 6 | **ViewScaler + แถบ zoom นอน/ตั้ง** | งานหนักของ Pixi viewport อยู่แล้ว; เป็นสิ่งที่ผู้ใช้ลอก OpenUtau จะสังเกตว่า "ขาด" ทันที |

### 4.2 ต้องตระเตรียม view state แต่ยังไม่ต้องแตะ Rust — ต้นทุน S–M

| # | ของที่จะยกมา | หมายเหตุ |
|---|---|---|
| 7 | **Pen Plus** (วาดแล้วตั้ง lyric/duration เริ่มต้น) + **Note Defaults** dialog | เป็น UI preference ของเครื่อง ไม่ใช่ของโปรเจกต์ |
| 8 | **SearchBar / Search note (Ctrl+F)** | ค้นหาใน snapshot ที่อ่านมาแล้วได้ |
| 9 | **Pan fader** ต่อ track | `track.set_pan` มีอยู่แล้ว |
| 10 | **Track rename** (คลิกชื่อ) | `track.rename` มีอยู่แล้ว |
| 11 | **Track color** + **Use track color** | สี = view state (ยกเว้นอนาคตจะเก็บใน project) |

### 4.3 ต้องมีคำสั่ง §6 ที่มีอยู่แล้ว แต่ต้องออกแบบ dialog — ต้นทุน M

| # | ของที่จะยกมา | คำสั่งที่ใช้ |
|---|---|---|
| 12 | **คลิก 4/4 → แก้ time signature** | `project.set_time_signature` ✅ |
| 13 | **คลิก BPM → แก้ tempo** (+ เพิ่ม tempo change ตาม tick) | `project.set_tempo` ✅ |
| 14 | **Key menu** (คีย์ของเพลง สำหรับ degree) | `project.set_metadata key` ✅ |

### 4.4 ของที่คุ้มมากแต่ต้องใช้ grouped command — ต้นทุน M

| # | ของที่จะยกมา | เหตุผล |
|---|---|---|
| 15 | **Batch ▸ Notes** — Transpose ±1/±12, Auto Legato, Add tail `-` / `R`, Add breath, Clear vibratos | ทุกตัวคือ `note.move`/`note.set_lyric` ที่รันใน **หนึ่ง group** → ตรงกับกติกา "หนึ่ง gesture = หนึ่ง undo" ของ AGENTS พอดี และเป็นงานที่คนทำเพลงใช้บ่อยที่สุดใน OpenUtau |
| 16 | **Batch ▸ Lyrics** — Hiragana↔Romaji, Replace `-` → `+`, Insert slur, Remove phonetic hint/suffix | `note.set_lyric` ในหนึ่ง group; เป็นที่มาของคุณภาพเสียงร้อง (phonemizer ใช้ lyric) |
| 17 | **Edit Lyrics** dialog (Enter หลายโน้ต) | ต่อยอดจาก 16 / ใช้ LyricBox |

### 4.5 ยังไม่ควรทำตอนนี้ (ติดของอื่น)

| ของ | ติดอะไร |
|---|---|
| **Phoneme view (O)** / **Phonemizer tags** | ต้องรอ phonemization จาก engine (T013) |
| **Note Tone (Y)** / ปุ่มเล่นเสียงตอนคลิก | ต้องมี audio path (T011 streaming playback) |
| **View Pitch (I)** / **Final Pitch (R)** / **Vibrato (U)** / **Overwrite Pitch** | pitch/vibrato/tuning มี command/read ใน v3.1; final pitch และการต่อ UI ยังต้องทำ |
| **Pitch tools 5 ตัว** (line/S-curve/sine/smoothen/draw) | ใช้ `curve.apply_stroke` ได้ แต่ต้องมี "เส้น pitch ที่แก้ได้" ใน roll ก่อน (งาน lớnกว่า Batch/transport) |
| **Copy / Cut / Paste / Paste as plain notes** | §6 ยังไม่มี clipboard command (ต้องกติกา identity ของ id ที่ paste) |
| **Track add / remove / reorder / duplicate** | create/remove/reorder รองรับใน v3.1; duplicate ยัง deferred |
| **Packages / Singers / Install / Preferences** | เป็น host capability ไม่ใช่ editor UI |
| **Export audio (WAV/Mixdown) / UST / MIDI / DiffSinger ds** | ต้องมี render plan + codec (T011 ต่อ) |
| **Import Tracks / Import Audio** | ต้องมี media reference model (§6.3.1 wave part) |

### 4.6 ของที่เรามีแล้วและ OpenUtau ไม่มี (เก็บไว้ ไม่ต้องลอก)

- **Skip ทีละห้อง / ทีละโน้ต** (⏮ ◁ ▷ ⏭) พร้อมคีย์ `←/→`, `,/.`
- **Restart** เป็นปุ่มแยกจาก Stop
- **Dock Manager** (ซ่อน/เปิด panel ที่ปิดไปแล้ว)
- **Project chip + dirty dot** และ **File menu ที่ผูกกับ session lifecycle จริง**

## 5. ลำดับที่แนะนำ (ถ้าจะทำต่อ)

1. **4.1 ทั้งกลุ่ม** (Seek End, Tips, Auto scroll, Special keys, Lock unselected, ViewScaler/zoom) — ปิดช่องว่างที่เห็นได้ทันทีโดยไม่แตะ Rust
2. **4.4 Batch ▸ Notes + Lyrics** — ให้คุณค่าเชิงดนตรีสูงสุดต่อหนึ่งหน่วยงาน และบังคับให้เส้นทาง grouped command ถูกใช้จริงทั้งเส้น
3. **4.3 BPM / Time signature / Key** — ใช้คำสั่งที่ §6 มีอยู่แล้ว ให้โปรเจกต์ "แก้ได้จริง" ไม่ใช่แค่อ่าน
4. **4.2** ที่เหลือ แล้วค่อยไป **4.5** เมื่อ T011/T013 ส่งข้อมูลมาให้

> กติกาที่ต้องยึดตอนทำ: เมนู/ปุ่มทั้งหมดนี้เป็น **view state** ยกเว้นรายการที่เขียนว่าใช้คำสั่ง §6 — ห้ามให้ frontend คิดค่า musical เอง (AGENTS.md) ของที่ทำแล้วต้องมี probe check ใน `npm run drag-probe` กำกับทุกอัน
