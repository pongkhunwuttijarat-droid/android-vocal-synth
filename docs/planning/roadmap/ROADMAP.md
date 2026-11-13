# Roadmap

> ลำดับการพัฒนา Android Voice Synth — engine-first
> อัปเดต: 2026-09-12

---

## Milestones Overview

```
MS1 ✅ ─────► MS2 ✅ ─────► MS3 🟡 ─────► MS4 🟡
Main Engine    Worldline      Renderers       Productize
(Rust core)    + golden       native/WASM     Flutter + Web
                  │               │               │
                  └──── pure-Rust WORLDLINE-R / WASM migration ────┘
```

---

## MS1 — Main Engine Core (engine-first)

**เป้าหมาย:** Rust core ครบ — domain + commands + phonemizer + feed + runtime (test ได้โดยไม่ต้อง .so)

| Sprint | เนื้อหา | Status |
|---|---|---|
| 1.1 | Domain model (.ustx compat) + TimeAxis + expressions | ✅ |
| 1.2 | Command system + undo/redo | ✅ |
| 1.3 | Phonemizer (JP VCV / EN CVVC) + oto parser | ✅ |
| 1.4 | Feed pipeline (12 transformers) + RenderInput | ✅ |
| 1.5 | Runtime: scheduler/chunker/cache/mixer | ✅ |
| P.1-P.4 | Plugin track (คู่ขนาน): plugin ABI, FFI smoke, NDK build, golden ref | ✅ |

**Done:** engine core ครบ + FFI smoke test ผ่าน (prebuilt .so)

## MS2 — Worldline Integration + Test

**เป้าหมาย:** engine ↔ libworldline.so → render .ustx → golden ผ่าน → Android

| Sprint | เนื้อหา | Status |
|---|---|---|
| 2.1 | Worldline plugin crate + NDK build | ✅ |
| 2.2 | Render full project (phrase grouping + mixer) | ✅ |
| 2.3 | Golden test vs OpenUtau desktop | ✅ Current acceptance; ดูหมายเหตุ timing/alignment ใน MS2 |
| 2.4 | LAN API + Android (Mi Pad) | ✅ |

## MS3 — Renderers เต็ม

| Sprint | เนื้อหา | Status |
|---|---|---|
| 3.1 | classic mode (UTAU flags compat) | 🟡 Pure-Rust resampler parity ผ่าน; flags ยังไม่ครบ |
| 3.2 | worldline v2 (WORLD + NSF-HiFiGAN) | ✅ Native + golden |
| 3.3 | DiffSinger (onnxruntime + variance/pitch) | 🟡 Tensor→WAV ผ่าน; timing/device ยังเหลือ |
| 3.4 | Vogen (f0_man + VSVocoder) | ⬜ |
| 3.5 | Cache + performance (1 note < 1s, RAM < 1GB) | 🟡 Cache/desktop benchmark ผ่านบางส่วน; device/stress gate ยังเหลือ |

## MS4 — Productize (กำลังทำ)

| Sprint | เนื้อหา | Status |
|---|---|---|
| 4.1 | Flutter UI tablet pen-first (อ้างอิง VocalSona) | 🟡 Editor ใช้งานและแก้โน้ตได้ |
| 4.2 | Wire engine + playback | 🟡 ใช้ LAN synth-server; ยังไม่ใช่ final offline/native wire |
| 4.3 | Import/Export (SAF) + voicebank manager | 🟡 Project save/open/autosave + WAV save + live list มีแล้ว; SAF import flow ยังเหลือ |
| 4.4 | Enunu + polish | ⬜ |

## Architecture policy — engine-owned state

**นโยบายใหม่ (2026-09-29):** Rust engine เป็น source of truth ของ project/musical state ทุก host; Flutter/Web เป็น UI-only ส่ง commands และแสดง snapshots. Web ใช้ `engine-wasm` pattern นี้อยู่แล้ว ส่วน Flutter ยังมี Dart-owned `_editedNotes`, `_editedCurves` และการประกอบ USTX ที่ต้องย้ายออก ดู [Engine-owned state policy and migration](../../architecture/engine-owned-state.md). อย่าเพิ่ม authoritative project state หรือ musical rules ใหม่ใน frontend ระหว่าง migration. **เริ่ม migration slice:** `ProjectDoc`/command semantics ถูกแยกเป็น platform-neutral `project-session` crate และ `engine-wasm` re-export API เดิม; shared tests ผ่าน 4/4 (รวม group cancellation/rollback) และ WASM project command regression ผ่าน 8/8. ยังไม่ใช่ Flutter migration: native session API, curve commands และ render-from-session ยังเป็นงานถัดไป.

**Implementation plan + draft contract:** ลำดับงาน 11 ขั้นอยู่ใน [CONTRACT-v1.md §6.19](../../CONTRACT-v1.md#619-implementation-plan-ลำดบทรแนะนำ) และสัญญา §6 (implementation-ready draft) อยู่ในเอกสารเดียวกัน; ข้อที่ยังไม่ freeze อยู่ใน §6.18.

**Slice 1 ของขั้นที่ 1 (stable identity) — เสร็จ:** `project-session::identity` ให้ ID แบบ opaque UUIDv4 ที่ Rust mint เท่านั้น (seed inject ได้), fingerprint ต่อ entity kind, `IdentityTable` + sidecar `rusted-utau.ids/1` (`table_to_json`/`table_from_json`), matching Pass A/A2/B + orphan tombstone + repair ID ซ้ำ/ผิดรูป, และ `fork_for_save_as`. ฝั่ง domain เพิ่ม load-time hardening: `CurveRepair` + `after_load_with_warnings` (ตัด `xs`/`ys` ที่ยาวไม่เท่ากันแทนการ panic, รายงาน abbr ที่ไม่ลงทะเบียน/ซ้ำโดยไม่ลบข้อมูล) และ `validate()` คืน default 4/4 + 120 BPM ของ OpenUtau เมื่อไฟล์ไม่มี `time_signatures`/`tempos` (เดิม panic).

**Slice 2 ของขั้นที่ 2 (session core บางส่วน) — เสร็จ:** `project-session::Session` รวมเอกสาร authoritative + identity + revision ตาม §6.9: revision เริ่ม 0, +1 ต่อ committed change, rejected/no-op ไม่ขยับ; `snapshot_json()` คืน DTO §6.3 พร้อม ID ครบ (project/track/part/note/curve); `save_sidecar()`/`identity_json()`; `ids_stable` + warnings (map `CurveRepair` → warning codes); `ProjectDoc::from_project` ให้ caller เก็บ load warnings ได้. **Group commands ถูกปฏิเสธด้วย `unsupported_operation`** ที่ชั้น session จนกว่า §6.8 จะ implement เพื่อไม่ให้ revision ไม่ตรงตาราง. **หลักฐาน:** session 15/15, identity 14/14, curve-repair 7/7, domain 34, `engine-wasm`/`commands`/`ustx-codec` ผ่านทั้งหมด; `.ustx` ไม่มี identity (test).

**ยังค้างในขั้นนี้:** group transactions (§6.8), `curve.apply_stroke` (§6.6), คำสั่ง `part.*`/`project.*`, render job model, capability descriptor + payload limits และ sidecar file I/O ที่ host adapter.

**Slice 5 ของขั้นที่ 4–5 (curve commands + capabilities + conformance vectors) — เสร็จ:** implement `curve.apply_stroke` ตาม §6.6 ทั้ง mode `replace` (ขอบเขต exclusive, คงค่าขอบด้วย descriptor default ไม่ใช่ 0) และ `clear` (inclusive, ลบ curve เมื่อว่าง) — canonicalization อยู่ใน Rust (`commands::curve_commands::canonicalize`: stable sort ตาม `(tick, ลำดับ input)` → snap 5 tick ด้วย half-to-even → duplicate ตัวหลังชนะ → clamp ค่าตาม descriptor) และ commit เป็น `ApplyCurveStrokeCommand` ที่ snapshot curve list ของ part ไว้ทำ undo แบบ byte-exact; v2 เพิ่ม rejection reasons ที่ machine-readable (`tick_out_of_range`, `stroke_point_out_of_range`, `empty_stroke`, `unexpected_points`, `abbr_mismatch`, `duplicate_curve_descriptor`, `unsupported_curve_descriptor`) และ change `curve.changed` ที่ patch canonical points / invalidate เมื่อ curve ถูกลบ. เพิ่ม **§6.15 ฝั่ง session**: `capabilities()` + บังคับ `max_command_bytes` (1 MiB) และ `max_stroke_points` (20k) จริง. เพิ่ม **§6.17 conformance vectors**: `tests/vectors/session_vectors.json` 14 vectors + runner ที่อ้าง entity ด้วย `@note:0`/`@curve:0`/`@last_note:` จึงยกไปรันกับ native/WASM adapter ได้. **หลักฐาน:** domain 66+, commands 80 (curve 9 + document 2 + เดิม), project-session 76 (curves 10 + groups 13 + identity 17 + v2 16 + session 4 + vectors 2 + …), engine-wasm/ustx-codec/synth-cli/runtime/feed ผ่านทั้งหมด.

**Decisions 2026-09-29 (ผู้ใช้ตัดสิน):** (1) ไฟล์ที่ไม่มี `tracks` หรือ `tracks: []` ให้ Rust ซ่อมเป็น `Track1` แบบ OpenUtau + warning `missing_tracks_repaired` — **implement แล้ว** (load-repair 11 tests, session แสดง warning code); part ที่อ้าง track ที่ไม่มีถูกรายงาน `part_references_missing_track`. (2) native session idle TTL 30 นาที, reset ด้วย activity, ห้าม expire ขณะมี render job queued/running — freeze ใน §6.12. (3) sidecar เขียนผ่าน session v2 save path เท่านั้น; legacy `/project` ไม่เปลี่ยน semantics. (4) **fingerprint round-trip probe กับ `OpenUtau.Core.dll` รันแล้ว:** โปรเจกต์ 2 tracks/2 parts/3 notes/1 curve ผ่าน OpenUtau load→save ไม่มี field ใน fingerprint เปลี่ยนเลย (เปลี่ยนแค่ `part.duration` 960→1440 ซึ่งจงใจไม่ใช้) → `id_schema_version = 1` เป็น baseline ได้; หลังใช้งานจริงห้ามแก้เงียบ ๆ ต้อง bump version. (5) `/post-fx` 2 MiB, render-cache eviction, native thread scaling = documented follow-up (§6.18 ข้อ 9–11). (6) ยังไม่ commit — แยกไฟล์ของงานนี้ให้ตรวจได้ก่อน.

**Slice 4 ของขั้นที่ 3 (group transactions) — เสร็จ:** implement §6.8 ครบ — `Session::group_begin` สร้าง tentative แยก (clone-on-begin ตามที่ตัดสินไว้ พร้อม lazy snapshot จึงไม่ serialize ตอนเปิด), child ส่งผ่าน `apply_v2` ที่มี `group_id`, ไม่ขยับ public revision และไม่คืน `changes`; child ที่ล้มปิด group ทันทีด้วย `group_failed` (ไม่มี partial commit); `group_commit` ติดตั้ง tentative เป็นเอกสารจริงด้วย `RestoreDocumentCommand` หนึ่งตัว → หนึ่ง undo step + revision +1 ครั้งเดียว; `group_cancel` คืนค่าเดิมเป๊ะและไม่เพิ่มประวัติ; ระหว่างมี group เปิดอยู่ mutation/undo/redo/legacy path ตอบ `session_busy`; planning error (`not_found`) ไม่ทำให้ group ตาย. **หลักฐาน:** `tests/session_groups.rs` 13/13 + `commands::RestoreDocumentCommand` 2 tests; domain 66 / commands 71 / project-session 64 / engine-wasm / ustx-codec ผ่านทั้งหมด.

**Slice 3 ของขั้นที่ 2 (v2 command surface) — เสร็จ:** เพิ่ม `project-session::v2` ให้สัญญา v2 ใช้งานได้จริง: typed error codes (§6.14), mutation envelope ที่บังคับ `expected_revision` (+ `revision_conflict`), change vocabulary ที่มีกฎ `authority: patch|invalidate` ชัดเจน, **คำสั่งที่อ้าง stable ID** (`note.add/remove/set_lyric/set_tone/move/resize` และ `track.set_mute/set_solo/set_volume_db/set_pan/rename`) โดย translate ไปเป็นคำสั่ง index เดิม เพื่อให้ validation/undo ยังเป็นชุดเดียว, bounded reads (§6.10) และ `views.rs` ที่ใช้ DTO ชุดเดียวกันระหว่าง snapshot กับ change payload (กัน field drift). เพิ่ม **lazy snapshot**: `ProjectDoc` เลื่อน serialize จนมีคนขอ (เดิมจ่าย 38.4 ms ต่อคำสั่งบน badapple) — มี test `a_command_defers_snapshot_serialization`. `curve.*`/`part.*`/`project.*` ตอบ `unsupported_operation` (ไม่ใช่ validation error) เพื่อไม่ให้ client เข้าใจผิดว่าใช้ได้. **หลักฐาน:** project-session 49 tests (identity 14 + session 16 + v2 16 + envelope/legacy 3), `engine-wasm` 8/8 (legacy facade ไม่กระทบ), commands/ustx-codec ผ่าน.

## WASM/Web migration track

| งาน | Status |
|---|---|
| Pure-Rust WORLDLINE-R + Classic kernel | ✅ เทียบ native `.so` ผ่าน |
| `engine-wasm` in-memory project/command/render API | ✅ 11/11 tests ผ่าน (2026-09-12) |
| Browser engine smoke | ✅ PCM/WAV ตรง native fixture |
| Worker pool + per-chunk loading/incremental cache | ✅ Test harness ผ่านและ bit-identical |
| Browser editor vertical slice | ✅ Boot/canvas/edit/undo/draw/marquee/render/zoom ผ่าน headless test |
| ต่อ editor เข้ากับ RenderPool + progress/cancel | ✅ 2026-09-23 — editor ใช้ pool แล้ว (แก้ 1 note = 1/6 chunk, cancel ได้) |
| OPFS project/voicebank flow, multi-track/mixer, curves | 🟡 project store + autosave/export ทำแล้ว (2026-09-23); voicebank OPFS, mixer, curves ยังเหลือ |
| Service worker/PWA, deploy, neural engines บน WASM | ⬜ |

อ้างอิง: [WASM migration status](../wasm-migration-status.md) ·
[Web UI implementation plan](../web-ui-design.md)

## Verification snapshot (2026-09-12)

| ชุดตรวจ | ผล |
|---|---|
| `cargo test -p engine-wasm` | ✅ 11/11 |
| `web/run-smoke.sh` | ✅ PCM/WAV fixture PASS |
| `web/run-pool.sh` | ✅ cold/cache/one-note incremental bit-identical |
| `web/editor-test.html` (headless Chromium) | ✅ Vertical slice PASS |
| `web/run-editor.sh` — pool wiring (2026-09-23) | ✅ แก้ 1 note → 1/6 chunk · ไม่แก้อะไร → 0/6 chunk (2 ms) · cancel ทิ้งผล |
| **ไฟล์จริง**: `web/run-real.sh` vs native `r1-rs` (2026-09-23) | ✅ เพลงจริง + bank จริง 558 wav → `cmp` bit-identical (302162 bytes) |
| `flutter test --no-pub` | 🟡 48 pass, 9 skipped, 2 fail ใน `note_edit_test.dart` |

Flutter failures ปัจจุบันอยู่ที่ gesture test ของ lyric tap และ right-edge resize:
log แสดงว่า test หา vertical `Scrollable`/พิกัด roll ผิดหลัง layout มี nested scroll
(`resize=null`, dialog ไม่เปิด). จึงจัดเป็น **test-harness/integration issue ที่ต้อง
วินิจฉัยต่อ** ยังไม่ใช้เป็นหลักฐานว่าฟีเจอร์บน device regression และไม่ปิด MS4.

---

## เหตุผล engine-first

```
1. .so บน Android = พิสูจน์แล้ว (OpenUtau Mobile prebuilt + เคยใช้)
   → ไม่ต้อง "พิสูจน์ความเสี่ยง" ก่อน
2. Engine = งานใหญ่สุด + test ได้เอง (cargo test ไม่ต้อง .so)
3. Feed/domain เป็นของที่ทุก plugin ต้องใช้ → ทำก่อนเสมอ
4. Plugin ต่อท้ายได้เลย (มี contract + prebuilt .so)
```
