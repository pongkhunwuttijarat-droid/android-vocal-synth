# Web UI Design — editor บน static web + wasm engine

> สถานะ: **Implementation in progress — editor render ผ่าน RenderPool แล้ว** · อัปเดต 2026-09-23
> ต่อจาก: [wasm-target.md](../architecture/wasm-target.md) · [wasm-parallel.md](../architecture/wasm-parallel.md) · [web-static-feasibility.md](web-static-feasibility.md)

UI production ตัวแรกของ Rusted UTAU บน browser — tablet-first, landscape, ปากกาเป็น input
หลัก. `engine-wasm` render bit-identical กับ native แล้ว และ fan chunk ออก Web Worker
ได้ — ตอนนี้มี UI vertical slice แล้ว; งานที่เหลือคือเชื่อม production rendering,
persistence และ feature breadth ตามเฟสด้านล่าง

**อ่านเพื่อเขียนเอกสารนี้:** `ui-mock/index.html` · `web/{engine.js,worker.js,index.html,pool-full.html,README.md}` · `app/lib/widgets/piano_roll.dart` · `app/lib/screens/editor_screen.dart` (1–800) · `native/crates/{engine-wasm,commands,domain,feed}/src/*` · `native/tools/synth-cli/src/chunking.rs` · `docs/architecture/wasm-*.md` · `docs/planning/{web-static-feasibility,wasm-migration-status,BACKLOG}.md` — (ไม่ได้อ่าน `index-om.html` / `index-mixer.html`; เอกสารนี้ไม่อ้างเนื้อหาสองไฟล์นั้น)

---

## 1. Scope & goals

**Rusted UTAU Web = editor เต็มตัวบน tablet** — เปิด URL แล้วแก้เพลง, กด play ได้ยินเสียง,
export WAV ด้วย engine ตัวเดียวกับ native เป๊ะ โดยไม่มี server / `.so` / dlopen

ลำดับความสำคัญ: (1) **แก้ note ได้จริงด้วยปากกา** — draw/move/resize/erase/split/
box-select/multi-move ต้องนิ่งพอเขียนเพลงจริงได้ (2) **ได้ยินเสียงเร็ว** — worker pool
+ chunk cache, re-render เฉพาะ chunk ที่เสีย (3) **เห็น engine ทำงาน** — chunk marks/
progress จาก `chunk_meta_json()` จริง ไม่ใช่ของประดับแบบ mock (4) **work ไม่หาย** —
undo/redo ที่เชื่อถือได้ + autosave ลง OPFS (5) **แชร์ได้** — export WAV + ไฟล์ `.ustx`

### นอก scope

| ไม่อยู่ใน scope | เหตุผล |
|---|---|
| Neural engines (r2 / diffsinger / moe) | ยัง native-only (`ort`/`burn`); wasm มีแค่ classic path |
| Train model ใน browser (WebGPU) | ต้อง spike แยก — คนละงาน |
| Real-time preview ต่อ keystroke | engine เป็น offline chunk renderer (1.4–2.1s/chunk) |
| Portrait / phone layout | tablet-first; ทำ responsive แค่พอลองได้ |
| Cloud sync / share link / multi-user | static web + OPFS; แชร์ = ส่งไฟล์ `.ustx` |
| VST, MIDI in/out, recording | ไม่มีใน native app เช่นกัน |
| Plugin `.so` / dlopen | ไม่มีบน web โดยสถาปัตยกรรม (feature-gate แล้ว) |
| Wave parts (audio track), guide vocal | `UWavePart` มีใน domain แต่ UI ไม่ทำรอบนี้ |

---

## 2. Screen layout (tablet-first, landscape 1280×800)

คง**โครงเดียวกับ mock** (rail 64 + topbar + main 3 คอลัมน์ + transport + mixer sheet)
แต่เพิ่ม **expression curve lanes** ที่ mock ไม่มี

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│ TOPBAR 48px   midnight_demo ▸ Verse 01        [WORLDLINE-R ▾] ↶ ↷ │Save │Export│
├──────┬──────────────────────────────────────────────────────────────────────────┤
│ RAIL │ TOOLBAR 44px                                                            │
│ 64px │ ✋View ↖Select ✎Draw ∿Pitch ⌁Split ⌫Erase │ ☑All▸ │∿Pitch ⚡FX │        │
│      │ Rate:Normal  Snap:1/16   ZoomX ──●── 90%  ZoomY ──●── 100%  ⊟ ⊞        │
│  ◈   ├──────────────────────────────────────────────────────────────────────────┤
│  ♬   │ TRACKSTRIP 132px                                                         │
│  ▣   │ ┌─ hdr-col 236px ────────┬─ tick-area (timeline 22px + parts) ───────┐  │
│      │ │ [>>>] [110BPM][4/4][C] │  1    2    3    4    5    6               │  │
│  ⚙   │ │ 1 ● Lead Vocal  Teto   │  ▓▓▓▓▓▓▓▓▓▓ Lead Vocal · Verse 01         │  │
│  ?   │ │ 2 ● Harmony     Teto   │  ░░░░░░ Guide (muted)                     │  │
│      │ │ 3 ● Guide      Momo    │                                           │  │
│      │ │ [+]                    │                                           │  │
│      │ └────────────────────────┴───────────────────────────────────────────┘  │
│      ├──────────────────────────────────────────────────────────────────────────┤
│      │ PIANO ROLL (flex — พื้นที่ใหญ่สุด)                              ┌───────┐ │
│      │ ┌─keys 60px─┬─ canvas: grid + notes + phoneme labels + playhead ┤ INSP  │ │
│      │ │ C5         │  1     2     3     4     5     6     7           │ 320px │ │
│      │ │ B4         │  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  │ Lyric │ │
│      │ │ A#4  ▒▒▒▒  │  ╔════════════╗  [m i][d n aI t]                │ Phon  │ │
│      │ │ G#4  ▒▒▒▒  │  ╚════════════╝  ╔══════════╗                  │ Pitch │ │
│      │ │ G4         │  ── ∿ pitch curve (orange, PITD overlay) ─────  │ Dur   │ │
│      │ │ F#4        │  ┊chunk┊ rendering ┊chunk┊ ready  ┊ stale ┊    │ Expr  │ │
│      │ └────────────┴─────────────────────────────────────────────────  │ V/Pan │ │
│      ├──────────────────────────────────────────────────────────────────────────┤
│      │ EXPRESSION LANES  3 lanes × 52px (collapsible)                          │
│      │ ┌ PITD ──── ±1200 ─ 0 ── ▓ 0.0 ─┬─ curve canvas (pen draws here) ─────┐ │ │
│      │ ├ DYN ───── -240..120 ▓ 0.0 dB ┼──────────────────────────────────────┤ │ │
│      │ ├ VOIC ──── 0..100 ─── ▓ 100 ──┼──────────────────────────────────────┤ │ │
│      │ └ [+ add lane ▾: GENC TENC BREC PITD DYN VOIC]                         │ │
│      ├──────────────────────────────────────────────────────────────────────────┤
│      │ STATUS / RENDER BAR 28px                                                │
│      │ ● rendering chunk 2/4 · 1.4s · ETA 1.1s │ cache 3/4 │ ↻ re-render │ 🔊  │
├──────┴──────────────────────────────────────────────────────────────────────────┤
│ TRANSPORT 48px   ▶ ⏹ ⟲  00:18.420 / 01:24.000  [meter ▓▓▓░░░]  master ──●── ⊞Mixer│
├─────────────────────────────────────────────────────────────────────────────────┤
│ MIXER SHEET (slide-up 340px เหนือ transport — ปิดไว้เริ่มต้น): Preset[Default▾]   │
│ [EQ][Comp][SoftClip] │ ต่อ track: VOL fader · PAN knob · EQ L/M/H · Comp GR meter│
│ │ + MASTER strip 132px (border ซ้ายสี primary) ────────────────────────────────│ │
└─────────────────────────────────────────────────────────────────────────────────┘
```

**z-order ใน roll** (ลอกจาก `piano_roll.dart` ซึ่งพิสูจน์แล้วว่าถูก): keys เป็นคอลัมน์
แยก (ไม่ scroll แนวนอน) → grid+notes+lyric+phoneme labels+playhead → PITD curve →
FX/chunk overlay → selection outline → timeline 22px ทับบนสุด

**ค่าคงที่ทางเรขาคณิต — ยกจากของเดิมทั้งชุด ห้ามตั้งใหม่**

| ค่า | ค่า | ที่มา |
|---|---|---|
| keys / timeline / row height | 60 / 22 / 32 px | `_keysWidth`, `_timelineHeight`, `_rowHeight` + mock |
| note height / px-per-beat / zoom clamp | 24 px (75% ของ row) / 80 px / 30–300 % | `_noteHeightFor`, `_basePxPerBeat`, `_applyZoom` |
| ช่วงเสียง | A0..C8 (88 rows, `_topPitch` 48) | `_rows` |
| snap / capture (Rate) | 1/1..1/16 beat / Fine 1/16 · Normal 1/8 · Coarse 1/4 | `_snapBeat`, `_captureStep`, mock `#rateBtn` |
| note สั้นสุด / drag threshold / long-press | 0.25 beat / 4 px / 550 ms | `_onPointerMove`, `_showNoteContextMenu` + mock |
| debounce edit / mixer / autosave / chunk target | 500 / 250 / 1500 ms / ~2 s | `_scheduleReRender`, `_onMixerParams`, `_scheduleAutosave`, `chunking.rs` |

**ทำไม Canvas2D ไม่ใช่ WebGL2:** ระหว่าง render worker กิน CPU ครบ GPU ว่างไม่ช่วย;
Canvas2D วาด text (lyric + phoneme label) ง่ายกว่ามาก และ `CustomPainter` ที่พิสูจน์
แล้วก็เป็น immediate-mode แบบเดียวกัน. ใช้ layered offscreen canvas (static grid /
content / overlay playhead) + dirty-rect เพื่อไม่ repaint note ทั้งก้อนตอน playhead วิ่ง.
ถ้า note เกิน ~5,000 ค่อยย้าย content layer ไป WebGL2

---

## 3. Interaction spec

### Pen-priority mode (สำคัญสุด)

- เห็น `event.pointerType === 'pen'` ครั้งแรก → `penMode = true`
- `penMode` → **finger (`touch`) แก้ note/curve ไม่ได้เลย** ทำได้แค่ pan + pinch
  (mock และ Flutter ไม่แยก pointer type — บน tablet จริงฝ่ามือ/นิ้วชนะ gesture arena บ่อย)
- ยังไม่เห็น pen → touch แก้ได้ (ให้ใช้บนโน้ตบุ๊ก/มือถือได้); Settings toggle:
  Auto / Pen-only / Touch-edits
- **Palm rejection:** ขณะมี pointer `pen` กดอยู่ ignore ทุก `touch` จนกว่า pen จะขึ้นครบ;
  canvas ใช้ `touch-action: none` + `preventDefault()` เฉพาะ event ที่ consume
  (อย่า block scroll ของ panel)

### ตาราง gesture → mutation

| gesture | tool / เงื่อนไข | ผล | command |
|---|---|---|---|
| tap note | ทุก tool ยกเว้น View | select + โชว์ inspector | — (session) |
| tap note ซ้ำ (ไม่มี drag) | Select | dialog แก้ lyric | `ChangeNoteLyricCommand` |
| tap/ลาก note | Erase หรือ **ปากกาด้ามลบ** | ลบ note (+ curve anchor ที่ผูกอยู่) | `RemoveNoteCommand` |
| ลากตัว note | Select + เริ่มในเนื้อ note | ย้าย (snap X `snapBeat`, Y semitone) | `MoveNoteCommand` |
| ลากขอบขวา | Select + 10px สุดท้าย (pen) / 16px (finger) | resize duration (snap, ≥0.25 beat) | `ResizeNoteCommand` |
| ลาก note ในกลุ่มที่เลือก | Select + multi-selection | ย้ายทั้งกลุ่มด้วย delta เดียว | N × `MoveNoteCommand` ในกลุ่มเดียว |
| ลากพื้นที่ว่าง | Select (All) | marquee box-select | — (session) |
| ลากพื้นที่ว่าง | Pitch หรือ Select+Curve | วาด PITD freehand | `SetCurvePointsCommand` |
| ลากบน lane | ทุก tool ในโซน lane | วาด curve ของ expression นั้น | `SetCurvePointsCommand` (abbr ของ lane) |
| tap note | Split | ตัดที่ beat ที่แตะ (snap) | กลุ่ม: `ResizeNoteCommand` + `AddNoteCommand` |
| tap note | Split + multi-select | ตัดทุกตัวที่ playhead/snap | เหมือนบน (คูณจำนวน) |
| long-press 550ms | ทุก tool | menu: Edit lyric / Move up / Move down / Split / Delete | ตามที่เลือก |
| hover (**pen เท่านั้น**) | ทุก tool | preview: outline note, resize cursor, ค่า curve บน lane | — |
| ลาก (View) / middle-drag / space+drag | — | pan X+Y | — |
| 2 นิ้ว / 2 นิ้วลาก | ทุก tool | pinch = zoom X **และ** Y (anchor ที่ focal) / pan | — |
| wheel / shift+wheel / ctrl+wheel / alt+wheel | — | pan Y / pan X / zoom X รอบ cursor / zoom Y รอบ cursor | — |
| คีย์ 1–6 | — | View/Select/Draw/Pitch/Split/Erase | — |
| ←→ / ↑↓ | มี selection | nudge `snapBeat` / ±1 semitone | `MoveNoteCommand` |
| Del / Ctrl+Z / Ctrl+Shift+Z / Ctrl+A / Space / S | — | ลบ / undo / redo / select all / play-stop / split ที่ playhead | ตามบริบท |

### Gesture ที่ pen-critical

1. **วาด PITD / expression curve** — freehand ละเอียด 1–2 px, นิ้วห้ามวาด
   (pen-priority); thinning 3px เหมือน `_capturePx` เดิม
2. **resize ขอบขวา** — 10px (pen) / 16px (finger) และตรวจเฉพาะใน `pointerdown`
   (Flutter เช็ค `(pos.dx - rect.right).abs() <= 8`)
3. **hover preview** — ข้อได้เปรียบเดียวที่ touch ไม่มี; ไม่มี hover → degrade เป็น
   cursor เปลี่ยนตามโซน (เมาส์) หรือไม่โชว์ (นิ้ว)
4. **eraser tail** — `pointerType === 'eraser'` หรือ `pen` + `buttons & 32` → Erase
   ชั่วคราว. **ยังไม่ยืนยันว่า Android stylus ส่งอะไร** (§9 ข้อ 8)
5. **pressure** ไม่ใช้ตัดสินใจใน v1 (engine offline, ไม่มี velocity→expression mapping)
   — เก็บ `event.pressure` ไว้ debug; **barrel button** (`button === 2`) = context menu
   ทางลัดแทน long-press

### กฎ snap / grid

- **X (เวลา)** snap เป็น `snapBeat`, เก็บ `position` เป็น tick = `round(beat × 480)`
  (`domain::RESOLUTION`) — ห้ามเก็บ float beat เพราะ Rust `TimeAxis` ใช้ `csharp_round`
  (banker's rounding) และเป็นเจ้าของการแปลง beat↔ms
- **Y (pitch)** note เป็น semitone เต็ม; **curve เป็น fractional ได้** — `_pencilWrite`
  จงใจไม่ `floor()` (ไม่งั้นเป็นบันได); web เก็บตาม descriptor (PITD = cents ±1200)
- **curve capture** snap X เป็น tick ต่อเป็น multiple ของ `UCurve::INTERVAL = 5` และ
  **upsert** ที่ tick เดิม (ไม่ push ซ้ำ); **smoothing** ผสม point ซ้าย 30% + clamp
  step ละ ±4 semitone (สูตรจาก `_pencilWrite` ซึ่งแก้ "จุดตกพิเศษ" จากการ flick แล้ว)

**Toolbar:** `View · Select · Draw · Pitch · Split · Erase` + select-target วน
`All → Notes → Curve` (mock `#selModeBtn`) + `∿ Pitch` + `⚡ FX` + `Rate` + `Snap` +
Zoom X/Y + ปุ่มลอยกลม 3 ปุ่มมุมขวาบนของ roll (mock `.float-tools`)

---

## 4. Component breakdown — `web/app/`

**Rust เป็นเจ้าของความจริง, JS เป็นเจ้าของพิกเซลและ I/O**: ทุกการตัดสินใจทางดนตรี
(tick, rounding, curve interval, phoneme, chunk plan) อยู่ใน wasm; ทุกการวาด/ชี้/จัดคิว
อยู่ใน JS

| module | หน้าที่ | public surface |
|---|---|---|
| `main.js` | bootstrap: โหลด wasm, เปิด stores, ประกอบ view | `boot()` |
| `engine/driver.js` | ห่อ `Engine`+`Mixer` wasm เป็น object เดียวที่ testable | `ready() plan(t) chunkMeta() chunkKeys(i) applyCmd(c) undo() redo() yaml() loadProject(y) phonemize(t) mixChunks(l) wav()` |
| `store/session.js` | UI state ที่ไม่ใช่ project (tool, selection, zoom, snap, rate, lanes, penMode, mixer params) | `state set(patch) on('change') persist() restore()` |
| `store/voicebank.js` | OPFS voicebank (oto.ini, wav, `<stem>_wav.frq`) | `list() import(files) oto(id) sample(k) frq(k) wavKeys(id)` |
| `store/projects.js` | OPFS project (`.ustx` text) | `list() load(name) save(name, yaml) remove(name)` |
| `render/scheduler.js` | chunk ไหนต้อง render ใหม่ + epoch/progress | `markDirty(t) invalidateAll() on('progress'\|'state') audio() epoch` |
| `render/pool.js` | worker pool ที่ reuse + per-chunk sample loading | `ensure(n) render(track, indices) setVoicebank(meta) terminate()` |
| `render/chunkcache.js` | cache `key → Float32Array` + LRU + mixdown | `put(k,pcm) get(k) keep(keys) bytes() mix()` |
| `render/player.js` | Web Audio playback + playhead | `play(buf, fromMs) stop() position() on('end')` |
| `render/wav.js` | encode Float32 → 16-bit WAV (fallback) | `encodeWav(pcm, sr)` |
| `view/rollcanvas.js` | วาด roll ลง layered canvas | `attach(root) render(scene) invalidate(rect?) resize()` |
| `view/lanes.js` | วาด expression lane + header (ค่า ณ playhead) | `attach() render(lanes, curves)` |
| `view/viewport.js` | แปลง beat↔px, pitch↔px, zoom/pan (pure) | `x() beat() y() pitch() zoomAt(pt,f,axis) visible()` |
| `view/hit.js` | hit-test note/ขอบ/lane (pure) | `noteAt(pt) edgeAt(pt) laneAt(pt)` |
| `input/gestures.js` | pointer state machine → intent | `down(e) move(e) up(e) cancel()` |
| `input/tools.js` | intent → command JSON (รวม snap) — แยกจาก gestures เพื่อ test | `commit(intent, session) → Cmd[]` |
| `input/keys.js` | keyboard shortcuts | `attach(win)` |
| `ui/{topbar,trackstrip,toolbar,mixer,status,inspector,dialogs}.js` | DOM chrome ทีละแถบ | `attach(el, ...)` |

### ใช้ Rust ร่วม vs เขียนใหม่ใน JS

**ใช้ของเดิมใน Rust (ผ่าน wasm-bindgen):** `domain` (`UProject`, `UNote`, `UVoicePart`,
`UCurve`, expression descriptors, `TimeAxis`, `csharp_round` — ห้าม reimplement
banker's rounding ใน JS) · `commands` + `UndoStack` (**ต้องเพิ่ม wasm binding — วันนี้
`engine-wasm` ไม่ได้ depend `commands`**) · `ustx-codec` (wasm-verified) · `phonemizer`
(wasm-verified) · `feed`/`phrase`/`synth-cli::pipeline` (pitch grid, curves,
`plan_track_chunks`, `Chunk::hash`, `render_planned_chunk`) · `worldline-rs`+`world-rs` ·
`runtime::mixer` (seam crossfade + `db_to_linear` × `pan_gain` — mix ต้องผ่านตัวนี้
เท่านั้น ไม่งั้นไม่ตรง native) · `mixer-fx::MixerChain` (EQ/comp/clip pure Rust) ·
`wavwriter`

**เขียนใหม่ใน JS:** layered canvas renderer + dirty-rect · viewport math, hit-testing,
gesture state machine, snapping *เพื่อ preview* (ค่าที่ commit ยังผ่าน Rust) · worker
pool, per-chunk loading, chunk cache, epoch/cancel, progress · Web Audio playback (ใช้
`AudioContext.currentTime` เป็น source of truth แบบเดียวกับ native ที่ใช้
`onPositionChanged`) · OPFS I/O · DOM chrome ทั้งหมด · keyboard/pen arbitration/palm
rejection · **marquee multi-select** (ไม่มีใน Flutter เลย — `piano_roll.dart` มี
`Note? _selectedNote` ตัวเดียว)

### API ฝั่ง wasm ที่ต้องเพิ่ม

```ts
// project เป็นเจ้าของใน Rust (§6)
engine.project_new(); engine.project_load_yaml(y); engine.project_yaml();
engine.notes_json(track); engine.curves_json(track);          // view-model
engine.apply_command(json); engine.undo(); engine.redo();
engine.can_undo(); engine.can_redo();
engine.phonemize_json(track);                                 // ไม่มี server แล้ว
mixer.add_chunk_track(pcm, position_ms, leading_ms, volume_db, pan); // Mixer วันนี้ใช้ TrackSpec::default()
mixer.finish_wav();
engine.apply_mixfx(pcm, paramsJson);
engine.cancel_render();                                       // pipeline มี AtomicBool แต่ render_chunk ยังไม่ wire
```

มีอยู่แล้วและใช้ได้เลย: `plan_project()` · `chunk_meta_json()` · `chunk_wav_keys(i)` ·
`render_chunk(i)` · `render_planned_sequential()` · `plan_skipped()` · `Mixer.add_chunk()` ·
`Mixer.finish()` · `fft_backend()` · `warnings()` · `last_panic()`

---

## 5. Rendering & scheduling

### Plan และการ invalidate

1. project change → `scheduler.markDirty(track)` (debounce 500 ms)
2. `plan_project(yaml, track)` → `chunk_meta_json()`:
   `[{index, hash, position_ms, leading_ms, start_ms, end_ms, own_start, own_end}]`
3. `cacheKey = chunk.hash + "|" + curveDigest(own_start..own_end) + "|" + voicebankId + "|" + engineSalt`
4. hash เดิม → ย้าย PCM เดิมไป key ใหม่ (ไม่ render); hash ใหม่ → เข้าคิว;
   chunk ที่หายไป → ทิ้ง

> ⚠️ **`Chunk::hash()` ไม่ครอบคลุม expression/pitch curve** — `chunking.rs` hash แค่
> `phoneme, position_ms, duration_ms, tone, tone_shift` ของ `RenderPhoneme` แต่
> DYN/GENC/TENC/BREC/VOIC ถูก sample เป็น `feed::render_input::Curves` และ PITD เข้า
> pitch grid (`feed/src/pitch.rs`) ซึ่ง**ไม่ถูก hash** ⇒ ใช้ hash เดียวเป็น cache key
> แล้วแก้ curve จะเสิร์ฟ PCM เก่า (ผิดเงียบๆ). เลือก **(a) แนะนำ** แก้ `Chunk::hash`
> ฝั่ง Rust ให้ hash slices เหล่านั้นด้วย (ถูกที่ต้นทาง; worker ใช้ plan เดียวกันจึงไม่
> ต้องส่งอะไรเพิ่ม) **(b) ชั่วคราว** คำนวณ digest ของ curve ในช่วง chunk ฝั่ง JS

### Worker pool + per-chunk sample loading

ตัวเลขจาก `wasm-parallel.md` (voicebank จริง 558 wav / 198 MB):

| โหมด | worker พร้อมใน | render 4 chunk | RAM/worker |
|---|---|---|---|
| spawn ต่อ render + โหลดทั้ง bank | 625 ms | 3059 ms | ~198 MB |
| **pool + เฉพาะ chunk** | **32 ms** | **2159 ms** | ไม่กี่ MB |

→ **เลือก pool + per-chunk loading** (โหลดทั้ง bank × 4 workers ≈ 790 MB ไม่ผ่านบน Mi Pad)

```
workerCount = min(4, max(2, (navigator.hardwareConcurrency ?? 4) - 1))
worker i รับ chunk [i, i+W, i+2W, ...]
keys(i) = union(chunk_wav_keys(k) for k in chunks(i))   // ส่งเฉพาะ key ที่ worker ยังไม่มี
```

**main thread ไม่ต้องโหลด wav เลย**: `plan_project` + `chunk_wav_keys` ใช้แค่ alias
table จาก `oto.ini` (ไม่แตะ `MemorySource`) ⇒ `set_voicebank(id, name, otoDir, otoBytes)`
อย่างเดียวพอ. `web/engine.js` วันนี้มี `attachVoicebankFromOpfs()` ที่โหลด **ทุก** wav
เข้า engine — ต้องแยกเป็น `attachVoicebankMetaFromOpfs()` (oto only) แล้วส่ง bytes ให้
worker เป็นราย chunk; worker เรียก `add_sample_with_frq`. **frq จำเป็น** —
`wasm-target.md` วัดได้ `max|diff| = 1.588` ถ้าไม่ส่ง เทียบกับ `0.000e0` เมื่อส่ง.
OPFS อ่านฝั่ง main thread แล้ว `postMessage` แบบ transfer (chunk ละ 1–3 sample);
ทางเลือกขั้นสูงคือให้ worker อ่าน OPFS เอง — ลด copy แต่ต้อง feature-check (§9 ข้อ 6)

> **Implement แล้ว (2026-09-23):** `web/app/editor.js` สร้าง `RenderPool` ตอน
> boot ด้วย `workerCount = max(1, min(4, cores-1))`, เรียก `setProject()` +
> `render({signal, onProgress})` หลังทุกคำสั่งแก้, มี epoch guard กันผลของ render
> เก่ามาทับ, `cancelRender()` ผูกกับปุ่ม Stop, และ chunk overlay วาดสถานะจาก
> `pool.cache` จริง. ทางถอยคือ `driver.renderAll()` บน main thread เมื่อ Worker
> ใช้ไม่ได้. เทสต์: `./web/run-editor.sh` (แก้ 1 note → 1/6 chunk, ไม่แก้อะไร →
> 0/6 chunk ใน 2 ms, cancel → ผลถูกทิ้ง)

### Progress และสถานะ chunk

`chunk_meta_json()` ให้ `start_ms/end_ms/own_start/own_end` จริง → วาด chunk mark ตรง
ตำแหน่งจริง **แทนของปลอมใน mock** (`CHUNK_PX = 230` + hash จากการคูณ 2654435761).
state ต่อ mark: `pending` (เทา) → `rendering` (เหลือง + progress bar) → `ready` (เขียว) /
`stale` (ส้ม + hatch: PCM เก่า ยังเล่นได้แต่ไม่ตรง project) / `failed` (แดง, hover ดู
`last_panic()`). status bar: `● rendering chunk 2/4 · 1.4s · ETA 1.1s · cache 3/4` +
`↻ re-render` (บังคับ invalidate all) + ปุ่ม mute meter

### ระหว่าง render in flight — เลือกชัด: **ไม่ block**

| สถานการณ์ | พฤติกรรม |
|---|---|
| cold start (cache ว่าง) | Play disabled + "Rendering 0/4"; roll แก้ได้เต็มที่ |
| แก้ note หลังมี cache | chunk ที่เปลี่ยนเป็น `stale`/`pending`; **เล่น audio เดิมต่อได้** (วาด hatch ทับช่วง stale); export disabled |
| render เสร็จระหว่างเล่น | **ไม่สลับ AudioBuffer กลางเพลง** (จะกระตุก) — เตรียม buffer ใหม่ให้รอบถัดไป; กด ⏹ แล้ว ▶ ได้เสียงใหม่ทันที |
| แก้ระหว่าง render | bump `epoch`; ผลของ epoch เก่าถูกทิ้ง (แบบเดียวกับ `_client.renderEpoch` ใน `editor_screen.dart`) |
| cancel | `cancel_render()` → หยุดที่ phrase ถัดไป; ถ้ายังไม่ wire จะเสีย CPU ต่อได้ถึง ~2.1s/chunk |
| error | mark แดง, เก็บ PCM เก่า (ไม่ทำให้เงียบ), โชว์ panic message, disable export |

**ทำไมไม่ block:** incremental เร็วกว่า full render แค่ ~1.5x (`wasm-parallel.md`: 1.4s
vs 2.2s — chunk ที่หนักสุดพอๆ กับ render ทั้งเพลงพร้อมกัน). Block ทุกครั้งที่แก้ note =
หน่วง 1.4s ต่อการลากหนึ่งครั้ง; เล่นเสียงเก่าต่อทำให้ยัง "ร้องตาม" ได้ แลกกับต้องโชว์ชัด
ว่า stale. ฝั่ง native `editor_screen.dart` มีธง `_audioDirty` และ **ห้ามเล่น audio เก่า
เด็ดขาด** — web เลือกต่างโดยเจตนา (§9 ข้อ 5 มี toggle)

**งบหน่วยความจำ** (f32 mono 44.1k): 14.5s = 2.6 MB PCM + 2.6 MB AudioBuffer; 5 นาที =
53 + 53 MB. main thread ~0 (oto only); ต่อ worker ไม่กี่ MB. cache policy: LRU เพดาน
256 MB, เก็บ chunk ใกล้ playhead ±30s ไว้เสมอ

---

## 6. Project state & undo

**ข้อเท็จจริง:** `engine-wasm` รับ project เป็น **YAML text ต่อการเรียก**
(`render_project(ustx_yaml, track)`, `plan_project(ustx_yaml, track)`) — ไม่มี project
object ที่แก้ได้ และไม่มี undo. `domain` + `commands` (command ครบ + `UndoStack` ที่มี
group/cancel_group) มีอยู่แล้วใน Rust แต่ `engine-wasm` ไม่ได้ depend `commands` และ
`commands` **ไม่อยู่ในตาราง wasm-verified** ของ `wasm-target.md`. ฝั่ง Flutter undo/redo
ยังเป็น no-op (BACKLOG ระบุไว้)

### คำแนะนำ: Rust เป็นเจ้าของ document, JS เป็นเจ้าของ view-model

```
┌─ main-thread wasm instance ────────────────────────────────┐
│  UProject  ←(apply_command)→  commands::UndoStack          │
│      │                              ↑ undo()/redo()        │
│      └─ project_yaml() ──► JS (ส่งให้ worker ทุกตัว plan)   │
└────────────────────────────────────────────────────────────┘
                    │ notes_json(track) / curves_json(track)
                    ▼
        JS view-model (plain objects) — วาด + hit-test เท่านั้น
```

- ทุก gesture ที่ commit → **1 command JSON** → `apply_command` → wasm validate ก่อน
  mutate (ตามสัญญาของ `Command` trait) → คืน revision → JS `notes_json()` มา sync
  view-model
- **ระหว่างลากไม่ round-trip**: JS ทำ preview ใน view-model เอง, commit ครั้งเดียวตอน
  `pointerup` ⇒ 1 gesture = 1 command = 1 undo step (ตรงกับ OpenUtau ที่สร้าง command
  ตอนปล่อยเมาส์). multi-note ห่อ N command ด้วย `start_group`/`end_group`
- undo/redo = `engine.undo()`/`redo()` แล้ว re-sync view-model + `markDirty()`
- `UndoStack::cancel_group` ใช้เฉพาะกรณี abort drag กลางทาง (เช่น Esc ระหว่างลาก
  multi-select) ไม่ใช่ path หลัก

**ทำไม:** ไม่ต้อง reimplement domain semantics ใน JS เลย (tick 480, `csharp_round`,
`UCurve::INTERVAL = 5`, note validation, expression ranges) ซึ่งเป็นที่มาของ "UI กับ
engine ไม่ตรงกัน" ที่แพงที่สุดในการ debug — และ render ใช้ `UProject` ตัวเดียวกับที่ UI แก้

```jsonc
// command JSON (ร่าง)
{ "op": "move_note",   "part": 0, "note": 3, "pos_tick": 1920, "tone": 62 }
{ "op": "resize_note", "part": 0, "note": 3, "dur_tick": 480 }
{ "op": "add_note",    "part": 0, "pos_tick": 1440, "dur_tick": 240, "tone": 60, "lyric": "la" }
{ "op": "set_curve",   "part": 0, "abbr": "pitd", "xs": [0,5,10], "ys": [0,40,10] }
{ "op": "set_track",   "track": 1, "volume_db": -3.0, "pan": 0.3, "mute": false }
{ "op": "group",       "name": "Split note", "cmds": [ /* ... */ ] }
```

ฝั่ง Rust: `apply_command` เป็น factory `match op` ที่สร้าง `Box<dyn Command>` แล้วโยน
เข้า `UndoStack::execute` — ~150–250 บรรทัด. ถ้า `commands` wasm ไม่ได้จริง ทางถอยคือ
JS snapshot undo (เก็บ YAML ก่อน/หลังทุก gesture) — ง่ายแต่กิน RAM ~project size × N
และเสีย group semantics ไม่แนะนำ

**Persistence:** OPFS `projects/<name>.ustx` เขียนจาก `project_yaml()`; autosave 1500 ms
หลัง edit (ตาม `_scheduleAutosave`); ขอ `navigator.storage.persist()` ครั้งแรกที่บันทึก;
เตือนชัดว่า "ล้าง site data = project + voicebank หายหมด" และมี `Export .ustx`.
undo history ไม่ persist (เหมือน native)

---

## 7. Direct reuse vs rewrite

### 7.1 port จาก `ui-mock/index.html` (plain HTML/CSS/JS — ยกได้เลย)

**ยกทั้งก้อน:** โครง grid `.app` (rail 64 + topbar + main 3 คอลัมน์ + bottom transport) ·
palette (มาจาก OM DarkThemeColors จริง) + typography + ปุ่ม/chip/slider · trackstrip
(`.hdr-col` 236px, `.th-row` 44px, `.th-tag/.th-avatar/.th-info`, `.th-detail` V/P +
CVVC/WORLD chips + ↑🗑↓, `.th-mute`, `.add-track`, `.timeline` 22px, `.parts-area`,
`.part`) · mixer sheet (`.mixer-sheet` translateY, `.mixer-head` preset + fx-chips,
`.strip` 118px / `.strip.master` 132px, fader/pan-knob/eq-knob/comp-meter และสูตรที่
ผูกกันเป๊ะ `faderPct = (db+60)/72`, `panRot = pan*135`, `eqAng = ((db+12)/24)*270`) ·
toolbar + right-tools (Rate/Snap/Zoom) · `.float-tools` · FX overlay semantics
(`.chunkmark`, `.gr`, `.fx-hot`) · `.portrait`

**ไม่ยก:** JS ที่สร้าง `.note` เป็น DOM element ต่อ note (`setupNote` ผูก
`pointerdown/move/up` ทีละตัว) — ใช้กับเพลงร้อย note ไม่ได้; เปลี่ยนเป็น canvas โดย port
behavior เดิม (select → drag → snap Y 32px, erase = `n.remove()`, pencil สร้าง note ที่
snap X 40px)

**แก้ของ mock ที่ผิด/หลอก:** `renderFxLayer()` สร้าง chunk mark ทุก 230px พร้อม hash
ปลอมจาก `(x*2654435761>>>0)` และ `renderCurve()` วาด curve จาก "ความยาว lyric % 5" →
แทนด้วยข้อมูลจริง (`chunk_meta_json()`, `curves_json()`). `#zoomX.oninput` ที่เอา
`parseInt(style.left)*value/90` มาคูณ note ทีละตัวเป็น bug (ซ้ำซ้อนเมื่อเลื่อนสไลเดอร์
กลับ) → ใช้ viewport transform

### 7.2 port *พฤติกรรม* จาก `piano_roll.dart` (Dart — reuse โค้ดไม่ได้)

ไฟล์นี้ **สะอาดเรื่อง platform** (import แค่ `dart:async`, `dart:math`,
`flutter/foundation.dart`, `flutter/material.dart`, `../models.dart`, `../theme.dart` —
ไม่มี `dart:io`, ไม่มี MethodChannel) แต่เป็น Dart + Flutter widget tree จึงยกมาใน web
build ไม่ได้แม้บรรทัดเดียว — port ได้แค่ logic/ค่าคงที่:

| port | จาก | หมายเหตุ |
|---|---|---|
| tool enum + select sub-mode, geometry | `PianoRollTool`, `SelectTarget`, §2 | ชื่อ/ลำดับ/ค่าตรงกัน |
| anchored zoom | `_applyZoom(nx, ny, viewportPoint)` | เก็บ content point ใต้ focal ให้นิ่ง |
| pencil upsert | `_pencilWrite` | snap capture grid + blend 30% + clamp ±4 semitone |
| curve anchor ผูกกับ note | `_syncNoteAnchors` | จำเป็นต่อ UX ตอนย้าย note |
| resize ขอบขวา + min 0.25 beat | `_onPointerMove` | web ขยายเป็น 10/16px ตาม pointer type |
| long-press 550ms → menu, tap → lyric dialog | `_showNoteContextMenu`, `_editLyric` | ต้อง clear `phoneme` เมื่อ lyric เปลี่ยน (bug ที่แก้แล้ว: label ค้าง) |
| Catmull-Rom curve | `_CurvePainter` | 8 steps/segment, glow 7px + line 2.2px, fill 10% |
| phoneme labels เหนือ note | `_RollPainter` | boxH = 46% ของ note, กล่องดำ + ขอบขาว 25%, กระจายเต็มความกว้าง |
| FX overlay / z-order 4 layer | `_FxOverlayPainter`, `build()` Stack | dashed chunk line + GR curve + hot ring |
| playhead จาก audio position จริง | `playbackPositions` stream | web ใช้ `AudioContext.currentTime` |
| "ห้ามเล่น audio เก่า" | `_audioDirty` | web ผ่อนคลายโดยเจตนา (§5) |

**ยังไม่มีใน Flutter → เขียนใหม่:** box-select/multi-selection · split (Flutter เป็น stub
ที่แค่ select) · expression lanes + curve ต่อ lane · multi-track mix · chunk cache/progress
UI · OPFS · Web Audio

### 7.3 เขียนใหม่ทั้งหมด

`view/{rollcanvas,hit}.js` · `input/{gestures,tools}.js` · `render/{scheduler,pool,chunkcache,player}.js` ·
`store/{voicebank,projects}.js` · `ui/*.js` · pen arbitration · marquee · expression lanes ·
chunk-state overlay

---

## 8. Implementation phases

ขนาดเป็นวันทำงานของ 1 engineer

| เฟส | สถานะ 2026-09-12 | สิ่งที่ทำแล้ว / gap หลัก |
|---|---|---|
| **P0** | 🟡 เกือบครบ | `commands` compile WASM; project/load/YAML, command group, undo/redo, notes/curves/tracks API และ parity tests ผ่าน. `phonemize_json` ยังไม่มี |
| **P1** | 🟡 เกือบครบ | Shell/CSS, `Driver`, transport/play, HTTP fixture bank, **OPFS project store + autosave + export/import** และ worker production path ต่อแล้ว (2026-09-23); เหลือ voicebank import เข้า OPFS |
| **P2** | 🟡 Vertical slice | Canvas roll, viewport, hit-test, Select/Draw/Move/Resize/Erase/Split, snap และ pointer handling มีแล้ว; lyric/phoneme/playhead/pen hardware QA ยังไม่ครบ |
| **P3** | ✅ 2026-09-23 | `RenderPool` ต่อเข้า editor แล้ว (epoch/progress/cancel + chunk state + main-thread fallback); `run-editor.sh` ยืนยัน incremental 1/6 chunk |
| **P4** | ⬜ | Curve API มีฐานและ chunk hash ครอบ pitch/curves แล้ว; PITD overlay/expression lanes ยังไม่ implement |
| **P5** | 🟡 UI only | Track list + mute/solo commands มี แต่ render/mix ยังเป็น track เดียวและ `TrackSpec::default()` |
| **P6** | 🟡 บางส่วน | Marquee, split, inspector, keyboard shortcuts และ undo/redo UI มี; context menu/polish ยังเหลือ |
| **P7** | 🟡 2026-09-23 | autosave + project browser + export/import `.ustx` ทำแล้ว; เหลือ export WAV จาก editor, service worker/PWA |
| **P8** | ⬜ | ยังไม่ผ่าน tablet DPR/palm-rejection/perf/memory/i18n/error-state QA |

**รวม ~24–35 วัน.** P0 เป็น dependency ของ P1–P6; P3 ควรมาก่อน P4 เพราะ chunk cache ทำให้
การวาด curve (invalidate บ่อย) ไม่ทรมาน. จุดที่ควรหยุดให้คนดู: หลัง P3 (แก้ note +
re-render เฉพาะ chunk = "wow" แรก) และหลัง P6 (editor ใช้ทำงานได้จริง)

---

## 9. Open questions / risks

1. ✅ **`commands` compile wasm ได้แล้ว** — `engine-wasm` project command tests
   ครอบ load/round-trip/edit/group/undo/redo/track commands และ render parity.
2. **panic ฆ่า engine ทั้งตัว** — wasm เป็น `panic = "abort"`; `last_panic()` ดักข้อความ
   ได้ แต่ instance ตายแล้ว ⇒ `apply_command` ที่ panic = **project + cache หายหมด**.
   ต้อง validate ใน factory, เก็บ YAML snapshot ไว้ rebuild, เลี่ยง `unwrap`
3. ✅ **cache key ครอบ curve/pitch แล้ว** — `Chunk::hash` hash window ของ PITD และ
   expression curves พร้อม regression tests; pool incremental ยัง bit-identical.
4. **multi-track ยังไม่มี API** — `Mixer.add_chunk` ใช้ `TrackSpec::default()` (vol 0,
   pan 0) ⇒ mute/solo/fader ยังไม่มีผล. ต้องเพิ่ม `add_chunk_track` + `finish_wav` + mix
   หลาย track; ทางเลือกคือ Web Audio node ต่อ track (ได้ fader ทันทีแต่ memory ×N และต้อง
   แปลง f32 → AudioBuffer เอง) — แนะนำ wasm เพื่อ bit-identical
5. **นโยบาย stale audio** — เอกสารนี้เลือก "ไม่ block + hatch" ต่างจาก native ที่
   `_audioDirty` ห้ามเล่นของเก่าเด็ดขาด. ต้องให้คนตัดสิน (อาจทำเป็น setting)
6. **Safari** — ไม่มี directory picker (fallback zip import), OPFS-in-worker ต้อง
   feature-check, `OffscreenCanvas` ต่างจาก Chrome. ต้องตัดสินว่าเป็น target ระดับไหน
7. **memory บน Mi Pad** — per-chunk loading ลด RAM worker แล้ว แต่ mix buffer +
   AudioBuffer ของเพลงยาวยังกิน (5 นาที ≈ 53 MB ×2) ต้องวัดจริงก่อนสรุป cache cap
8. **ปากกาด้ามลบ Android** — ไม่ยืนยันว่า Mi Pad ส่ง `pointerType === 'eraser'` หรือ
   `pen` + `buttons & 32`; ถ้าไม่ส่งเลยต้องพึ่ง long-press/toolbar. ทดสอบกับเครื่องจริง
9. **phoneme labels** — native ได้จาก server `/phonemize`; web ต้องเพิ่ม
   `phonemize_json()` (phonemizer wasm-verified แล้ว) หรือ fallback แยก letter จาก lyric
   + เทียบ alias set ใน JS (แบบ `phonemesForLabel`). แนะนำ wasm เป็น authoritative
10. **"แก้ note แล้วดังช้า"** — incremental เร็วกว่า full แค่ ~1.5x; ต้องตัดสินว่า UX ที่
    รับได้คือ hatch + status หรือต้องมี "preview note เดียว" แบบ blocking
11. ✅ **`.ustx` render round-trip ผ่าน** — `project_load_yaml → project_yaml`
    render bit-identical กับ input fixture; ยังควรเพิ่ม fixture coverage สำหรับ field
    ที่ UI ยังไม่ expose.
12. **PWA / offline** — เป็น product target ไหม (service worker + แคช wasm 1.43 MB /
    508 KB gzip); ถ้าใช่ต้องมี versioning ของ engine asset

## ภาคผนวก — gate ถัดไป

```
[x] P0: `commands` + `engine-wasm` project/command/undo APIs compile และ test ผ่าน
[x] P0: command edit render = YAML edit render (bit-identical)
[x] curve/pitch อยู่ใน chunk hash + regression tests
[ ] เพิ่ม authoritative `phonemize_json`
[x] wire `web/app/editor.js` → `RenderPool` + progress/cancel/epoch (2026-09-23)
[ ] ตัดสิน multi-track mix API และ stale-audio policy
[x] project autosave/import/export ใน OPFS (`app/project-store.js` + `projects/<name>.ustx`) — 2026-09-23
[ ] voicebank import เข้า `OpfsBank` + ทดสอบ offline/service worker
```
