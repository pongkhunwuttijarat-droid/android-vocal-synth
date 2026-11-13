# CONTRACT-v1 — UI ↔ Engine Data Contract (Freeze)

> **สถานะ:** v1 legacy contract frozen 2026-08-02 · implementation update 2026-09-12
> **สถาปัตยกรรมปัจจุบัน:** Flutter/Android ใช้ bundled `synth-server` ผ่าน loopback HTTP;
> browser ใช้ `engine-wasm` in-process. JNI embedded ยังเป็นทางเลือกถัดไป ไม่ใช่ path
> ที่แอปใช้อยู่ตอนนี้.
> **Project Session logical standard (2026-09-30):** ดู §6 `Project Session API (v2 logical contract — frozen)`
> — เป็น logical API ที่ transport adapter ของ native HTTP, WASM และอนาคต JNI ต้องให้ semantics
> เหมือนกัน; ไม่ได้หมายความว่าต้อง serialize ผ่าน HTTP ใน Web. §6 freeze logical schema และ
> semantics แล้ว โดยไม่รับรอง implementation readiness; §0–§5
> ยังเป็น legacy CONTRACT-v1 ที่ไม่ถูกเปลี่ยน.
> **สถานะ (2026-09-30):** logical contract §6 freeze แล้ว; implementation และ adapter conformance ยังตามหลังใน T004/T011. นี่ไม่ใช่การรับรองว่า v2 product release พร้อมใช้งาน. `part.*`/`project.*` เป็นคำสั่ง required, child failure ใช้ `group_failed`, native polling เป็น baseline และ WASM async session render unavailable จนกว่าจะมี worker adapter.
> **DTO encoding:** every Project Session logical request/result/error/capability DTO has a canonical JSON representation on every host. Native transports it as HTTP+JSON; WASM calls `Engine` directly and maps to/from the same JSON DTO shape, without HTTP. Function names/call signatures may differ; JSON field names/types/enums may not.
> อ้างอิง: `docs/architecture/data-contracts.md` (engine internals), `native/tools/synth-server/`

---

## 0. Transport Strategy (สำคัญ)

| โหมด | Transport | ใช้เมื่อ | สถานะ |
|---|---|---|---|
| **Dev** | HTTP synth-server (`http://<host>:18080`) | desktop dev, LAN test บน Mi Pad, golden | ✅ พร้อม |
| **Android app (current)** | bundled synth-server + `http://127.0.0.1:18080` | แอปจริงบน Android | ✅ |
| **Web (current)** | `engine-wasm` + JS/Canvas | static browser editor | 🟡 vertical slice |
| **Future native** | JNI embedded (libvoicesynth.so + Kotlin bridge) | ลด process/HTTP overhead | ⬜ |

**กฎ UI:** โค้ด UI ทั้งหมดต้องคุยกับ `EngineClient` interface เท่านั้น — transport (HTTP/JNI) swap ได้โดยไม่แตะ UI
- Flutter v1 ใช้ `ApiClient`/HTTP ทั้งบน desktop และ Android; Android จะสตาร์ต
  bundled server ใน `EngineProcess.kt` และ bind loopback.
- Settings server URL ยังมีไว้สำหรับ dev/LAN override; ไม่ได้แปลว่า app ต้องมี
  external server เสมอ.
- Web ไม่ใช้ transport นี้: ใช้ wasm-bindgen adapter ใน process เดียว.

## 1. Transport

| Item | Value |
|---|---|
| Protocol | HTTP/1.1 JSON (wav = raw bytes) |
| Base URL | `http://<host>:18080` (configurable ใน Settings) |
| Timeout | 10s (health), 300s (render; client default) |
| Auth | ไม่มีใน v1 (LAN only) |
| Error | `{"error": "..."}` + HTTP status (400/404/500) |

## 2. Endpoints (v1 — engine side เสร็จแล้ว)

| Method | Path | Request | Response |
|---|---|---|---|
| GET | `/health` | — | `{status:"ok", version, so_loaded}` |
| GET | `/capabilities` | — | `{modes[], expressions[], needs_oto, needs_wav_samples, needs_frq, channels, sample_rate}` |
| GET | `/voicebanks` | — | `{voicebanks:[{dir, name, aliases_count, wav_count, samples_rate}]}` |
| POST | `/synth-note` | `{voicebank, phoneme, tone, duration_ms}` | **audio/wav** bytes (หรือ `{error}`) |
| POST | `/render` | `{project, voicebank, engine?}` | **audio/wav** bytes (หรือ `{error}`) |
| GET | `/stats` | — | `{renders_count, total_ms, cache_hits}` |

**ตัวอย่าง /render request:**
```json
{"project": "/abs/path/song.ustx", "voicebank": "library"}
```

## 3. UI View Models (Dart — `app/lib/models.dart`)

| Dart model | Source | Fields |
|---|---|---|
| `Note` | local (legacy editor state) | lyric, pitch, position (beats), duration (beats), phoneme |
| `Track` | local (legacy editor state) | name, colorSeed, notes[] |
| `Voicebank` | **GET /voicebanks** | name←dir, format, status="ready", sizeMb←wav_count |
| `PitchPoint` | local (legacy editor state) | beat, semitones |

## 4. Wire Rules (UI side)

1. **ทุก screen รับ `EngineClient` interface** (inject ผ่าน constructor, default = `ApiClient`) — ห้าม import http/network ใน screen โดยตรง
2. **Voicebanks screen** โหลดจาก `/voicebanks` จริง — **fallback เป็น mock list ถ้า server offline** (UI options ห้ามหาย)
3. **Editor render/play/export** → `POST /render` → wav bytes → แสดงสถานะ + ขนาด + duration และเล่นผ่าน `audioplayers`; server รองรับ cancel/post-FX ด้วย
4. **Settings** มี field server URL + ปุ่ม "Test connection" (GET /health) — **ติด label "Dev mode"** ให้ชัดว่าไม่ใช่ production
5. **Project state** อยู่ใน editor และ save/load ผ่าน server-side `.ustx` codec/store; export/import เต็มรูปแบบยังขึ้นกับ host (Android SAF และ Web OPFS เป็นงาน product)
6. ทุก network call ต้องมี error state ที่ user เห็น (SnackBar/status text) — ห้าม crash

## 5. Out of Scope / follow-up

- จังหวะ piano roll ↔ server (ยัง local)
- per-track renderer selection ใน Flutter UI
- Android JNI direct call (ลด child-process/HTTP overhead)
- Web OPFS project browser, multi-track mix, expression lanes และ PWA
- neural renderer บน WASM

---
*Breaking changes ของ legacy endpoints ต้อง bump เป็น CONTRACT-v2 + แจ้งทั้งสองฝั่ง*

## 6. Project Session API (v2 logical contract — frozen 2026-09-30)

> **เป้าหมาย:** Rust ถือเอกสารโปรเจกต์เป็น source of truth เดียว ทุก frontend ส่ง semantic
> commands และแสดง authoritative read models. นี่คือ **logical API**: ทุก adapter ใช้
> semantics/schema เดียวกัน แต่ transport ต่างกันได้
>
> - Flutter/native: versioned HTTP ไปยัง draft `/api/v2/...` routes.
> - Browser: WASM facade เรียก `project-session` ใน process โดยตรง (ไม่ผ่าน HTTP).
> - อนาคต JNI/Tauri: native facade ครอบ session semantics ชุดเดียวกัน.
>
> **สถานะ:** logical contract นี้ freeze แล้ว; implementation gaps ด้านล่างเป็นงานตามให้ตรงสเปก ไม่ใช่ข้อกำกวมของสัญญา.
>
> **สิ่งที่ implement แล้ว (2026-09-29, `native/crates/project-session`):**
> §6.2 ทั้งหมด — `EntityId` (opaque UUIDv4, Rust-mint, seed inject ได้), fingerprint ต่อ kind,
> `IdentityTable` + sidecar `rusted-utau.ids/1` (`table_to_json`/`table_from_json`), matching
> Pass A/A2/B + orphan tombstone + repair, `fork_for_save_as`; §6.3 snapshot DTO พร้อม ID;
> §6.9 เฉพาะ command/undo/redo + save (revision เริ่ม 0, +1 ต่อ committed change,
> rejected/no-op/save ไม่ขยับ) รวมถึง `ids_stable` + warnings; **§6.4 envelope + result**
> (`expected_revision` บังคับ, `revision_conflict`, `include_snapshot`), **§6.4.3 change
> vocabulary** พร้อมกฎ `authority: patch|invalidate` (patch ⇒ มี data, invalidate ⇒ ไม่มี),
> **§6.14 typed error codes**, **คำสั่งที่อ้าง stable ID** (`note.add/remove/set_lyric/
> set_tone/move/resize`, `track.set_mute/set_solo/set_volume_db/set_pan/rename` — translate
> ไปเป็นคำสั่ง index เดิมเพื่อให้ validation/tick rules/undo เป็นชุดเดียวกัน),
> **§6.10 bounded reads** (notes ของ part, curve ตาม `curve_id`, track metadata — tag
> revision ทุกตัว) และ **lazy snapshot** (`ProjectDoc` เลื่อน serialize จนมีคนขอ;
> เดิมจ่าย 38.4 ms ทุกคำสั่งบน badapple). หลักฐาน: identity 14 + session 16 + v2 16 +
> curve-repair 7 tests และ `.ustx` ไม่มี identity.
>
> **implement เพิ่มในรอบล่าสุด (2026-09-29):** **group transactions (§6.8)** ครบทุก
> transition — tentative แยกจาก committed, child ไม่ขยับ revision และไม่คืน `changes`,
> fail-closed ด้วย `group_failed`, commit = หนึ่ง undo step + revision +1, cancel คืนค่าเดิมเป๊ะ;
> **`curve.apply_stroke` (§6.6)** ทั้ง mode `replace` และ `clear` พร้อม canonicalization ใน
> Rust (sort / snap 5 tick / duplicate last-wins / clamp) และ rejection reasons ที่
> machine-readable; **§6.15 ฝั่ง session** — `capabilities()` ประกาศ versions/features/limits
> และ **บังคับจริง** (request เกิน `max_command_bytes` ถูกปฏิเสธก่อน parse, stroke เกิน
> `max_stroke_points` ถูกปฏิเสธก่อนแตะเอกสาร); **§6.17 conformance vectors** ชุดกลางที่รันกับ
> `project-session` ได้แล้ว (14 vectors + runner, อ้าง entity ด้วย `@note:0` ฯลฯ).
>
> **สิ่งที่ยังไม่ implement (อย่าเข้าใจว่ามีแล้ว):**
> - ฝั่ง WASM ยังไม่เปิด async render job; `project-session` capability จึงรายงาน `async_jobs: false` และ `per_job_cancel: false`. Native server มี render jobs/poll/cancel แยกตาม `render_id` แล้ว
> - §6.10 bounded reads และ §6.4.4 commands ใช้ semantics กลางเดียวกัน; vectors ครอบ metadata/part group ผ่าน reference/native/WASM, stale/group/history/render invalidation ของ session vectors และ split curve-boundary ผ่านทั้งสาม adapter. Shared `project_part_edge_shared.json` now exercises 13 representative project/part invalid/no-op cases across reference, native HTTP, and direct WASM, including revision/document/history atomicity; it is not an exhaustive enumeration of every possible malformed payload or boundary value.
> - WASM/browser host ยังต้องเชื่อม sidecar file I/O กับ OPFS; shared crate ผลิต/อ่าน sidecar payload แต่ไม่เลือก host storage
> **Legacy:** §0–§5 และ index-addressed command facade ของ `engine-wasm`
> (`add_note`/`move_note`/`set_track_*` โดยใช้ `part`/`note` index) ยังคง behavior เดิม
> ไม่ถูกนิยามใหม่. Draft นี้เป็นสัญญาใหม่สำหรับ cross-host session เท่านั้น.

### 6.1 Ownership, invariants, and non-goals

1. Rust เป็นเจ้าของ persistent project/musical state, validation, mutations, undo/redo,
   canonical USTX, save/export/render input และ render-affecting semantics.
2. Frontend เป็น UI-only client. **ระหว่าง gesture ที่ยังไม่ commit** frontend เป็นเจ้าของ
   transient display-only state (drag delta, resize delta, raw stroke samples, selection
   rectangle, handles, pointer trajectory). ห้ามใช้ state นี้กับ save/export/render/validation
   หรือสร้าง canonical project.
3. Ordinary pointer move ไม่ต้องส่ง mutation ให้ Rust. Flow มาตรฐาน:
   `pointer down → frontend transient preview → local redraw → pointer up → one semantic Rust command → reconcile`.
4. Full snapshot เป็น authoritative read DTO; cache ใน frontend ต้อง disposable และ resync ได้.
5. Target ใหม่ใช้ stable opaque IDs (§6.2). `expected_revision` ใช้กับทุก public state-changing
   operation (รายละเอียด §6.9).
6. Save/render/export ผูกกับ committed revision ที่ระบุ; render result/progress ต้อง tag
   revision และห้ามถูก publish เป็น "current audio" แบบเงียบ ๆ ถ้า revision ไม่ตรง.
7. **Non-goals ของ v2:** cloud sync, multi-user sessions, durable server sessions, server-side
   cross-process recovery, plugin-defined commands, และ UI layout state ใน session.

### 6.2 Entity identity (FROZEN)

#### 6.2.1 ขนาดของปัญหา (หลักฐาน)

- OpenUtau USTX model **ไม่มี** id หรือ extension bag ที่ใดเลย: `UProject`, `UTrack`,
  `UPart`/`UVoicePart`/`UWavePart`, `UNote`, `UCurve` ไม่มี field ลักษณะ id
  (`ref(openutau+openutau mobile)/core/OpenUtau.Core/Ustx/*.cs`).
- Identity ปัจจุบันเป็น index: `project-session::notes_json` ส่ง `"index"`
  (`native/crates/project-session/src/lib.rs:204-225`) และคำสั่งใช้ `part`/`note` index
  (`:117-119`).
- `UNote` ที่มี `position` เท่ากันถูกเรียงด้วย `GetHashCode()` ใน `SortedSet<UNote>`
  (`UNote.cs:62-72`, `UPart.cs:44`) — เพราะฉะนั้น **index ไม่ใช่ durable identity**
  แม้แต่ภายใน OpenUtau เอง.

#### 6.2.2 ตัวเลือกที่ทดสอบแล้ว

**Option 1 (ฝัง key เพิ่มใน .ustx): ปฏิเสธบนหลักฐาน.**
OpenUtau ตั้ง `IgnoreUnmatchedProperties()` ฝั่ง deserializer
(`Util/Yaml.cs:24-27`) แต่ serializer ของ YamlDotNet เขียนเฉพาะ member ของ object graph
จึงไม่มีการ preserve ข้อมูลส่วนเกิน. การทดลองกับ `OpenUtau.Core.dll` ที่รีโปมีอยู่จริง
(`test/golden/golden-renderer/bin/Release/net8.0/OpenUtau.Core.dll`, mirror ขั้นตอน
`Ustx.Save`: Deserialize → AfterLoad → BeforeSave → Serialize → AfterSave) ให้ผล:
deserialize ผ่าน (tolerate) แต่ **key ที่ไม่รู้จักถูกลบทิ้งทุกกรณี** ทั้ง project, track,
voice_part, note, curve และ wave_part. ฝั่ง Rust เองก็เหมือนกัน: ไม่มี
`deny_unknown_fields`/`flatten` ที่ไหนในรีโป และ `to_yaml`
(`project-session/src/lib.rs:295-299`) serialize จาก domain structs ตรง ๆ → key ที่ไม่ได้
declare จะหายเมื่อเราบันทึกเอง. มี test ยืนยันแค่ครึ่งเดียวคือ *อ่านได้*
(`native/crates/domain/tests/round_trip.rs:229 unknown_keys_are_ignored`) ไม่ได้ยืนยันว่าคงอยู่.
หลักฐานภาคสนาม: `resolution: 1` ที่ Dart เขียน (`app/lib/editor_ustx.dart:59`) ปรากฏในไฟล์
`*-app.ustx`/`demo-song.ustx` แต่ **หายไป** ในคู่ `*-backup.ustx` ที่เขียนด้วยสไตล์ YamlDotNet.
> "Rust อ่านได้" ≠ "OpenUtau บันทึกกลับให้". การฝัง key จะทำให้ identity หายเงียบ ๆ
> เมื่อผู้ใช้เปิดแล้ว save ใน OpenUtau/OpenUtau Mobile.

**Option 3 (reconstruction ล้วน): ไม่พอ.** ใช้เป็น fallback ได้ แต่ two notes ที่
position/duration/tone/lyric เหมือนกันแยกไม่ออกด้วย content และ identity จะไม่ข้าม
การ reload.

**Option 2 (sidecar): เลือกใช้.** ไฟล์แยกที่ OpenUtau ไม่เห็น ไม่ parse และไม่เขียนทับ →
เป็นทางเดียวที่ lossless ข้าม OpenUtau round-trip.

> หมายเหตุ: OpenUtau preserve ได้เฉพาะ `project.comment`, `part.comment`, key ใน
> `expressions` map, `UExpressionDescriptor.options[]` และ `phoneme_expressions` ที่ abbr
> ลงทะเบียนแล้ว (ค่าฝั่ง note เป็น float) — ไม่มีช่องเดียวที่ครอบ project+track+part+note+curve
> ด้วย opaque string. ใช้เป็น hint ได้เท่านั้น และ comment เป็นข้อความที่ผู้ใช้แก้ได้.
> **ห้ามเพิ่ม undeclared key ลง `.ustx`.**

#### 6.2.3 ID representation (FROZEN)

| ข้อ | ค่า |
|---|---|
| ชนิด | opaque string บน wire |
| รูปแบบ | RFC 4122 v4 UUID ตัวพิมพ์เล็ก มีขีด (36 ตัวอักษร) |
| ชนิด entity | `project_id`, `track_id`, `part_id`, `note_id`, `curve_id` |
| ผู้สร้าง | **Rust เท่านั้น**; frontend ห้าม generate/แก้/ตีความ |
| ตัวสร้างภายใน | 128-bit random (ต้อง inject ได้เพื่อ deterministic test) |
| ลงใน `.ustx` | **ไม่** — USTX ยัง OpenUtau-compatible 100% |
| ลงใน DTO | ใช่: snapshot, mutation request/result, partial reads, sidecar |

`curve_id` **บังคับ** เพราะ `UVoicePart.curves: Vec<UCurve>` ไม่มีอะไรรับประกันว่า
`(part_id, abbr)` ไม่ซ้ำ (ดู §6.6.1) — lookup ทั้งหมดเป็น first-match
(`phrase/src/builder.rs:140,235`, `commands/src/exp_commands.rs:80,102,169,189`) และ
delete เป็น `retain(|c| c.abbr != …)` ซึ่งลบทุกตัวที่ตรง (`:118,204`).

**ที่เก็บ ID ภายใน (ตัดสินตอน implement — FROZEN):** ID **ไม่ได้** เก็บเป็น field บน domain
structs; `UProject`/`UTrack`/`UVoicePart`/`UNote`/`UCurve` ยังเหมือนเดิมทุก byte. session ถือ
`IdentityTable` แล้ว **อนุมาน** ID ด้วย matching ใน §6.2.4. เหตุผลเชิงบังคับ:
`commands/src/locate.rs` หา note ด้วย `UNote == UNote` (`locate.rs:87,91`) — การเพิ่ม field
`id` ที่มีผลกับ derived `PartialEq` จะทำให้ `execute`/`unexecute` หา note ไม่เจอทันทีที่
session mint ID หลัง construction; ทางเลือกอื่นคือเขียน `PartialEq` เองทุก struct.
ผลที่ตามมาและยอมรับไว้: entity ที่เหมือนกันทุกประการสองตัวจะถูกจับคู่ตามลำดับ (แยกไม่ออกจริง)
และ ID คือ *identity ที่อนุมานได้* ไม่ใช่ field ที่แนบมากับ object.

#### 6.2.4 Sidecar: ตำแหน่ง รูปแบบ และ matching (FROZEN)

**ตำแหน่ง/ชื่อ:** ไฟล์พี่น้องของ `.ustx` ชื่อ `<project-file-name>.rusted-utau-ids.json`
→ native: `<projects_root>/<name>.ustx.rusted-utau-ids.json`
(`native/tools/synth-server/src/projects.rs:8,60-62`)
→ web: OPFS `projects/<name>.ustx.rusted-utau-ids.json`.

**ผู้เขียน (FROZEN 2026-09-29):** เขียนผ่าน **session v2 save path เท่านั้น** ในรอบนี้.
Rust session เป็นเจ้าของเนื้อหา (`save_sidecar()`); host storage adapter (server filesystem /
OPFS) เป็นเจ้าของ I/O และเขียนแบบ atomic (temp + rename). **Legacy `POST /project`
endpoint ไม่ถูกแก้ semantics** — ยัง backward-compatible ตาม CONTRACT-v1 และไม่เขียน
sidecar เงียบ ๆ; การย้าย legacy path ไปเขียน sidecar เป็นงานแยกที่มี versioning ชัดเจน.

**รูปแบบ (schema v1 — ตรงกับ `project_session::identity` ที่ implement แล้ว):**

```json
{
  "schema": "rusted-utau.ids/1",
  "project_id": "8f14e45f-ea0a-4b1c-9d3a-2b7c6e5f0a91",
  "document_fingerprint": "9c1d4e0b77a2f3aa",
  "tracks": [
    {"id": "…", "fingerprint": "…",
     "parts": [
       {"id": "…", "fingerprint": "…",
        "notes":  [{"id": "…", "fingerprint": "…"}],
        "curves": [{"id": "…", "abbr": "pitd", "fingerprint": "…"}]}
     ]}
  ],
  "orphans": [
    {"kind": "note", "id": "…", "fingerprint": "…"}
  ]
}
```

- Rust ผลิต **เนื้อหา** JSON (`table_to_json`/`table_from_json`); host adapter เป็นเจ้าของ
  การเขียนไฟล์ — identity module ไม่ทำ file I/O.
- `document_fingerprint` = `xxh64` hex (`xxhash-rust 0.8`, feature `xxh64` — crate เดียวกับ
  render cache) ของ **canonical USTX text** ที่ Rust serialize; เป็น `null`/หายได้จนกว่าจะ save
  (`set_document_fingerprint`) เพื่อไม่ให้เส้นทาง hot ต้อง serialize ทุกคำสั่ง.
- `fingerprint` ต่อ entity = `xxh64` hex ของ field ชุดที่ตรึงไว้ (`id_schema_version = 1`):
  track = `track_name|singer|phonemizer`; part = `name|track_no|position`;
  note = `position|duration|tone|lyric`; curve = `abbr` + จุดทั้งหมด.
  หลักการ: เลือก field ที่เป็น *ดนตรี/เนื้อหา* ไม่ใช่ index และไม่รวม field ที่เครื่องมืออื่น
  recompute (เช่น `UVoicePart.duration` ที่ OpenUtau `AfterLoad` เขียนใหม่ 480→960).
  `mute/solo/volume/pan/track_color` จงใจไม่อยู่ใน fingerprint (เป็น mix/cosmetic state).
- `orphans`: entry ที่โหลดมาแล้วไม่ match — **เก็บไว้** (tombstone) เพื่อไม่ให้ ID churn
  เมื่อเอกสารถูกแก้ภายนอกหลายรอบ; ล้างได้เมื่อ save สำเร็จแล้วไม่มี entry นั้นจริง.

**Matching algorithm (deterministic, ต้องเหมือนกันทุก host) — implement แล้ว:**

1. **Pass A — content:** จับคู่ entry ที่อยู่ใต้ parent เดียวกันและ `fingerprint` เท่ากัน
   (curve ต้อง `abbr` ตรงด้วย) โดยไล่ตามลำดับใน list ทั้งสองฝั่ง (entry ที่ใช้แล้วถูก consume).
   → ทนต่อการ reorder/move.
2. **Pass A2 — descriptor (curve):** entry ที่ยังเหลือ จับคู่ด้วย `abbr` ที่ตรงกัน.
   → ทนต่อการวาด stroke ที่เปลี่ยนจุดจน fingerprint เปลี่ยน.
3. **Pass B — position:** entry ที่ยังเหลือ จับคู่ตามลำดับ (list order) ภายใน parent เดียวกัน.
   → ทนต่อการแก้เนื้อหา (rename track, แก้ tone note, ลากโน้ต).
4. Entity ที่ยังไม่ถูกจับคู่ → Rust **mint ID ใหม่**; candidate ที่ชนกับ ID ที่ถืออยู่แล้ว
   (รวม orphan) จะถูกข้ามเพื่อไม่ให้ ID ซ้ำในเอกสาร.
5. Entry ที่ยังไม่ถูกใช้และมี ID ถูกต้อง → ย้ายเข้า `orphans`; entry ที่ ID ผิดรูปถูกทิ้ง
   พร้อม warning.

หมายเหตุ: สอง entity ที่แยกไม่ออกด้วย content (note ซ้ำเหมือนกันเป๊ะ) จะถูกจับคู่ตามลำดับ
ซึ่งเป็นพฤติกรรมที่ deterministic และยอมรับได้เพราะแยกไม่ออกอยู่แล้ว.

**เมื่อใช้ sidecar ไม่ได้** (ไฟล์หาย / JSON เสีย / `schema` ไม่รู้จัก / version สูงกว่า):
- **ไม่ทำให้ open ล้มเหลว.** Mint ID ใหม่ให้ทุก entity (session reconstruction), บันทึก
  warning `ids_regenerated`, และไม่ลบ sidecar เดิมจนกว่าจะ save สำเร็จ.
- นี่คือ compatibility fallback ที่ตั้งใจ: ID จะไม่ข้าม reload ในกรณีนี้
  (ดู `ids_stable: false` ใน §6.11).

**Fingerprint field set — validated against the real OpenUtau (2026-09-29).**
รัน probe ที่ mirror ขั้นตอน `Ustx.Save` (Deserialize → AfterLoad → BeforeSave → Serialize
→ AfterSave) กับ `OpenUtau.Core.dll` จริงในรีโป บนโปรเจกต์ที่ใช้ field ครบ
(2 tracks, 2 parts, 3 notes, 1 curve): **ไม่มี field ที่ใช้ใน fingerprint เปลี่ยนเลย**
มีเพียง `part.duration` (960→1440) ที่เปลี่ยน ซึ่ง **จงใจไม่อยู่** ใน fingerprint ของ part.
→ ใช้ `id_schema_version = 1` เป็น implementation baseline ได้.
**หลัง schema ถูกใช้งานจริงแล้ว ห้ามแก้ algorithm/field set เงียบ ๆ — ต้อง bump
`id_schema_version`** (sidecar เก่าจะถูกปฏิเสธแล้ว mint ใหม่ ซึ่งเป็นพฤติกรรมที่ยอมรับได้).

**Duplicate/malformed ID repair (FROZEN, deterministic — implement แล้วใน `reconcile`):**
- ID ซ้ำภายใน sidecar → entry แรกที่เจอในลำดับ file คง ID ไว้, entry ที่ซ้ำถัดมาได้ ID ใหม่;
  warning `duplicate_id_repaired` (พร้อม `count`).
- ID ผิดรูป/ผิดชนิด → entry นั้นไม่ได้ ID เดิม แต่ entity ยังอยู่และได้ ID ใหม่ (entity ไม่หาย);
  warning `malformed_id_entry` (พร้อม `count`).
- Entry ที่มี ID ถูกต้องแต่ไม่ match entity ใดเลย → ย้ายเข้า `orphans`; warning
  `orphaned_entries` (พร้อม `count`).
- Sidecar ที่ `schema` ไม่รู้จัก → `table_from_json` คืน Err; caller ถือว่า "ใช้ sidecar ไม่ได้"
  แล้วเดินเส้นทาง fallback (mint ทั้งหมด + `ids_regenerated`) โดย **ไม่ลบ** sidecar เดิม.
- ID ที่อยู่ในคำสั่งแต่ไม่มีใน session → error `not_found`, ไม่มี mutation.
- ID ทั้งชุดถูกตรวจรูป (`8-4-4-4-12` hex ตัวพิมพ์เล็ก) ตอน adopt; candidate ที่ชนกับ ID ที่
  ถืออยู่แล้วจะถูกข้าม (จึงไม่มี ID ซ้ำในเอกสารที่ reconcile แล้ว).

#### 6.2.5 Load-time repairs (FROZEN 2026-09-29)

การโหลดต้องไม่ล้มเพราะข้อมูลที่ซ่อมได้ และ frontend ต้องไม่ต้องซ่อมเอง. Rust ทำ
deterministic repair ต่อไปนี้และรายงานเป็น warning code:

| สภาพในไฟล์ | Rust ทำอะไร | warning code |
|---|---|---|
| ไม่มี key `tracks` หรือ `tracks: []` | สร้าง `Track1` แบบ OpenUtau (constructor default) | `missing_tracks_repaired` |
| voice part อ้าง `track_no` ที่ไม่มี | **เก็บข้อมูลไว้** และรายงาน (ไม่สร้าง track เกินที่ตัดสินไว้) | `part_references_missing_track` |
| ไม่มี `time_signatures`/`tempos` | คืน default 4/4 + 120 BPM ของ OpenUtau | (ไม่รายงาน — จำเป็นต่อ `TimeAxis`) |
| `len(curve.xs) != len(curve.ys)` | ตัดให้เท่าฝั่งสั้น (เดิม panic ใน `UCurve::sample`) | `repaired_curve_arrays` |
| curve `abbr` ไม่ตรง descriptor | เก็บไว้ + รายงาน | `unregistered_curve_abbr` |
| curve `abbr` ซ้ำใน part เดียว | เก็บไว้ทั้งคู่ + รายงาน (ห้ามลบข้อมูลผู้ใช้) | `duplicate_curve_abbr` |

หลักฐานสำหรับ tracks: OpenUtau `UProject()` seed `Track1` ไว้ และ YamlDotNet คง default
นั้นเมื่อ key หาย (probe: 1 track หลัง deserialize) แต่ `tracks: []` ที่ระบุชัดทำให้
**OpenUtau เอง save ไม่ได้** (`BeforeSave` index `tracks[part.trackNo]` แล้ว throw) —
การซ่อมทั้งสองกรณีจึงทำให้ไฟล์ใช้งานได้จริงและตรงกับเจตนาของผู้ใช้.

#### 6.2.6 กฎการได้มา/คงอยู่ของ ID (FROZEN)

| เหตุการณ์ | พฤติกรรม |
|---|---|
| Session create (`new`) | mint `project_id` + ID ของ entity เริ่มต้น ทั้งหมดใน Rust |
| Open stored project | โหลด `.ustx` + sidecar ผ่าน matching §6.2.4 |
| Import / replace document | **mint ID ใหม่ทั้งหมด**, history reset, revision +1 (§6.9) — ไม่ใช้ sidecar ของไฟล์ต้นทาง |
| New object (add note/track/part/curve) | mint ID ใหม่ใน Rust เสมอ |
| Copy / paste / duplicate | **mint ID ใหม่เสมอ**; ห้ามคัดลอก ID (กัน ID ซ้ำ) |
| Move / reorder / resize / edit เนื้อหา | ID คงเดิม |
| Undo/redo | ID คงเดิม รวมถึงเมื่อ undo การสร้างแล้ว redo กลับมา (ต้องได้ `curve_id`/`note_id` เดิม) |
| Save | เขียน `.ustx` + sidecar (ID table ปัจจุบัน) |
| Save As | **mint `project_id` ใหม่**, เขียน sidecar ใหม่ให้ชื่อใหม่; **entity IDs คงเดิม** (เนื้อหาเดิม) — เป็นเอกสารใหม่แต่ประวัติ entity ต่อเนื่อง |
| Export `.ustx` (portable) | ไม่พา ID ไป (ตั้งใจ) — ไฟล์ที่แชร์จะได้ ID ใหม่เมื่อ import (ตรงกับ §6.2.6 import) |
| Session close แล้ว reopen ด้วยชื่อเดิม | ได้ ID เดิมถ้า sidecar ยังใช้ได้ |

### 6.3 Snapshot DTO (FROZEN schema v1)

Snapshot เป็น read-only DTO; canonical file ยังเป็น USTX ที่ Rust serialize:

```json
{
  "session_id": "opaque-session-token",
  "revision": 12,
  "can_undo": true,
  "can_redo": false,
  "ids_stable": true,
  "project": {
    "id": "uuid",
    "name": "scale-demo",
    "bpm": 110,
    "time_signature": {"numerator": 4, "denominator": 4},
    "tracks": [{
      "id": "uuid",
      "name": "Lead Vocal",
      "singer_id": "teto",
      "mute": false, "solo": false,
      "volume_db": 0.0, "pan": 0.0,
      "parts": [{
        "id": "uuid",
        "position_ticks": 0,
        "duration_ticks": 1920,
        "notes": [{"id":"uuid","position_ticks":0,"duration_ticks":480,"tone":60,"lyric":"la"}],
        "curves": [{"id":"uuid","abbr":"pitd","points":[{"tick":0,"value":0}]}]
      }]
    }]
  }
}
```

- **Tick semantics (FROZEN):** `position_ticks`/`duration_ticks` ของ note และ `tick` ของ curve
  point เป็น **part-relative ticks** ตาม domain (1 quarter = 480 ticks; `RESOLUTION`).
  `part.position_ticks` เป็น project ticks. Frontend แปลง display time เองได้ แต่ Rust เป็น
  ผู้ตัดสิน validity/rounding.
- **Ordering:** notes และ curve points เรียงตาม tick จาก Rust; client ห้ามถือว่า index คือ identity.
- **Curve `abbr`** เป็น lowercase ตาม descriptor (`expression.rs:189,246`); `curve_id` เป็น identity จริง.
- `ids_stable: false` หมายถึง session นี้ reconstruct ID (sidecar ใช้ไม่ได้) — client ต้องไม่
  cache ID ข้าม session ในกรณีนี้.
- `snapshot` ไม่ใช่ payload บังคับของทุก mutation (§6.4).

#### 6.3.1 Snapshot schema v2 (FROZEN)

The frozen-schema-v1 example above documents the current projection. The complete first-release
project/part contract needs these additional canonical fields; `snapshot_schema_version: 2`
retains v1 fields as deprecated read-only aliases:

- Project: `comment`, `key`, complete sorted `tempos:[{position_tick,bpm}]` and
  `time_signatures:[{bar_position,numerator,denominator}]` maps. Existing `bpm` and
  `time_signature` remain derived tick-zero/bar-zero convenience fields.
- Part: `kind` (`voice` or `wave`), `name`, `comment`, existing `position_ticks` and
  `duration_ticks`, plus notes/curves. A wave part exposes a logical `media_ref`; it never
  exposes an absolute host filesystem path. Host adapters resolve the reference to their own
  storage.
- All arrays use canonical domain order; identity continues to use opaque IDs, never indices.

`ProjectMetaView` in `project.changed` is exactly the project metadata/timing fields above,
excluding tracks. `PartView` in `part.*` changes is the part fields above with child notes and
curves when `authority:"patch"`; otherwise the change must invalidate and permit a
revision-tagged part read. This frozen schema is not a claim that the
current `snapshot_schema_version: 1` implementation emits it.

### 6.4 Mutation request, result, and change vocabulary

#### 6.4.1 Request

```json
{
  "expected_revision": 12,
  "command": {"type": "note.set_lyric", "note_id": "uuid", "lyric": "ka"},
  "include_snapshot": false,
  "group_id": null
}
```

- `expected_revision` บังคับสำหรับ state-changing operation (§6.9).
- `include_snapshot` เป็น hint; adapter อาจไม่ให้ snapshot ถ้าไม่ถูกขอ (§6.10).
- **`unexpected_revision`/stale → `revision_conflict`** และไม่เปลี่ยนอะไรเลย.

#### 6.4.2 Result

ทุก successful state-changing operation คืน **อย่างน้อย**:

```json
{
  "revision": 13,
  "command_name": "Change lyric",
  "changed": true,
  "can_undo": true,
  "can_redo": false,
  "changes": [
    {"kind":"note.changed","track_id":"uuid","part_id":"uuid","note_id":"uuid",
     "authority":"invalidate"}
  ]
}
```

- `snapshot` เป็น optional field ในผลลัพธ์เดียวกัน (เมื่อขอหรือเมื่อ adapter เลือกให้).
- **`changes` ต้องพอให้ client รู้ว่า read model ไหน stale.**
- ถ้า client patch cache ไม่ได้อย่างปลอดภัย → ขอ bounded read (§6.10) หรือ full snapshot.

#### 6.4.3 Change vocabulary (FROZEN, ปิด)

ทุก entry มี `kind` + `authority`. `authority` เป็นตัวบอกว่า payload สมบูรณ์หรือไม่:

| `authority` | ความหมาย |
|---|---|
| `"patch"` | entry มี canonical entity data **ครบ** สำหรับ entity นั้น (client patch cache ได้ทันที) |
| `"invalidate"` | entry บอกแค่ว่าอะไร stale; client ต้องขอ read model ก่อนแสดง/ใช้ |

ห้าม entry ที่ client เดาไม่ได้ว่า data ครบหรือไม่: ถ้ามี field `data` ต้องเป็น `patch` เท่านั้น.

| `kind` | required IDs | optional tick range | `data` (patch) |
|---|---|---|---|
| `project.changed` | — | — | canonical `ProjectMetaView` (see §6.4.4) |
| `track.added` | `track_id` | — | track metadata + empty parts |
| `track.removed` | `track_id` | — | — |
| `track.changed` | `track_id` | — | track metadata (name/singer/mute/solo/volume/pan) |
| `part.added` | `track_id`, `part_id` | — | canonical `PartView` + notes/curves |
| `part.removed` | `track_id`, `part_id` | — | — |
| `part.changed` | `track_id`, `part_id` | `start_tick`,`end_tick` (part-relative) | canonical `PartView` or invalidate |
| `note.added` | `part_id`, `note_id` | — | note |
| `note.removed` | `part_id`, `note_id` | — | — |
| `note.changed` | `part_id`, `note_id` | — | note |
| `curve.changed` | `part_id`, `curve_id`, `abbr` | `start_tick`,`end_tick` (part-relative) | canonical curve points (หรือ `curve.removed` ถ้าลบทั้งเส้น) |
| `history.changed` | — | — | `{can_undo, can_redo}` |
| `render.invalidated` | — | `start_tick`,`end_tick` (project ticks) | `{reason}` |

- `curve.changed` ต้องคืน canonical points **หลัง normalize** เมื่อ `authority: "patch"`
  (นี่คือกลไกที่ frontend เอา preview ไปแทนที่ด้วยผลจริง).
- `render.invalidated` ใช้เมื่อ committed change กระทบ render input (curve/pitch/edit
  ทั้งหมดที่ chunk hash ครอบ — `native/tools/synth-cli/src/chunking.rs:141-153`).
  Client ไม่ต้อง re-render เอง, แค่รู้ว่า audio ปัจจุบัน stale.
- `undo`/`redo` ที่ไม่มีอะไรให้ทำ → `changed: false` และ `changes: []` (ไม่มี `history.changed`
  ถ้า can_undo/can_redo ไม่เปลี่ยน).
- **ห้ามขยาย vocabulary นี้โดยไม่แก้ contract**; frontend ห้ามตีความ event ภายใน engine.

สำหรับ first required `part.*`/`project.*` commands: `project.changed.data` เป็น
canonical `ProjectMetaView` (name/comment/key + tempo map + meter map); `part.added.data`
เป็น canonical `PartView`. `part.changed` ใช้ `PartView` patch ได้เมื่อส่ง child data
ครบ มิฉะนั้นต้องเป็น invalidate แล้ว client เรียก bounded `part` read. `part.split` ส่ง
ทั้ง change ของ part เดิมและ part ใหม่ หรือ invalidate ทั้งสอง ID.

#### 6.4.4 Project and part mutation commands (FROZEN, required in v2)

คำสั่งในหัวข้อนี้เป็น **logical command names** ไม่ใช่ route หรือชื่อ binding.
ทุกคำสั่งส่งผ่าน envelope §6.4.1 พร้อม stable IDs; `expected_revision` บังคับ,
validation/apply ต้อง atomic, คำสั่งที่เปลี่ยน document เพิ่ม revision หนึ่งครั้งและเป็น
undo/redo หนึ่ง step. ใน group ใช้ child semantics §6.8 และเพิ่ม revision เฉพาะตอน commit.
ไม่มี-op ต้องคืน `changed:false` และไม่เพิ่ม revision/history.

| `command.type` | Fields | Semantics |
|---|---|---|
| `project.set_metadata` | `fields` object: optional `name`, `comment`, `key` | เปลี่ยนเฉพาะ fields ที่ส่ง; `name` ต้องไม่ว่าง, `key` อยู่ใน 0..11, comment อนุญาต empty string. Project identity ไม่เปลี่ยน |
| `project.set_tempo` | `position_tick`, `bpm` | Insert หรือแทน tempo ณ tick นั้น; tick ต้องไม่ติดลบ, BPM ต้อง finite และ > 0. ต้องมี tempo ที่ tick 0; position ซ้ำคือแทน entry เดิม |
| `project.remove_tempo` | `position_tick` | ลบ tempo change ที่ tick นั้น; ลบ tempo ที่ tick 0 ไม่ได้; ID ไม่มีเพราะ tempo เป็น timeline entry ไม่ใช่ entity |
| `project.set_time_signature` | `bar_position`, `numerator`, `denominator` | Insert หรือแทน meter ณ bar นั้น; bar ไม่ติดลบ, numerator/denominator เป็นจำนวนเต็มบวก และคำนวณ ticks-per-bar ได้ตรงจำนวนเต็มโดยไม่ overflow; ต้องมี entry ที่ bar 0 |
| `project.remove_time_signature` | `bar_position` | ลบ meter change ที่ bar นั้น; ลบ entry bar 0 ไม่ได้ |
| `part.create` | `track_id`, `kind`, `name`, `position_ticks`, `duration_ticks`; wave ต้องมี `media_ref` | สร้าง part ว่างบน track ที่ระบุ; engine mint `part_id`; name ไม่ว่าง, position ≥ 0, duration > 0. `kind:"voice"` มี notes/curves ว่าง. `kind:"wave"` ใช้ project-relative media reference ตาม §6.3.1; ห้ามส่ง host filesystem path |
| `part.remove` | `part_id` | ลบ part และข้อมูลภายในจาก committed document; undo ต้องคืนข้อมูลและ IDs เดิมครบ |
| `part.move` | `part_id`, `position_ticks` | ย้าย part บน project timeline โดยไม่เปลี่ยน relative note/curve ticks; position ไม่ติดลบ |
| `part.resize` | `part_id`, `duration_ticks` | เปลี่ยน extent โดยไม่ clip/ลบ note หรือ curve; ถ้าค่าใหม่ตัด content ที่มีอยู่ให้ reject ทั้งคำสั่ง |
| `part.set_metadata` | `part_id`, `fields` object: optional `name`, `comment` | เปลี่ยนเฉพาะ metadata ที่ส่ง; name ต้องไม่ว่าง; comment รับ empty string |
| `part.set_media_ref` | `part_id`, `media_ref` | เปลี่ยน source reference ของ wave part เท่านั้น; voice part ไม่รองรับ; reference ต้องเป็น non-empty project-relative/host asset key และห้าม absolute path หรือ `..`; เปลี่ยน render input และทำให้ render stale |
| `part.split` | `part_id`, `at_tick` | แยก voice part ณ part-relative tick ภายใน `(0,duration)`; part เดิมคง ID ฝั่งซ้าย, ฝั่งขวาได้ ID ใหม่ และ `position = old.position + at_tick`; notes ที่อยู่ขวาถูกย้ายและ rebase; note ที่คร่อมจุดแบ่งทำให้ reject atomic เพื่อไม่ split note โดยเงียบ. Wave part split ไม่อยู่ใน first scope |

`part.create` และ `part.remove` คืน `part.added`/`part.removed`; move/resize/metadata/media-ref คืน
`part.changed`; split คืน `part.changed` สำหรับ part เดิมและ `part.added` สำหรับ part ใหม่.
Project edits คืน `project.changed`. Patch data ต้องเป็น canonical view ครบตาม
`authority:"patch"`; ถ้า adapter ไม่มีข้อมูลครบให้ส่ง `invalidate` และ bounded read.
Tempo/meter และการเพิ่ม/ย้าย/resize/ลบ part ทำให้ render input stale จึงคืน
`render.invalidated`; เปลี่ยนชื่อ/comment/key ไม่ invalidate audio เว้นแต่การ render model
ของ Rust ระบุว่ามีผล. Undo/redo ต้องคืน project/session view และ IDs ที่ถูกต้อง.

รายละเอียด field names, edge cases และ patch DTO ถูกปิดใน §6.20. รายการนี้เป็นสัญญาที่
adapters ต้องทำตาม ไม่ใช่คำกล่าวว่าคำสั่งมี implementation แล้ว. `part.split` อยู่ใน v2;
track reordering และการแก้ voicebank reference อยู่นอกขอบเขต v2.

ตัวอย่าง request (illustrative logical shape):

```json
{"expected_revision":12,"command":{"type":"project.set_tempo","position_tick":1920,"bpm":100.0}}
```

```json
{"expected_revision":13,"command":{"type":"part.split","part_id":"part-opaque-id","at_tick":1920}}
```

Part commands ใช้ `position_ticks` และ `duration_ticks` เป็น project/part ticks ตาม
ชื่อ field; `part.split.at_tick` เป็น tick ภายใน part. หลัง split part เดิมเก็บช่วงซ้ายและ
ID เดิม, part ใหม่เริ่มที่ `old.position + at_tick`, duration เป็นส่วนที่เหลือ. Note ที่
คร่อมจุด split ปฏิเสธทั้งคำสั่ง. Curve ถูกแบ่งที่ boundary โดย sample linear interpolation
ด้วยกฎ round ของ domain; left side คง curve ID เดิม ส่วน right side ได้ curve ID ใหม่.
การ undo คืน part, curve และ IDs ก่อน split ครบถ้วน.

### 6.5 High-frequency gesture model (FROZEN)

สถานการณ์เป้าหมาย: ลากโน้ต ปรับความยาว วาด curve ด้วยปากกา → เกิด pointer event หลายร้อยครั้ง.

1. **ระหว่าง gesture:** frontend วาด preview ต่อ frame/input โดยใช้ transient state ที่ทิ้งได้ เช่น drag delta, resize delta, raw curve samples, selection rectangle, handles และ pointer trajectory. ยังไม่มี persistent mutation และ preview ห้ามใช้เป็น save/export/render input.
2. **ตอน pointer-up:** ส่งหนึ่ง semantic commit (`note.move` / `note.resize` / bulk command / `curve.apply_stroke`). ไม่มีข้อกำหนดให้ส่ง request ต่อ pointer event.
3. **Curve payload reduction (optional):** client MAY ลดจำนวน raw samples ก่อนส่งเพื่อลด traffic ก็ต่อเมื่อมี authoritative committed curve data ที่ revision ตรงกับ `expected_revision` และการลดนั้นผ่าน simplification vectors/tolerance เดียวกับ §6.7.2. ต้อง normalize base กับช่วง stroke ตาม §6.6.2 ก่อน simplify ทั้งเส้น. ถ้า base revision ไม่ตรง, ไม่มี base curve ที่ authoritative หรือ client reduction ยังไม่ผ่าน parity tests ให้ส่ง captured points โดยไม่ลด ภายใน limits ที่ประกาศ. การลดฝั่ง client เป็น transport optimization เท่านั้น ไม่ใช่ canonical state, ไม่เพิ่ม revision/undo และห้ามใช้แทน Rust validation/commit.
4. Rust ตรวจ `expected_revision`/stable IDs, validate, canonicalize/merge/simplify ตาม §6.6.2/§6.7.2 แบบ atomic, commit หนึ่ง undo step และเพิ่ม revision หนึ่งครั้ง; response/read ที่คืนจาก Rust คือผล canonical authoritative.
5. Frontend ทิ้ง buffer/preview หลังรับ commit result แล้ว reconcile จาก `changes` patch หรือ bounded read ที่ revision-tagged. ถ้า commit ล้มเหลว/stale ให้ทิ้งหรือคงไว้เป็น preview ที่ยังไม่ commitอย่างชัดเจน ห้ามนำไป persist/render.
6. `save`/`export`/`render` ระหว่าง gesture ใช้ committed Rust state เท่านั้น. Audio ที่กำลังเล่นอาจเล่นต่อ แต่ห้ามแสดงว่าเป็นผลของ preview.
7. Throttle การวาด preview ได้ตามต้องการ. Multi-entity gesture ใช้ bulk command หรือ committed group (§6.8) ให้ all-or-nothing และหนึ่ง undo step; อย่าใช้ group ส่งทุก raw pointer sample.

### 6.6 Curve semantics and `curve.apply_stroke` v2 (FROZEN)

#### 6.6.1 Authoritative representation (ตาม domain จริง)

| หัวข้อ | ค่า (หลักฐาน) |
|---|---|
| โครงสร้าง | `UCurve { xs: Vec<i32>, ys: Vec<i32>, abbr: String }` — parallel arrays (`domain/src/curve.rs:15-23`) |
| tick domain | **part-relative ticks**; 1 quarter = 480 (`domain/src/lib.rs:56`) |
| value domain | `i32` ในหน่วยของ descriptor (cents/dB/0-100) |
| interval/snap | `UCurve::INTERVAL = 5` ticks; snapping เกิดเฉพาะใน `set()` (`curve.rs:27,90-91`) |
| rounding | `csharp_round` = half-to-even (`domain/src/lib.rs:66-78`) — **`pub(crate)`**, ต้องเพิ่ม public helper ก่อนใช้จาก `commands` |
| ordering | xs เรียงจากน้อยไปมากเป็น **soft invariant**: `insert` รักษาลำดับ (`:76-84`) แต่ `SetCurvePointsCommand` ไม่ตรวจ (`exp_commands.rs:196-197`) |
| duplicate x | `insert` upsert ที่ x เดิม (`:78-80`) — ไม่เกิด xs ซ้ำถ้าใช้ insert |
| len(xs) == len(ys) | **ไม่ถูกตรวจที่ไหนเลย**; `sample` อ่าน `ys[idx]` หลังเช็คแค่ `xs.is_empty()` → YAML ที่ xs ยาวกว่า ys **panic** (`curve.rs:43-49`) |
| interpolation | linear ระหว่างจุด + ปัด half-to-even (`:50-55`) |
| นอกช่วงจุด | `sample` คืน `None` (`:56`) — **ต่างจาก C#** ที่คืน descriptor default (`UCurve.cs:66`); caller ต้อง apply default เอง (ทำอยู่ที่ `feed/src/curve.rs:31-35`) |
| `is_empty` | ไม่มีจุด **หรือ** ys เป็น 0 ทั้งหมด (`:34-36`) — ใช้ตัดสินว่าข้าม pitd (`feed/src/pitch.rs:205`) |
| descriptor | `UProject.expressions: HashMap<abbr, UExpressionDescriptor>`; `type: Curve`; `min/max/default_value: f32`; effective default = `custom_default_value()` (`expression.rs:261-263`); abbr ถูก lower-case (`:189,246`); **first registration wins** (`project.rs:348-352`) |
| value clamping precedent | `SetCurveCommand` clamp เป็น `(min as i32, max as i32)` เฉพาะค่าที่แก้ (`exp_commands.rs:35-40,109-110`) |
| uniqueness `(part, abbr)` | **ไม่รับประกัน**; `UVoicePart.curves: Vec<UCurve>` (`part.rs:46`); lookup เป็น first-match; delete เป็น `retain` ลบทุกตัวที่ abbr ตรง (`exp_commands.rs:118,204`) → ต้องใช้ `curve_id` |
| track-level Curve override | **ไม่ถูกใช้โดย render** — `phrase/src/builder.rs:228` วนจาก `project.expressions` เท่านั้น |
| simplify/merge | `Simplify()` มีใน OpenUtau reference และถูกเรียกจาก `SetCurveCommand.Merge()` หลัง merge gesture commands (`UCurve.cs:120-161`, `Commands/ExpCommands.cs:363-375`); **ยังไม่มีใน Rust** (`domain/src/curve.rs:4-6`). v2 `curve.apply_stroke` จะใช้ deterministic OpenUtau-compatible simplification ตาม §6.7.2 ที่ขอบเขต commit gesture; generic `MergeCurves` ยังอยู่นอก scope |
| render matching | curve-type expressions ใช้ part curves; curve ที่ไม่มี → default ทุกจุด (`builder.rs:249-260`); pitd อ่านตรงโดย `PitchComputer` ด้วย `unwrap_or(0)` (`feed/src/pitch.rs:203-216`) |
| cache impact | chunk hash ครอบ curve arrays (`synth-cli/src/chunking.rs:141-153`) |

**ค่า default ของ curve ที่ ship มา** (`expression.rs:316-337`):

| abbr | min | max | default | ปลายทาง |
|---|---|---|---|---|
| `dyn` | -240 | 120 | 0 | `curves.dynamics` |
| `pitd` | -1200 | 1200 | 0 | `PitchComputer` (cents) |
| `genc` | -100 | 100 | 0 | `curves.gender` |
| `brec` | -100 | 100 | 0 | `curves.breathiness` |
| `shfc` | -1200 | 1200 | 0 | `curves.extra` |
| `tenc` | -100 | 100 | 0 | `curves.tension` |
| `voic` | 0 | 100 | **100** | `curves.voicing` |

**Load-time repair ที่ v2 บังคับ (กัน panic + กัน data เพี้ยน):**
- `len(xs) != len(ys)` → ตัดให้เท่า `min(len)` และบันทึก warning `repaired_curve_arrays`
  (ไม่ reject ทั้งโปรเจกต์, ไม่ panic).
- curve ที่ `abbr` ไม่ตรง descriptor ใด (case-sensitive mismatch เช่น `DYN`) → เก็บไว้ แต่
  warning `unregistered_curve_abbr` (ปัจจุบัน curve นั้นไม่ถูก sample เลย).
- curve ที่ `abbr` ซ้ำภายใน part เดียว → เก็บไว้ทั้งหมด (ห้ามลบข้อมูลผู้ใช้), warning
  `duplicate_curve_abbr`, และทุกคำสั่งต้องอ้างด้วย `curve_id`.

#### 6.6.2 `curve.apply_stroke` — ขอบเขต v2 (FROZEN)

**หลักการ:** v2 freeze แค่ primitive เดียว = "แทนที่เนื้อในช่วงหนึ่ง" โดยมี 2 mode ที่ใช้
helper ที่มีอยู่จริงใน domain. Additive/blend/curve-shape interpolation เป็นงานอนาคต.

```json
{
  "type": "curve.apply_stroke",
  "part_id": "uuid",
  "curve_id": "uuid",
  "abbr": "pitd",
  "create": false,
  "mode": "replace",
  "start_tick": 1200,
  "end_tick": 2880,
  "points": [{"tick": 1205, "value": 12}, {"tick": 1210, "value": 18}]
}
```

| field | บังคับ | กฎ (FROZEN) |
|---|---|---|
| `part_id` | ใช่ | ต้องเป็น voice part ที่มีอยู่ใน session → ไม่งั้น `not_found` |
| `curve_id` | ใช่เมื่ออ้าง curve ที่มีอยู่ | ถ้า `create: true` ต้องเป็น `null`; Rust mint ID ใหม่ |
| `abbr` | ใช่เมื่อ `create: true` | ต้องเป็น descriptor `type: Curve` ที่ลงทะเบียนใน project → ไม่งั้น `validation_error`/`unsupported_curve_descriptor`. เมื่ออ้าง curve ที่มีอยู่ ถ้าส่งมาต้องตรงกับ abbr ของ curve นั้น → ไม่งั้น `validation_error`/`abbr_mismatch` (กันยิงผิดเส้น) |
| `create` | ไม่ (default false) | `create: true` + part มี curve abbr นั้นอยู่แล้ว → `validation_error`/`duplicate_curve_descriptor` (กัน duplicate ที่ render จะ shadow) |
| `mode` | ใช่ | v2 รองรับ **`"replace"`** และ **`"clear"`** เท่านั้น; ค่าอื่น → `unsupported_operation` |
| `start_tick`/`end_tick` | ใช่ | ต้อง `0 <= start_tick < end_tick <= part.duration_ticks` (part-relative) → ไม่งั้น `validation_error`/`tick_out_of_range` (reject, ไม่ clamp) |
| `points` | `replace` ≥1, `clear` = 0 | โครง `{tick, value}` (object form เท่านั้นใน v2; ห้าม parallel arrays). `clear` ที่มี points → `validation_error`; `replace` ที่ points ว่าง → `validation_error` |
| raw point `tick` | — | ต้องอยู่ภายใน `[start_tick, end_tick]` → ไม่งั้น `validation_error`/`stroke_point_out_of_range` (reject; ไม่ drop เงียบ ๆ) |
| `value` | — | **clamp** เข้า `[min as i32, max as i32]` ของ descriptor (ตาม precedent `SetCurveCommand`); จำนวนที่ถูก clamp รายงานในผลลัพธ์ |

**Canonicalization (FROZEN, deterministic, ทำใน Rust):**

1. เรียง points ด้วย `(raw_tick, original_index)` จากน้อยไปมาก (stable).
2. snap: `tick = round_half_even(tick / 5) * 5` (semantics เดียวกับ `UCurve::set`) แล้ว clamp
   เข้า `[start_tick, end_tick]`. ต้องเพิ่ม public helper ใน `domain` เพราะ `csharp_round`
   เป็น `pub(crate)` วันนี้.
3. tick ที่ snap แล้วซ้ำกัน → **ตัวหลังสุดชนะ** (ตรงกับ upsert semantics ของ `insert`).
4. clamp `value` เข้า `[min as i32, max as i32]`.
5. ผลลัพธ์ต้องมี `xs` เรียงเพิ่มแบบ strictly increasing และ `len(xs) == len(ys)`.

**การแทนที่ (interval semantics FROZEN):**

- `mode: "replace"`:
  1. อ่านค่าขอบเขต **ก่อนแก้**: `left = curve.sample(start_tick)`,
     `right = curve.sample(end_tick)`, โดย fallback เป็น effective default ของ descriptor
     (`sample(...).unwrap_or(default as i32)`) — ตรงกับ C# `Sample` และกับ
     `feed/src/curve.rs:31-35` **ไม่ใช่** `unwrap_or(0)` แบบ `UCurve::set`.
  2. ลบจุดที่อยู่ **strictly between** `start_tick` และ `end_tick`
     (`delete_between_exclusive`, `curve.rs:114-121`).
  3. ใส่จุดขอบเขตกลับถ้ายังไม่มี: `start_tick = left`, `end_tick = right` → จุดตัดกับข้อมูลเดิม
     ถูก preserve (continuity) แบบ deterministic.
  4. ใส่ points ที่ canonicalize แล้ว (ขั้น 1-4 ด้านบน).
- `mode: "clear"`:
  1. ลบจุดทั้งหมดในช่วง **inclusive** `[start_tick, end_tick]`
     (เหตุผล: "reset ช่วงนี้" ต้องลบขอบเขตด้วย ไม่งั้นเหลือจุดค้าง).
  2. ถ้า curve กลายเป็น `is_empty()` → **ลบ curve นั้นออก** (รายงาน `curve.changed` ที่
     มี `removed: true` หรือ `curve.removed` ตาม vocabulary §6.4.3).
  3. ไม่แตะ curve อื่นใน part เดียวกัน (บังคับด้วย `curve_id` ไม่ใช่ `abbr`).
- หมายเหตุความไม่สมมาตรของขอบเขต (exclusive สำหรับ `replace`, inclusive สำหรับ `clear`)
  เป็น **เจตนาและถูก freeze** พร้อมเหตุผลข้างต้น.

**Undo data (FROZEN):** `part_id` + `curve_id` + `abbr` + `Option<(Vec<i32>,Vec<i32>)>` ของ curve
ก่อนแก้ + `existed: bool`. Unexecute: ถ้า `existed == false` → ลบ curve นั้น **ด้วย
`curve_id` เท่านั้น** (ห้าม `retain(|c| c.abbr == …)` ซึ่งลบทุกเส้นที่ abbr ตรง); ถ้า
`existed == true` → คืน `xs`/`ys` เดิมแบบ byte-exact. Redo ต้องคืน `curve_id` เดิมเสมอ.

**ผลลัพธ์:** `curve.changed` ที่มี `part_id`, `curve_id`, `abbr`, `start_tick`, `end_tick`,
`authority: "patch"` และ canonical points (หรือ invalidation ถ้า adapter เลือกไม่ฝัง data)
+ metadata `clamped_values: <n>`, `points_written: <n>` + `render.invalidated` สำหรับช่วงที่กระทบ
(แปลงเป็น project ticks).

**OpenUtau gesture boundary (หลักฐาน):** `SetCurveCommand` ถูกสร้างต่อ edit update, `CanMerge()` รวมคำสั่ง curve gesture และ `Merge()` เรียก `curve.Simplify()` ก่อนสร้าง merged undo command (`ref-openutau-clean/core/OpenUtau.Core/Commands/ExpCommands.cs:330-375`). ดังนั้น Rusted UTAU v2 จะรับ preview/batches ได้ แต่ทำ simplify **ครั้งเดียวตอน commit gesture**—ตรง logical boundary ของการรวม gesture ไม่ใช่ทุก pointer update และไม่ใช่ทุก save/render.

### 6.7 Canonicalization / resampling ownership (FROZEN)

#### 6.7.1 Sparse storage vs render sampling

Curve storage เป็น **sparse control-point list**, ไม่ใช่ recording ที่มี fixed Hz. ช่วง snap 5 ticks คือ edit grid/render evaluation interval; ไม่ได้บังคับให้มี stored point ทุกช่วง. Renderers อาจ sample/interpolate เส้นตาม tick positions ที่ต้องใช้ โดยไม่ materialize ทุก sample ลง curve ที่บันทึก.

#### 6.7.2 OpenUtau-compatible simplification at commit

หลัง Rust canonicalize และ merge/replacement ช่วง stroke ตาม §6.6.2 แล้ว Rust MUST simplify **curve ทั้งเส้น** ด้วยพฤติกรรมเทียบเท่า OpenUtau `UCurve.Simplify()` (`UCurve.cs:120-161`): OpenUtau เรียก `Simplify()` บน curve object หลัง merge gesture command ทั้งหมด ไม่ได้จำกัดเฉพาะช่วงที่ pointer ลาก.

**Algorithm reference (ต้องคง tie/scan order ของ implementation เพื่อ parity):**

```text
simplify(points, first, last, tolerance):
  if last - first < 2: return
  best_distance = 0
  best_index = 0
  for i in first .. last-1:        // exclude final endpoint
    d = perpendicular_distance(points[first], points[last], points[i])
    if d > best_distance:          // strict; equal distance keeps the earlier index
      best_distance = d
      best_index = i
  if best_distance > tolerance and best_index != 0:
    keep(best_index)
    simplify(first, best_index, tolerance)
    simplify(best_index, last, tolerance)
```

- `tolerance = min(5, (descriptor.max - descriptor.min) * 0.005)` in descriptor value units.
- Perpendicular distance is the Euclidean distance to the **infinite line** through the span endpoints, as implemented by OpenUtau's doubled triangle-area divided by endpoint length (`UCurve.cs:140-161`). Do not silently replace this with vertical interpolation error or segment-clamped distance.
- Keep first and last points of the whole curve; recursively retain an interior point only when its perpendicular distance is **strictly greater** than tolerance. If the maximum distance ties, keep the first point in ascending-x order (`height > maxHeight` in reference). Then recurse left/right.
- Canonicalization guarantees strictly increasing x ticks, so a multi-point span's endpoint x values differ and its endpoint segment cannot have zero length. The implementation guards zero length defensively by measuring from the endpoint; this branch is unreachable for valid canonical input.
- Inputs to simplification have x strictly increasing after §6.6.2 canonicalization/dedupe. Spans with fewer than three points are unchanged.
- Distance comparisons and retained-point decisions MUST match in native and WASM. Match the reference's IEEE-754-style distance comparison and tie rule; no host-specific epsilon. If a fixed-point implementation is desired, prove it makes exactly the same keep/drop decisions at threshold and tie fixtures before substituting it.
- Simplification is part of one committed mutation: it adds no revision/undo. Undo restores the pre-stroke arrays byte-exactly; response patch/read carries the post-simplification committed curve.
- Since reference simplifies the whole curve after merged gesture commands, redundant points outside the replaced interval may be removed if they are within tolerance of the retained polyline. No unrelated curve/entity may change. Generic cross-part `MergeCurves` and save/render-time simplification are out of scope.

**Conformance:** strict tolerance boundary (equal drops, greater retains), endpoint preservation, first-maximum tie, collinear points, line-extension/perpendicular-distance case, narrow spike below/above tolerance, uneven x-spacing, negative descriptor values, multi-span gesture, redundant outside-span removal only within tolerance, unaffected curve/entity preservation, deterministic replay, exact undo/redo, native/WASM identical output, and degenerate zero-length endpoint cases for the explicitly chosen rule.

| เรื่อง | v2 |
|---|---|
| UI sampling density | input/preview policy; dense samples ได้เฉพาะภายใน advertised payload/point limits |
| Rust stored result | canonicalize ตาม §6.6.2 → interval merge → simplify ทั้ง curve ตาม §6.7.2 → เก็บ parallel `Vec<i32>` |
| simplification | **required** สำหรับ adapters ที่ประกาศ `curve_apply_stroke` |
| tolerance | `min(5, descriptor_range * 0.005)`; retain เมื่อ `distance > tolerance` เท่านั้น |
| determinism | ascending-tick scan, first-maximum tie, exact shared distance decision, same native/WASM output |

### 6.8 Groups: state machine and transaction semantics (FROZEN)

#### 6.8.1 State machine

Session มี group ได้ **สูงสุดหนึ่ง** ต่อเวลา. States ของ session:

```
none --begin--> active --commit--> none   (outcome: committed, revision R+1)
                       --cancel--> none   (outcome: cancelled, revision R)
                       --child fail--> none (outcome: failed, revision R, tentative ทิ้งทันที)
```

- **ไม่มี** `failed` state ที่ค้างให้ inspect: child ที่ fail จะ rollback tentative ทันทีและ
  ปิด group; response คือ `group_failed`. เหตุผล: frontend ต้องไม่ต้องเดาว่า group ที่ fail
  ยังมี mutation ค้างไหม; และไม่มี use case ที่ต้อง inspect failed group.
- Terminal outcome ทั้งสาม (`committed`/`cancelled`/`failed`) ถูกรายงานใน response; group
  ถูกถอดออกจาก session ทั้งสามกรณี. คำสั่งกับ group ที่ปิดแล้ว → `not_found`.
- Nested group: **ไม่รองรับใน v2** → `invalid_group_state`.

#### 6.8.2 Operation rules

| operation | `expected_revision` | public revision | tentative state | หมายเหตุ |
|---|---|---|---|---|
| `group_begin` | ต้องเท่ากับ R | คงที่ R | สร้างสำเนา tentative จาก R | คืน `group_id`, `base_revision` |
| child command | ต้องเท่ากับ R + `group_id` | คงที่ R | validate/apply บน tentative | **ไม่คืน `changes`** (tentative ยังไม่ใช่ public state) |
| child fail | — | คงที่ R | ทิ้งทั้ง tentative ทันที, ปิด group | error `group_failed`; ไม่มี partial commit |
| `group_commit` | ต้องเท่ากับ R | R → **R+1** (ครั้งเดียว) | tentative กลายเป็น document จริง | หนึ่ง undo step; คืน `changes` = merged child change set |
| `group_cancel` | ไม่บังคับ (ถ้าส่งต้องเท่ากับ R) | คงที่ R | ทิ้ง tentative | `changed: false`; เอกสารเท่ากับก่อน begin เป๊ะ |
| อื่น ๆ ระหว่าง open group | — | ต้องถูก reject | — | `session_busy` สำหรับ mutation/undo/redo/import; `revision_conflict` ถ้า base เปลี่ยนจากภายนอก |

- **Read ระหว่าง group คืน committed state เท่านั้น** (revision R). v2 **ไม่รองรับ tentative
  read** — ไม่มี use case: frontend แสดง preview ของตัวเองอยู่แล้ว.
- Commit เมื่อ committed document ไม่ได้อยู่ที่ R (เช่นถูก reload/replace) →
  `revision_conflict`, ทิ้ง tentative, ไม่เปลี่ยนอะไร.
- `group_cancel` บน group ที่ปิดแล้ว → `not_found` (ไม่ idempotent; ระบุชัดในสัญญา).
- Session close ขณะมี open group → ทิ้ง tentative, ไม่มีการเปลี่ยน committed state,
  close สำเร็จ.
- `group_begin` ที่ชื่อว่าง → ใช้ชื่อ default `"Edit"`.
- Commit ที่ group ว่าง/no-op → ไม่เพิ่ม revision, ไม่เพิ่ม undo step, `changed: false`.

#### 6.8.3 Implementation strategy (internal, ไม่ expose ใน API)

**เลือก clone-on-group-begin.** หลักฐาน: `UProject` เป็น `Vec`/`String`/`HashMap` ล้วน
ไม่มี PCM (`domain/src/project.rs:212-266`); วัดจริงกับ `badapple.ustx` (867 notes, ~250 KB
YAML, retained ≈225 KB) → **clone 34.5 µs**, `clone + before_save` 145.5 µs (debug build,
เป็น upper bound). เทียบกับ command-journal rollback ที่ต้องให้ทุก child command reverse ได้
แบบ exact ซึ่งเสี่ยงและซับซ้อนกว่า. ไม่มี use case ที่ต้อง transaction ขนาดใหญ่.

**ข้อควรระวังที่พบ (สำคัญต่อ implementation):** `ProjectDoc::apply` วันนี้ตั้ง
`self.snapshot = self.yaml()` ทุกคำสั่ง (`project-session/src/lib.rs:94,99,108`), และ
`to_yaml` = `project.clone()` + `before_save()` + `serde_yaml::to_string` (`:295-299`)
→ วัดได้ **38.4 ms** ต่อคำสั่งกับ badapple (debug). ฉะนั้น:
- `snapshot`/`yaml()` ต้องเป็น **lazy** — คำนวณเมื่อมี read/mutation result ขอเท่านั้น
- child command ใน group **ห้าม** serialize YAML ต่อคำสั่ง
- ถ้าไม่ทำสองข้อนี้ จะขัดกับ §6.4 ที่บอกว่า snapshot ไม่บังคับ และจะทำให้ group แพงโดยไม่จำเป็น

**Undo payload ของ commit:** ติดตั้ง tentative document แล้วบันทึก **หนึ่ง** undo step ที่มี
payload = เอกสารก่อน group (clone ที่ถืออยู่แล้วตอน begin) ผ่าน command ชนิด
"restore document" ที่เพิ่มใหม่ใน `commands` — ทำให้ `group_cancel`/undo คืนค่าได้ exact.
ค่าใช้จ่ายเพิ่ม = หนึ่ง clone ต่อ committed group (ยอมรับได้ตามตัวเลขข้างบน).

**สถานะ:** implement แล้วใน `project-session` (2026-09-29) — `ActiveGroup` ถือ
`ProjectDoc::from_project_lazy(committed.clone())` + identity table ของตัวเอง, child ใช้
`ProjectDoc::apply` เดิมบน tentative แล้ว reconcile identity, commit เรียก
`ProjectDoc::commit_transaction` ซึ่ง push `RestoreDocumentCommand` หนึ่งตัวลง undo stack จริง
(revision +1 ครั้งเดียว). หลักฐาน: `tests/session_groups.rs` 13 tests ครอบทุก transition,
failed child, cancel exact restore, no-op commit, stale base, busy rejection และ id stability.

**Legacy compatibility:** facade เดิม `engine-wasm` ใช้ string command
`group_begin`/`group_end`/`group_cancel` ที่ child mutate document ทันทีและ
`group_cancel` reverse ให้ (`project-session/src/lib.rs:86-102`) — **ยังคง behavior เดิม**
ไม่ถูก redefine. API ใหม่แยกด้วย `type` discriminators และ `/groups` routes จึงไม่ชนกัน.
Semantics แบบ isolated tentative transaction เป็นของ API v2 เท่านั้น และต้อง implement
ก่อน freeze CONTRACT-v2.

### 6.9 Revision rules (FROZEN — ตารางเดียวสำหรับทุก adapter)

`revision` เพิ่มขึ้น **เท่านั้น** และเพิ่ม **เฉพาะเมื่อ committed authoritative state เปลี่ยน**.
Native และ WASM ต้องให้ผลตรงกันสำหรับลำดับคำสั่งเดียวกัน.

| operation | ต้องมี `expected_revision`? | committed document เปลี่ยน? | public revision | undo history | render input | response revision |
|---|---|---|---|---|---|---|
| create session (`new`) | ไม่ | สร้างใหม่ | **0** (session แรกเกิดที่ revision 0) | ว่าง | pending | `0` |
| open session (`stored_project`/`imported_ustx`) | ไม่ | โหลดจากไฟล์/payload | **0** | ว่าง (history ไม่ถูกโหลดจากไฟล์) | pending | `0` |
| command success | ใช่ (= R) | ใช่ | **R+1** | +1 step (หรือ +0 ถ้าอยู่ใน open group) | invalidated ถ้ากระทบ | `R+1` |
| command validation failure | ใช่ | ไม่ | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` |
| stale revision failure (`revision_conflict`) | ใช่ | ไม่ | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` (ค่าปัจจุบัน) |
| group begin | ใช่ (= R) | ไม่ | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` |
| group child success | ใช่ (= R) + `group_id` | ไม่ (tentative เท่านั้น) | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` |
| group child failure | ใช่ | ไม่ (tentative ทิ้ง) | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` |
| group commit | ใช่ (= R) + `group_id` | **ใช่** (ทั้งหมด atomic) | **R+1** | **+1 step** (ทั้ง group) | invalidated | `R+1` |
| group commit ว่าง/no-op | ใช่ | ไม่ | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R`, `changed: false` |
| group cancel | ไม่บังคับ | ไม่ | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` |
| undo (มี step) | ใช่ (= R) | ใช่ | **R+1** | undo −1, redo +1 | invalidated | `R+1` |
| redo (มี step) | ใช่ (= R) | ใช่ | **R+1** | undo +1, redo −1 | invalidated | `R+1` |
| no-op undo/redo (stack ว่าง) | ใช่ | ไม่ | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R`, `changed: false` |
| import / replace document | ใช่ (= R) | **ใช่** (แทนทั้งเอกสาร) | **R+1** | **clear ทั้งหมด** | invalidated | `R+1` |
| save | ใช่ (= R) | ไม่ (แค่ persist) | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` |
| export | ไม่ | ไม่ | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` (ใน metadata) |
| render start | ใช่ (= R) | ไม่ | คงที่ R | ไม่เปลี่ยน | ไม่เปลี่ยน (จับ snapshot ที่ R) | `R` ใน job descriptor |
| render completion | ไม่ (อ้าง render_id) | ไม่ | คงที่ R ของ job | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` ของ job |
| render cancel | ไม่ (อ้าง render_id) | ไม่ | คงที่ R ของ job | ไม่เปลี่ยน | ไม่เปลี่ยน | `R` ของ job |
| session close | ไม่ | ไม่ | ไม่มีต่อ | ทิ้ง | ทิ้ง | — |

**รายละเอียดที่ปิดความกำกวม:**

1. `revision` เริ่มที่ 0 ต่อ session (ไม่ใช่ 1) และไม่ถูก serialize ลง `.ustx`/sidecar —
   เป็นค่าของ session เท่านั้น.
2. **Save/export/render ไม่เปลี่ยน revision** เพราะไม่เปลี่ยน committed state.
3. **Group begin/child/cancel ไม่เปลี่ยน revision**; มีเพียง commit ที่ขยับครั้งเดียว.
4. **Undo/redo ที่สำเร็จขยับ revision เสมอ แม้จะย้อนหลาย edit ในหนึ่ง step** (หนึ่ง step =
   หนึ่ง revision transition).
5. Failure ทุกชนิด (validation, conflict, payload, busy, group) → response `revision`
   คือค่าปัจจุบันของ session และ committed state/history/render input ต้องไม่ถูกแตะ.
6. Revision เป็น **ต่อ session** ไม่ใช่ต่อ project file. Session ใหม่ที่เปิดไฟล์เดิมเริ่มที่ 0
   อีกครั้ง. Client ห้ามใช้ revision ข้าม session.
7. การเทียบ `expected_revision` เป็น equality ตรงตัว; client ที่ไม่มี revision ให้ส่งค่าที่
   มันเห็นล่าสุดจาก read/mutation result; ถ้าไม่ส่งเลย → `validation_error` (ไม่ใช่
   last-write-wins).

### 6.10 Bounded read models (FROZEN, ขั้นต่ำ)

| read | path (proposed) | คืนอะไร | ใช้เมื่อ |
|---|---|---|---|
| full snapshot | `GET /api/v2/sessions/{id}` | snapshot §6.3 | create/open, resync, conflict recovery, structural change, test |
| project metadata/maps | `GET /api/v2/sessions/{id}/project` | `{revision, project: ProjectMetaView}` | หลัง `project.*` ที่ invalidate |
| one part | `GET /api/v2/sessions/{id}/parts/{part_id}` | `{revision, part: PartView}` | หลัง `part.*` ที่ invalidate |
| part notes | `GET /api/v2/sessions/{id}/parts/{part_id}/notes` | `{revision, part_id, notes[]}` | หลัง `note.*` ที่ `authority: "invalidate"` |
| one curve | `GET /api/v2/sessions/{id}/parts/{part_id}/curves/{curve_id}` | `{revision, part_id, curve_id, abbr, points[]}` | หลัง `curve.changed` ที่ invalidate |
| track metadata | `GET /api/v2/sessions/{id}/tracks` | `{revision, tracks[]}` | หลัง `track.*` ที่ invalidate |

- ทุก read model ต้องมี `revision` ที่ authoritative และต้องมี identity ครบ (`session_id`
  อยู่ใน path/descriptor, `part_id`/`curve_id`/`track_id` ตามชนิด).
- **Stale-response handling (FROZEN):** client เปรียบ `revision` ของ read กับ revision
  ที่ตนถืออยู่ — ถ้า read revision **น้อยกว่า** → ทิ้งผลนั้น ห้ามทับ cache ที่ใหม่กว่า.
  ถ้าเท่า → patch ได้. ถ้ามากกว่า → ยอมรับและอัปเดต revision (เกิดได้เมื่อมี mutation
  จากที่อื่น).
- v2 บังคับเฉพาะ read models ในตารางนี้. **ไม่บังคับ** ให้ client ยิง read หลังทุกคำสั่ง:
  ถ้า `changes[].authority == "patch"` ก็ patch จาก data ที่ได้เลย.
- Partial reads เป็น optional capability ในความหมายว่า adapter อาจยังไม่ implement ครบ —
  แต่ถ้าไม่ implement ต้อง **ประกาศใน capabilities** และ mutation ต้องคืน `patch` data
  มากพอแทน (ห้ามปล่อยให้ client ค้างโดยไม่รู้).

### 6.11 Session creation / open schema (FROZEN — discriminated `source`)

```json
{"source": {"type": "new", "name": "Untitled", "bpm": 110}}
```
```json
{"source": {"type": "stored_project", "name": "scale-demo"}}
```
```json
{"source": {"type": "imported_ustx", "name": "from-openutau", "ustx": "…yaml…"}}
```

| `type` | ความหมาย | ID behavior |
|---|---|---|
| `new` | สร้างเอกสารว่างด้วย Rust defaults | mint ID ทั้งหมด (§6.2.6) |
| `stored_project` | เปิดโปรเจกต์ที่ host store ไว้ (`.ustx` + sidecar ที่ host เป็นเจ้าของ) | matching ตาม §6.2.4 |
| `imported_ustx` | เอกสาร USTX ที่ส่งมา inline (ผู้ใช้ import/เปิดไฟล์) | **mint ID ใหม่ทั้งหมด**, `ids_stable: false` |

- `source.name` ต้องผ่านกฎชื่อเดียวกับ host store; ชื่อซ้ำกับ `stored_project` ที่มีอยู่ →
  `validation_error`/`name_conflict` (ห้าม overwrite เงียบ ๆ).
- **ไม่ expose host filesystem path** ใน logical API: `stored_project` อ้างด้วยชื่อใน store
  ของ host; path/SAF/OPFS เป็นเรื่องของ adapter. เหตุผล: path ไม่ใช่ shared semantics และ
  ผูก contract กับ OS.
- v2 ไมรองรับ `recovery` source (ไม่มีการกู้ session) — ถ้าจะเพิ่มในอนาคตต้องเพิ่ม variant ใหม่
  ไม่ใช่ยัดความหมายเข้า `stored_project`.
- Response ของ create = full snapshot (บังคับ) + session descriptor + capabilities ที่
  session ใช้จริง.

#### Lifecycle persistence adapters (draft implementation)

- Native save ใช้ `PUT /api/v2/sessions/{id}/save`; Save As ใช้
  `PUT /api/v2/sessions/{id}/save-as` กับ `{name, expected_revision}`. Save As เตรียม
  `.ustx` + sidecar ที่มี `project_id` ใหม่และ entity IDs เดิม, เขียนไฟล์ทั้งคู่ก่อน
  แล้วค่อย adopt project ID ใหม่เข้า live session. ความล้มเหลวก่อนเขียนสำเร็จต้องไม่
  เปลี่ยน identity/revision ใน session.
- รอบร่างนี้ยังไม่ได้เพิ่ม lifecycle Save As/export API ให้ WASM. จะทำ adapter
  ฝั่ง WASM และ UI Web แยกในขั้นลง Web โดยคง identity fork semantics เดียวกับ
  `project-session`; ห้ามถือว่าร่างนี้เป็น API ที่มีอยู่แล้ว.
- native export ใช้ `GET /api/v2/sessions/{id}/export` ส่ง canonical USTX โดยไม่มี
  sidecar. Portable export และการ import ที่ mint IDs ใหม่เป็น logical semantics
  ร่วมกัน แต่ adapter ฝั่ง WASM ยังรอการลงงานแยก.
- Portable export เป็น canonical USTX จาก Rust โดยไม่มี sidecar; open/import ไฟล์นั้น
  จะ mint identity ใหม่ตาม §6.2.6.

### 6.12 Session lifecycle (FROZEN)

**หลักการ:** logical session เป็น in-memory, process-local, **ไม่ durable**. v2 ไม่มี
durable session, ไม่มี cross-restart recovery, ไม่มี multi-user.

#### Native (synth-server)

**Web host recovery extension (2026-10-17):** the HTTP editor may send an opaque
per-tab `recovery_id` when it opens a session. The server writes a separate
Rust-authored checkpoint under its project storage after each committed
revision (grouped gestures checkpoint at commit). This checkpoint is not a
named project and never overwrites one. After a page/server restart the same
tab can call `POST /api/v2/recovery/{recovery_id}`; the server opens a fresh
session from the last checkpoint. The logical session is still process-local:
revision and undo history restart, and an in-progress gesture is not recovered.
The browser stores only the opaque recovery key; project bytes, sidecars, and
uploaded `asset:<id>` media bytes remain in Rust/server storage.

| เหตุการณ์ | พฤติกรรม (FROZEN) |
|---|---|
| create | session ใหม่ + registry entry (server ต้องเพิ่ม session registry วันนี้ยังไม่มี — `AppState` มีแค่ voicebanks/projects/renderer/stats, `state.rs:16-27`) |
| explicit close | ทิ้ง session และ state ทั้งหมดที่ยังไม่ save; open group ถูกทิ้ง; render job ที่ค้างอยู่ต้องถูก cancel; idempotent (`not_found` ถ้าไม่มีอยู่แล้ว) |
| idle expiry | **FROZEN 2026-09-29:** default **30 นาที** ของ "ไม่มี activity"; ทุก logical operation นับเป็น activity และ **reset TTL**. Session ที่มี render job สถานะ `queued` หรือ `running` **ต้องไม่ expire** จนกว่า job จะจบหรือถูก cancel. Expiry = close แบบเดียวกับ explicit close (state ที่ไม่ save หาย). ค่าจริงต้องประกาศใน capabilities |
| unsaved committed changes | logical session หายเมื่อ close/expire/restart. HTTP Web host recovery ข้างต้นเก็บ checkpoint ของ committed document แยกจาก session; host อื่นต้องกำหนด recovery ของตัวเอง |
| server restart | live session และ undo history หาย. HTTP Web host เปิด session ใหม่จาก checkpoint ได้; หากไม่มี checkpoint client ต้องเปิดจาก `.ustx` + sidecar. นี่ไม่ใช่ durable logical session |
| crash | เหมือน restart. sidecar ที่เขียน atomic แล้วทำให้ ID ต่อเนื่องได้; revision/undo history เริ่มใหม่ที่ 0 |
| Android | server เป็น child process ที่ถูก kill ตอน `onDestroy` (`MainActivity.kt:92-95`, `EngineProcess.kt:173-179`) และ **ไม่มี watchdog** (`engineRunning` ไม่เคยถูกเรียกจาก Dart) → ถ้า process ตาย client ต้องตรวจ `/health` และ reopen เอง; v2 ไม่สัญญา auto-recovery |
| concurrency | 여러 session isolated; render ใช้ worker เดียวและคิว FIFO วันนี้ (`render_service.rs:92-113`) → v2 ต้องมี per-job cancel ไม่ใช่ global flag (ดู §6.13) |

#### WASM / browser

- หนึ่ง session = หนึ่ง `Engine`/session object (`engine-wasm/src/lib.rs` ถือ shared
  `project_session::Session` + derived render `plan`). Close = drop object. ไม่มี idle expiry.
- ไม่มี cross-tab/cross-worker shared session ใน v2; หลาย `Engine` = หลาย session ที่ isolated กัน.
- Revision/ID ทั้งหมดอยู่ใน Rust; epoch ที่ `web/app/editor.js` ถืออยู่เป็น UI-side
  stale-result guard เท่านั้น และ **ไม่ใช่** revision ของสัญญานี้ (ต้อง map ให้ชัดตอน implement).

#### สิ่งที่ v2 ไม่สัญญา

- logical v2 เองไม่มี durable session หรือ recovery semantics. Host-specific recovery
  เช่น Web HTTP ต้องแยกจาก session lifecycle และห้ามเปลี่ยน revision/ID semantics.
  ไม่มี multi-user/remote access control.

### 6.13 Render job model (FROZEN — asynchronous)

**หลักฐานที่ทำให้เลือก async:** วันนี้ `/render` เป็น synchronous และคืน WAV ตรง ๆ
(`handlers.rs:434-497`, `wav_response` `:666-681`); มี worker เดียว + คิว unbounded
(`render_service.rs:92-113`); ไม่มี progress/job id; `/cancel` เป็น `AtomicBool` global ตัวเดียว
ที่ถูก poll ตอนเริ่ม chunk (`pipeline.rs:674-679`) และถูก **clear ทุกครั้งที่เริ่ม render ใหม่**
(`render_service.rs:400-401`) → Stop ที่กดระหว่าง render N ไม่หยุด render N+1 ที่เข้าคิวแล้ว.
ขนาดงานจริง: chunk target 4 s (`synth-cli/src/pipeline.rs:76`); badapple = 867 notes /
504.23 s / **136 chunks** (`wasm-performance.md:84`); วัดได้ 36–47 s บน desktop และ
~1.4 s/chunk บน Mi Pad → เพลงยาวบนอุปกรณ์ประมาณ ~2.5–3 นาที. ดังนั้น synchronous-only
ไม่พอสำหรับงานจริง.

**Logical semantics (ต้องเหมือนกันทุก adapter):**

1. `render start` รับ `expected_revision` (= R) และ **จับ immutable committed snapshot ที่ R**
   ไว้กับ job. Mutation หลังจากนั้น **ไม่กระทบ** input ของ job นั้น.
2. คืน `render_id` (opaque) + `{session_id, revision: R, status: "running"|"queued", engine}` ทันที.
3. Progress: `{session_id, render_id, revision: R, done_chunks, total_chunks, elapsed_ms}`
   โดย tag ครบ. Granularity ขั้นต่ำ = ขอบ chunk (worker คืนผลต่อ worker ไม่ใช่ต่อ chunk
   วันนี้ — `web/worker.js:111-130`) และ adapter **ห้ามสัญญา** ละเอียดกว่านั้น.
4. Completion: `{session_id, render_id, revision: R, sample_rate, frames, engine, status:"done"}`
   + ทางดึง audio (response body หรือ artifact reference).
5. `render cancel` อ้าง `render_id` (ไม่ใช่ global) → job นั้นหยุด; job อื่นในคิวไม่ถูกกระทบ.
   สถานะสุดท้ายของ job ที่ถูก cancel = `cancelled` + error code `render_cancelled`.
6. **Stale audio ห้าม publish เองเป็น current**: client จะเล่น/แสดงผลลัพธ์ที่ `revision !=
   current` ได้เฉพาะเมื่อ UI ติดป้าย stale ชัดเจน (การเลือกเป็น UI policy, semantics คือห้ามเงียบ).
7. Transport ต่างกันได้: native HTTP อาจใช้ polling/SSE/WebSocket; WASM ใช้ worker
   messages/callbacks. **Semantics ต้องเหมือนกัน.**
8. Synchronous render เหลือได้ในฐานะ **adapter convenience** สำหรับงานสั้น/เทสต์
   (adapter block จน job เสร็จแล้วคืน body) แต่ **state model ยังเป็น job เดียวกัน** —
   ไม่ใช่โมเดลที่สอง.
9. v2 ต้องไม่รับ render จาก path ที่อาจ stale: worker ต้องรับ **serialized immutable snapshot**
   ที่ session ส่งไป (วันนี้ worker re-read `.ustx` จาก path เอง — `render_service.rs:403`).
10. คิว/worker count เป็นเรื่อง throughput ของ host (วันนี้ 1 worker, FIFO) ไม่ใช่ semantics;
    การเพิ่ม worker ภายหลังต้องไม่เปลี่ยนข้อ 1–7.

### 6.14 Error model (FROZEN)

Response: `{"error": {"code": "...", "message": "...", "details": {...}}, "revision": R}`

| code | HTTP | หมวด | เกิดเมื่อ |
|---|---|---|---|
| `not_found` | 404 | user | session/group/render_id/track_id/part_id/note_id/curve_id/stored project หรือ tempo/meter position ไม่มี |
| `validation_error` | 400 | user | input ผิด (tick out of range, lyric/tone ผิด, `points` ไม่ถูกตาม mode, `abbr` mismatch, `name_conflict`) — แก้ได้โดยผู้ใช้ |
| `unsupported_operation` | 400/501 | user | คำสั่ง/mode ที่รู้จักแต่ v2 ไม่รองรับ (เช่น `mode: "blend"`, nested group) |
| `revision_conflict` | 409 | concurrency | `expected_revision` ไม่ตรงกับ committed revision |
| `session_busy` | 409 | concurrency | มี open group อยู่ แล้วมี mutation/undo/redo/import อื่นเข้ามา |
| `invalid_group_state` | 409 | concurrency | commit/cancel บน group ที่ปิดแล้ว/ผิด state, nested group |
| `group_failed` | 409 | concurrency | child command ใน group ล้ม → tentative ถูกทิ้งทั้งชุด (code นี้ยังคงไว้ เพราะ frontend ต้องแยกจาก validation error เดี่ยว) |
| `payload_too_large` | 413 | transport | เกิน limit ที่ประกาศใน capabilities — ไม่มี mutation/revision/history change |
| `storage_error` | 500 | storage | อ่าน/เขียน `.ustx`/sidecar/lock ล้มเหลว |
| `media_unavailable` | 404 | storage | host ไม่พบ asset ตาม logical `media_ref` |
| `render_error` | 500 | internal/render | render ล้มเหลวระหว่างทำงาน |
| `render_cancelled` | 409 | user | job ถูก cancel โดยผู้ใช้/close |
| `internal_error` | 500 | internal | bug/panic ที่ถูก guard แล้ว; ต้องมี diagnostic |

- Frontend **branch บน `code` เท่านั้น** ห้าม parse `message`. `message` เป็นข้อความที่แสดง
  ให้ผู้ใช้/แปลภายหลังได้.
- `details` มี structured diagnostics ได้ (เช่น `current_revision`, `part_id`, `field`) แต่
  frontend logic ต้องไม่พึ่ง field ที่ไม่ถูก freeze ในตารางนี้.
- Rejected operation ทุกกรณี: committed document, public revision, undo/redo history และ
  render input ต้องไม่เปลี่ยน (§6.9).
- **หมายเหตุที่พบ:** extractor วันนี้ map body-limit rejection เป็น **400** พร้อมข้อความ
  axum (`handlers.rs:50-62`) → v2 ต้อง map เป็น 413 + `payload_too_large` แทนเพื่อให้ client
  แยก "payload ใหญ่" จาก "JSON ผิด" ได้.

### 6.15 Payload limits and capability negotiation (FROZEN)

**Limits ที่ต้องประกาศและบังคับ:**

| limit | shared minimum guarantee | เหตุผล |
|---|---|---|
| `max_command_bytes` | **1,048,576 B (1 MiB)** | object-form stroke point ≈27.4 B; 20k points ≈548 KB → 1 MiB ครอบเคสจริง. native วันนี้ได้ 2,097,152 B อยู่แล้ว (axum default `Bytes` limit, ไม่มี `DefaultBodyLimit` ใน server) |
| `max_stroke_points` | **20,000 points ต่อหนึ่ง `curve.apply_stroke`** | เกินกว่านี้คือการวาดที่ UI ควร downsample เองก่อนส่ง (transport detail ไม่ใช่ semantics) |
| `max_snapshot_bytes` | ไม่กำหนด (informational) | snapshot เป็น response; ใหญ่ได้ตามโปรเจกต์ |
| `max_import_bytes` | host-defined, ต้องประกาศ | `imported_ustx` ส่ง YAML inline |

- Client ต้องเคารพค่าที่ host ประกาศ ไม่ใช่ค่าขั้นต่ำ; host **ห้ามประกาศสูงกว่าที่ทำได้จริง**.
- **implement แล้วฝั่ง session:** `Capabilities`/`Limits` ถูก declare และบังคับจริง — request ที่
  เกิน `max_command_bytes` ถูกปฏิเสธก่อน parse และ stroke ที่เกิน `max_stroke_points` ถูกปฏิเสธ
  ก่อนแตะเอกสาร ทั้งคู่คืน `payload_too_large` โดยไม่เปลี่ยน revision/history
  (`project_session::v2::capabilities`).
- เกิน limit → `payload_too_large`, **ไม่มี** mutation/revision/history change, และไม่มีการ
  เขียนบางส่วนลงเอกสาร.
- **Legacy ที่ต้องไม่สืบทอดเงียบ ๆ:** `POST /post-fx` วันนี้รับ raw WAV เป็น request body →
  ติด 2 MiB (~23.8 s mono 16-bit 44.1 kHz) ทำให้ใช้กับ render ยาวไม่ได้. v2 ต้องไม่ผูก audio
  transfer ไว้ฝั่ง request; บันทึกเป็น known legacy limitation (นอก scope ของ draft นี้).

**Capability descriptor (FROZEN shape):**

```json
{
  "contract_version": "2.0",
  "command_schema_version": 2,
  "snapshot_schema_version": 2,
  "id_schema_version": 1,
  "features": ["stable_ids", "partial_reads", "group_transactions",
               "curve_apply_stroke", "curve_clear"],
  "limits": {"max_command_bytes": 1048576, "max_stroke_points": 20000},
  "render": {"async_jobs": false, "per_job_cancel": false, "progress_transports": []},
  "host": {"engines": [{"id": "r1-rs", "available": true}],
           "idle_ttl_seconds": 1800, "max_import_bytes": 8388608,
           "transport_methods": ["http_json"]}
}
```

- Shared logical DTO fields are `contract_version`, schema versions, `features`, `limits`, and
  `render`; host-only extensions live under `host` (for example `engines`, idle TTL, import size,
  and transport methods). Native implementations include `poll` when the polling route is ready;
  direct WASM may report no progress transport while `async_jobs:false`.
- **ห้าม** ให้ client ตรวจ support ด้วยการยิงคำสั่งให้ error (probe by error).
- **หมายเหตุ:** `/capabilities` วันนี้คืน static `WorldlineCapabilities` และไม่รายงานว่า
  server เปิด engine ไหนได้จริง (`handlers.rs:360-372`, `worldline-plugin/src/capabilities.rs:64-79`)
  — v2 ต้องรายงานตามจริง ไม่ใช่ static list.
- Legacy CONTRACT-v1 ยังแยกจาก descriptor นี้; client ต้องเลือกใช้ตาม endpoint/version
  ไม่ใช่เดาจากการมีอยู่ของ field.

### 6.16 Legacy compatibility (ย้ำ)

- §0–§5 ของเอกสารนี้ยังเป็น CONTRACT-v1 ที่ frozen; v2 logical contract ไม่แก้ route/behavior เดิม.
- `engine-wasm` facade ที่รับ string command `{"cmd":"move_note","part":0,"note":3,...}`
  (index-addressed) **ยังทำงานเหมือนเดิม** และยังใช้โดย Web editor ปัจจุบัน; การย้าย Web ไป
  stable-ID session เป็นงานถัดไป ไม่ใช่การลบของเดิม.
- `group_begin`/`group_end`/`group_cancel` แบบ string ของ facade เดิมมี semantics
  "apply ทันที + reverse" — **ไม่ถูก redefine**; ของใหม่ใช้ `type` + `/groups` routes.
- `ustx-codec` เป็น layer mapping JSON view แบบ lossy (`ustx-codec/src/lib.rs:43-86`) และ
  rebuild `UProject::create()` ใหม่ (`:151-213`) → **ห้ามใช้เป็นที่เก็บ ID** และห้ามถือเป็น
  canonical document; canonical ยังเป็น USTX ที่ domain serialize.
- Sidecar **ต้องไม่เปลี่ยน bytes ของ `.ustx`**: ไฟล์ `.ustx` ที่ Rusted UTAU เขียนต้องไม่มี key ใหม่
  เพิ่ม (พิสูจน์ด้วย test เทียบกับ USTX ที่ OpenUtau อ่านได้).
- ห้ามลบ legacy API ก่อนที่ path ที่ migrate แล้วจะไม่พึ่งมัน (ดู §6.19 ขั้น 11).

### 6.17 Conformance test vectors (FROZEN requirements — implemented)

**สถานะ:** ชุด vector อยู่ที่ `native/crates/project-session/tests/vectors/session_vectors.json`
พร้อม runner `tests/conformance_vectors.rs` (14 vectors ครอบ note edit/undo/redo, `not_found`,
stale revision, `note.added` แล้ว address ต่อได้, move แล้ว id คงอยู่, curve replace/clear/create
+ rejection reasons, group commit/cancel/failed child/busy, bounded reads ที่ tag revision และ
`ids_stable` fallback). Vector อ้าง entity ด้วย ref `@note:0`/`@part:0`/`@curve:0`/`@last_note:`
จึงยกไปรันกับ adapter อื่นได้โดยไม่ต้องเขียนใหม่ — native/WASM adapter จะรันไฟล์เดียวกันนี้.

ต้องมีชุด vector กลางที่รันได้ทั้ง native adapter และ WASM adapter แล้วเทียบผล
(normalize เฉพาะ transport field). กลุ่มที่บังคับ:

**Stable identity**
1. ID คงอยู่ผ่าน save/reload เมื่อ sidecar ใช้ได้ (`ids_stable: true`).
2. Reorder track/part/note แล้ว ID ไม่เปลี่ยน (ใน session และหลัง reload).
3. Copy/paste/duplicate → ID ใหม่ทั้งหมด; ไม่มี ID ซ้ำในเอกสาร.
4. Sidecar ที่มี ID ซ้ำ → entry แรกคงไว้, ที่เหลือได้ ID ใหม่, warning `duplicate_id_repaired`.
5. Sidecar หาย/เสีย/version ไม่รู้จัก → open ยังสำเร็จ, `ids_stable: false`, warning `ids_regenerated`.
6. Sidecar ที่ structure เปลี่ยน (จำลอง OpenUtau save) → Pass A/B matching ให้ผล deterministic
   และ ID ของ entity ที่ไม่ถูกแตะต้องคงเดิม.
7. Import `.ustx` → ID ใหม่ทั้งหมด; save → sidecar ใหม่ถูกเขียน; `.ustx` bytes ไม่มี ID.
8. Save As → `project_id` ใหม่, entity ID คงเดิม.

**Curve semantics**
9. Stroke เดียวกันให้ canonical result เท่ากันทุก bit ทั้ง native และ WASM.
10. หนึ่ง stroke = หนึ่ง undo step / หนึ่ง revision transition.
11. Invalid stroke (points ว่างใน `replace`, points ใน `clear`, tick นอกช่วง, mode ไม่รองรับ,
    `abbr_mismatch`, `duplicate_curve_descriptor`, `create` โดยไม่มี abbr) → atomic, ไม่มี
    revision/history change.
12. Range/default: ค่าถูก clamp ตาม descriptor min/max; จุดขอบเขตได้ค่า default ที่ถูกต้อง
    (ทดสอบกับ `voic` ที่ default = 100 เพื่อกัน bug `unwrap_or(0)`).
13. Snap/dedupe: ticks snap เป็นทวีคูณของ 5, tick ซ้ำ → last wins, xs strictly increasing,
    `len(xs) == len(ys)`.
14. Undo คืน `xs`/`ys` ก่อนหน้าแบบ byte-exact และคืน `curve_id` เดิม; undo ของ `create`
    ลบเฉพาะ curve นั้น (ไม่ลบเส้นอื่นที่ abbr เดียวกัน — สร้างเคส duplicate abbr เพื่อทดสอบ).
15. `clear` ที่ทำให้ curve ว่าง → curve ถูกลบ; undo คืนทั้ง curve พร้อม `curve_id` เดิม.
16. โหลด USTX ที่ `len(xs) != len(ys)` → repair + warning, **ไม่ panic**.
17. หนาแน่น: stroke 20,000 points ผ่าน limit และให้ผล deterministic.

**Revisions**
18. Test ทุกแถวในตาราง §6.9 (รวม save/export/render ที่ต้องไม่ขยับ revision).
19. Stale `expected_revision` → `revision_conflict`, document/history/render input ไม่เปลี่ยน.
20. Partial read ที่ revision เก่ากว่าไม่ทับ cache ที่ใหม่กว่า (client-side rule, ทดสอบผ่าน harness).

**Groups**
21. Transition ครบ: `none→active→committed` / `→cancelled` / `→failed`.
22. Child fail → tentative ทิ้งทั้งหมด, commit ต่อไม่ได้, ไม่มี partial edit, revision คงที่.
23. `group_cancel` คืนเอกสารเท่ากับก่อน begin เป๊ะ (เทียบ canonical YAML) และ revision/history
    ไม่เปลี่ยน.
24. Commit → revision +1 ครั้งเดียว, undo step 1 อัน, undo แล้วกลับก่อน group ทั้งหมด.
25. Mutation/undo/redo/import ระหว่าง open group → `session_busy` และไม่มีอะไรเปลี่ยน.
26. Nested group → `invalid_group_state`.
27. Close session ระหว่าง open group → ไม่มีการเปลี่ยน committed state.

**Render**
28. Render start จับ revision R; แก้เอกสารหลังจากนั้น → input ของ job นั้นไม่เปลี่ยน (พิสูจน์ด้วย
    hash/ผลลัพธ์เดิม).
29. Cancel ตาม `render_id` ไม่กระทบ job อื่นในคิว (ทดสอบเคส queued 2 jobs — ปิด bug ที่ flag
    global ถูก clear ตอนเริ่ม render).
30. ผลลัพธ์ render ที่ revision เก่ากว่าไม่ถูก publish เป็น current audio อัตโนมัติ.
31. Progress event มี `session_id` + `render_id` + `revision` ครบ.

**Adapter parity**
32. รัน vector ชุดเดียวกันกับ `project-session` ตรง ๆ, WASM adapter และ native adapter →
    เทียบ normalized snapshot/change/revision outcomes (ไม่นับ transport field เช่น
    session_id/route/header).
33. Capabilities: features/limits ที่ประกาศตรงกับพฤติกรรมจริง (เช่น ถ้าไม่ declare
    `partial_reads` ต้องไม่ตอบ 404 แบบงง แต่ต้องบอกใน capabilities).

### 6.18 Implementation gaps and non-blocking follow-ups

ข้อที่ **ปิดแล้ว** (2026-09-29) — ดูรายละเอียดที่อ้างถึง:

1. ~~Fingerprint field set~~ — **ปิด.** ชุด field ถูกตรึงเป็น `id_schema_version = 1` และ
   **ตรวจกับ OpenUtau จริงแล้ว** (§6.2.4): probe ผ่าน `OpenUtau.Core.dll` ไม่พบ field ใน
   fingerprint เปลี่ยนเลย. หลังใช้งานจริง ห้ามแก้เงียบ ๆ — ต้อง bump `id_schema_version`.
2. ~~Default ของ `tracks`~~ — **ปิด.** ซ่อมทั้ง "ไม่มี key" และ `tracks: []` เป็น `Track1`
   พร้อม warning `missing_tracks_repaired` (§6.2.5); ส่วน part ที่อ้าง track ที่ไม่มีถูกรายงาน
   เป็น `part_references_missing_track` โดยไม่สร้าง track เกิน.
3. ~~Idle TTL default~~ — **ปิด.** 30 นาที, reset ด้วย activity, ไม่ expire ขณะมี render job
   `queued`/`running` (§6.12).
4. ~~ผู้เขียน sidecar~~ — **ปิด.** เขียนผ่าน session v2 save path เท่านั้น; legacy `/project`
   ไม่เปลี่ยน semantics (§6.2.4).

ข้อกำหนด logical ต่อไปนี้ปิดแล้ว; รายการที่ยังเหลือเป็น implementation/release gates เท่านั้น:

5. **Command discriminators, result/change field names, route names และ status code สุดท้าย** —
   logical DTO spellings ถูกกำหนดใน §6.20; adapter คง host-specific transport. Child failure
   ใช้ `group_failed` และปิด/discard group; absent/closed group ใช้ `invalid_group_state`.
6. **Progress transport** — **Decision 2026-09-30:** native polling เป็น baseline; SSE/WebSocket
   เป็น optional. WASM session async render ประกาศ unavailable จนมี worker adapter.
7. **Extra key `resolution: 1` ที่มีอยู่ในไฟล์เดิม (เขียนโดย Dart ที่ `app/lib/editor_ustx.dart:59`,
   พบใน 13 ไฟล์ `.ustx`)** — ตรวจแล้วว่า OpenUtau drop key นี้ และ Rust ไม่ serialize
   `resolution` (เป็น `const fn` ที่ `domain/src/project.rs` ไม่ใช่ field) จึงไม่กระทบ
   ID/identity. ยังไม่ตัดสินว่าจะทำความสะอาดตอน save หรือปล่อยไว้; ไม่ block.
8. **Third-party `.ustx` consumers** — พิสูจน์เฉพาะ OpenUtau; consumer ที่ strict กว่า
   (หรือ serde ที่ `deny_unknown_fields`) อาจ reject ไฟล์ที่มี key แปลก. เป็นเหตุผลเสริม
   ที่ทำให้ไม่ฝัง key เพิ่ม; ไม่ block.

**Documented follow-up ที่จงใจยังไม่แก้ในงานนี้ (decision 2026-09-29):**

9. `/post-fx` รับ raw WAV เป็น request body → ติด axum default 2 MiB (≈23.8 s mono 16-bit
   44.1 kHz). v2 ต้องไม่ย้าย audio ไปฝั่ง request; ของเดิมยังเป็น limitation ที่บันทึกไว้
   (`handlers.rs:50-62` map เป็น 400 ด้วยข้อความ axum — v2 ต้องเป็น 413 + `payload_too_large`).
10. render cache เขียน `res-*.bin` แล้ว **ไม่มี eviction** (`render_service.rs:25,407-413`;
    ไม่มีตัวลบใน `clean-cache.sh`) — ความเสี่ยงเรื่องดิสก์โตบนอุปกรณ์.
11. badapple native render **ไม่ scale ตาม threads** (`wasm-performance.md:99-102`) — ยังไม่มี
   คำอธิบาย; กระทบ SLO ที่ยังไม่ควรสัญญา ไม่กระทบ semantics (§6.13 ข้อ 10).
12. **Command scope:** first required v2 scope รวม `part.*` และ `project.*`; logical schemas อยู่ใน
   §6.4.4/§6.20. Planner ปัจจุบันยัง reject; implement และรัน adapter vectors ก่อนประกาศ
   product readiness (T004).

### 6.19 Implementation plan (ลำดับที่แนะนำ)

ทุกขั้นระบุไฟล์/ครบ, ความเสี่ยง compatibility, test ที่พิสูจน์ว่าเสร็จ, และ block ขั้นถัดไปหรือไม่.

| # | งาน | ไฟล์/crate หลัก | ความเสี่ยง | Test ที่พิสูจน์ | Block ขั้นถัดไป? |
|---|---|---|---|---|---|
| 1 | Stable IDs + sidecar identity | `native/crates/domain/src/` (id fields/`ids.rs`), `native/crates/project-session/src/` (matching), `native/crates/ustx-codec` (ห้าม serialize ID) | `.ustx` bytes เปลี่ยนโดยไม่ตั้งใจ; `PartialEq`/`locate.rs` พังถ้า ID เข้า equality | USTX byte-identical กับก่อนแก้; vector ข้อ 1–8 | **ใช่** |
| 2 | Session API types | `native/crates/project-session/src/lib.rs` (revision, `changes`, error enum, lazy snapshot, `ids_stable`) | ทำให้ lazy snapshot แล้วที่เดิมพึ่ง `snapshot` ทันที | unit test revision/lazy; vector ข้อ 18–20 | **ใช่** |
| 3 | Group transactions | `native/crates/project-session/src/`, `native/crates/commands/src/` (restore-document command), `undo_stack.rs` | undo/redo บน restore step; memory ต่อ session | vector ข้อ 21–27 | ใช่ (สำหรับ Flutter ที่ใช้ group) |
| 4 | Curve command | `native/crates/commands/src/exp_commands.rs` (หรือ command ใหม่), `native/crates/domain/src/curve.rs` (`pub` snap helper + repair), `native/crates/project-session` | deviation จาก OpenUtau `Simplify`; `SetCurveCommand` เดิมยังอยู่ | vector ข้อ 9–17 | ใช่ (สำหรับ curve migration) |
| 5 | Shared conformance vectors | crate ใหม่ เช่น `native/crates/project-session/tests/` + fixture JSON | vector ต้องไม่ผูก transport | รัน vector กับ session ตรง ๆ ผ่าน | ไม่ |
| 6 | WASM adapter (ใหม่) | `native/crates/engine-wasm/src/lib.rs`, `web/app/driver.js` | ต้องคง legacy facade เดิมไว้ | vector ข้อ 32 ฝั่ง WASM; legacy `project_commands.rs` ยังผ่าน | ไม่ |
| 7 | Native v2 session adapter + async render | `native/tools/synth-server/src/` (`state.rs` session registry, `server.rs` routes, `handlers.rs`, `render_service.rs` per-job cancel + snapshot input), `docs/CONTRACT-v1.md` | server วันนี้ไม่มี lock/session เลย; ต้องเพิ่มโดยไม่ทำ legacy route พัง | vector ข้อ 28–33; `/health`+legacy `/render` ยังผ่าน | ใช่ (สำหรับ Flutter) |
| 8 | Flutter note migration | `app/lib/api/engine_client.dart`, `app/lib/screens/editor_screen.dart`, `app/lib/widgets/piano_roll.dart`, `app/test/note_edit_test.dart` | widget test ที่ assert local mutation ต้องเปลี่ยน; undo/redo ที่ว่างอยู่ต้อง wire | Flutter test: dispatch intent + snapshot refresh + group drag + atomic error | ใช่ |
| 9 | Flutter curve migration | `editor_screen.dart`, `piano_roll.dart`, `app/lib/project/server_project_store.dart`, `app/lib/editor_ustx.dart` | **พบ unit bug**: `server_project_store` เขียน `pitd` เป็น `xs = beat*100`, `ys = semitones` (ไม่ใช่ ticks/cents) และ `editor_ustx` เขียน curve เป็น per-note `pitch.data` (ms/tenths) → ต้องแก้ให้ตรง domain พร้อม migration | vector curve + render parity (audio เทียบ native) | ใช่ |
| 10 | Save/open/render migration | `editor_screen.dart` (`_buildCurrentUstx`, `_renderTarget`, `_saveProject`), `web/app/{editor,driver,project-store}.js` | autosave/rename/SAF/OPFS; render ต้องใช้ snapshot ไม่ใช่ path | save→reopen→ID คงเดิม; render จาก session; web pool parity | ใช่ |
| 11 | ลบ legacy Dart authority | `app/lib/editor_ustx.dart` (build/parse), `app/lib/project/project_store.dart`, `file_project_store.dart` | ลบก่อนที่ทุก path migrate = พัง | ไม่มี caller เหลือ (grep) + test ทั้งชุดผ่าน | ไม่ |

**หมายเหตุลำดับ:** ขั้น 1–5 เป็นรากฐานและไม่แตะ UI; ขั้น 8–10 เป็นงานย้ายจริงและต้องรอ
adapter (ขั้น 7) พร้อม; **ห้ามลบ legacy API ก่อนขั้น 11** และต้องมี test ที่พิสูจน์ว่าไม่มี
caller เหลือ.

### 6.20 Canonical JSON DTO catalogue (FROZEN)

All examples below are normative JSON shapes. Unknown fields in request objects are rejected
with `validation_error`; response consumers ignore unknown additive fields. Enum values are
closed within a schema version. A breaking field/type/meaning change increments the relevant
schema version. Required command support is advertised per adapter only after its conformance
group passes.

#### Shared logical DTOs

```json
{"expected_revision":12,"command":{"type":"part.move","part_id":"uuid","position_ticks":960},"include_snapshot":false,"group_id":null}
```

`MutationRequest` is `{expected_revision:u64, command:Command, include_snapshot?:boolean=false,
group_id?:EntityId|null}`. `Command` is a closed tagged object with discriminator `type`; its
variants and required fields are the note/track/curve variants in §§6.4–6.6 and the project/part
variants in §6.4.4. `EntityId` is an opaque lowercase UUID string per §6.2. `Tick` is an integer
in the stated project-relative or part-relative domain; fractional values, NaN and infinity are
invalid.

The required note/track command field sets are:

| `type` | Required fields besides `type` |
|---|---|
| `note.add` | `part_id`, `tone`, `position_ticks`, `duration_ticks`, optional `lyric` |
| `note.remove` | `note_id` |
| `note.set_lyric` | `note_id`, `lyric` |
| `note.set_tone` | `note_id`, `tone` |
| `note.move` | `note_id`, optional `delta_position_ticks` (default 0), optional `delta_tone` (default 0) |
| `note.resize` | `note_id`, `delta_duration_ticks`, optional `from_start` (default false) |
| `track.rename` | `track_id`, `name` |
| `track.set_mute` / `track.set_solo` | `track_id`, matching boolean field `mute` / `solo` |
| `track.set_volume_db` | `track_id`, `volume_db` |
| `track.set_pan` | `track_id`, `pan` |
| `curve.apply_stroke` | Exact object in §6.6.2 |

Note add positions/durations are part-relative ticks; tone is an integer MIDI tone and duration
must be positive. Delta fields are signed integers. Rename/name and lyric are strings; volume and
pan are finite numbers validated to the domain range. All referenced IDs must resolve in the
session and part/track kind constraints apply. Defaults are used only when a field is omitted;
unknown fields are rejected. `note.add` returns the minted `note_id` in its change patch.

```json
{"revision":13,"command_name":"Move part","changed":true,"can_undo":true,"can_redo":false,"changes":[{"kind":"part.changed","authority":"patch","track_id":"uuid","part_id":"uuid","data":{"id":"uuid","kind":"voice","name":"Verse","comment":"","position_ticks":960,"duration_ticks":3840,"media_ref":null,"notes":[],"curves":[]}}]}
```

`MutationOutcome` is `{revision:u64, command_name:string, changed:boolean, can_undo:boolean,
can_redo:boolean, changes:Change[], snapshot?:Snapshot}`. `Change` is `{kind, authority,
track_id?, part_id?, note_id?, curve_id?, abbr?, start_tick?, end_tick?, data?, reason?}`.
For `authority:"patch"`, `data` is the complete canonical entity/view defined in §6.4.3;
for `invalidate`, `data` is omitted. Inapplicable fields are omitted, never null.

`SessionError` is `{code:string, message:string, details?:object}`; `details` is omitted when
empty. Codes are the set in §6.14. Rejected mutations leave document, revision and history
unchanged. Native maps this logical error once to HTTP status/body; WASM returns the equivalent
logical error directly.

#### Canonical project and part views

```json
{"id":"uuid","name":"Untitled","comment":"","key":0,"tempos":[{"position_tick":0,"bpm":120.0}],"time_signatures":[{"bar_position":0,"numerator":4,"denominator":4}],"bpm":120.0,"time_signature":{"numerator":4,"denominator":4}}
```

`ProjectMetaView` fields are `id`, `name`, `comment`, `key`, sorted `tempos`, sorted
`time_signatures`, and deprecated derived aliases `bpm` and `time_signature` for tick/bar zero.
Tempo positions are nonnegative project ticks and BPM is finite and positive. Exactly one tempo
entry at tick zero is required. Meter bar positions are nonnegative; numerator/denominator are
positive integers and `1920*numerator/denominator` must be an exact positive integer within Tick
range. Exactly one meter entry at bar zero is required. Setting at an existing position replaces
it; removing position zero is invalid; removing a missing position is `not_found`. Key is integer
0..11. Name is nonempty; comment may be empty.

```json
{"id":"uuid","kind":"wave","name":"Backing","comment":"","position_ticks":0,"duration_ticks":9600,"media_ref":"asset:drums.wav","notes":[],"curves":[]}
```

`PartView` fields are `id`, `kind`, `name`, `comment`, `position_ticks`, `duration_ticks`,
`media_ref`, `notes`, `curves`. `kind` is `voice|wave`; `media_ref` is null for voice and required
for wave. It is a nonempty opaque document-relative URI/string owned by the host asset store;
absolute OS paths are forbidden. Hosts keep it stable when moving/saving a project. An unresolved
reference yields `media_unavailable` when the asset is needed and is never silently rewritten.
New parts have empty notes/curves. Name is nonempty; comment may be empty; position >= 0 and
duration > 0. Resize that would exclude a note or curve point rejects atomically. Part ordering is
by position, then stable ID.

#### Part split boundary rule

`part.split` accepts voice parts only and requires `0 < at_tick < duration_ticks`; a note whose
half-open interval crosses the boundary rejects the command. Notes at or after the boundary move
right and subtract `at_tick` from their relative positions. For each curve, sample its original
linear curve at the boundary using domain half-to-even rounding and include that boundary point in
both outputs (right-side tick zero). Existing points before the boundary remain left; points after
move right and are rebased. A point exactly at the boundary is represented once in each output.
Left curve IDs remain; right curves get fresh IDs. Other curve metadata is preserved. Undo
restores the exact original points and IDs.

#### Lifecycle, history, reads, and capabilities DTOs

`CreateSessionRequest` is `{source:Source}` where `Source` is exactly one of
`{type:"new", name?:string, bpm?:finite-positive-number}`,
`{type:"stored_project", name:string}`, or
`{type:"imported_ustx", name:string, ustx:string}`. Create/open returns
`{session_id:string, revision:0, ids_stable:boolean, warnings:Warning[], snapshot:Snapshot,
capabilities:Capabilities}`. `Warning` is `{code:string, count?:u64}`. Save request is
`{name,expected_revision}` and returns `{saved:true,name,revision,sidecar_written:true}`. Save As
adds `project_id` to that result. Portable export returns
`{revision,ustx,sidecar_included:false}`. Close returns `{closed:true}`. Host storage identifiers
are names or opaque media refs, never filesystem paths.

Undo/redo request is `{expected_revision}` and returns `MutationOutcome`. Group begin request is
`{expected_revision,name?:string}` and returns `{group_id,base_revision,revision}`; child
mutations carry `group_id`; commit/cancel requests are `{expected_revision,group_id}` and return
normal outcome. Failed child operations return `group_failed` and discard the group; absent or
closed groups return `invalid_group_state`.

Bounded read results are the envelopes in §6.10; each carries `revision` and relevant stable IDs.
The shared logical capabilities descriptor is
`{contract_version,command_schema_version,snapshot_schema_version,id_schema_version,features,
limits:{max_command_bytes,max_stroke_points},render:{async_jobs,per_job_cancel,progress_transports}}`.
Host extensions live under `host` and may report engines, idle TTL, import limits and transport
methods. Native requires polling; SSE/WebSocket are optional. WASM reports async jobs and
per-job cancel unavailable until its worker adapter implements them. A feature is listed only
after its adapter passes that conformance group. Progress uses
`{session_id,render_id,revision,done_chunks,total_chunks,elapsed_ms}`; completion is tagged with
the same session/revision and an audio/artifact reference. A result for a revision other than the
current committed revision is marked stale and cannot silently become current audio.

This catalogue is the frozen logical baseline. Golden fixtures may add examples but may not
redefine fields or behavior; changes follow the schema versioning rules above.

---
*The §6 logical standard is frozen. Adapter implementation and cross-host conformance remain
tracked separately in T004/T011 and must pass before claiming v2 product readiness.*
