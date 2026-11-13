# wasm refresh 2026-09 — รื้อ wasm ให้เป็นของใหม่ (T23)

> วันที่ 2026-09-23 · โจทย์จากผู้ใช้: "ขอ wasm ปรับมาให้เป็นของใหม่ด้วย"
> เป้าหมาย: rebuild target `wasm32-unknown-unknown` ให้ดึง fix ล่าสุดทั้งหมด (phonemizer default/japanese, mixer pre-roll, gap=silence, …) + รัน harness ครบ + วัด baseline ใหม่ + ระบุว่าเว็บยังขาดโหมดไหน

## 0. สรุป (TL;DR)

- **build wasm ผ่าน**: `RUSTFLAGS="-C target-feature=+simd128" cargo build --release --target wasm32-unknown-unknown -p engine-wasm` (SIMD) — recompile ครบทุก crate ที่เกี่ยว (feed/phrase/runtime/worldline-rs/worldline-plugin/synth-cli/engine-wasm) ⇒ ได้ fix ล่าสุดทั้งหมด
- **เว็บเห็น fix แล้ว** (ในระดับ engine): `engine-wasm/src/lib.rs:271,586` เรียก `synth_cli::pipeline::PhonemizerKind::from_track(track)` ⇒ enum `Default` + กติกาเลือก phonemizer ใหม่ถูก compile เข้า wasm
- **แต่เว็บ "ยังไม่มีช่องเลือก" โหมด default**:
  - `std::env::var("RUSTED_UTAU_PHONEMIZER")` **ใช้ไม่ได้ใน wasm32-unknown-unknown** (ไม่มี process env) ⇒ เส้นทาง env ของ CLI ไม่มีอยู่ในเบราว์เซอร์
  - ไม่มี UI/command สำหรับตั้ง phonemizer (มีแค่ field อ่านใน track view model — `engine-wasm/src/project.rs:303-315`)
  - fixture ของเว็บใช้ Universal ทั้งคู่: `web/fixture/demo-song.ustx` → `phonemizer: ''` · `web/fixture/scale-demo.ustx` → `phonemizer: 'Universal'`
  ⇒ **default path ของเว็บ = Universal** (ต่างจาก CLI ที่แนะนำ `--phonemizer default`) — ผลกระทบดู §3

## 1. build + artifacts

- `RUSTFLAGS="-C target-feature=+simd128" cargo build --release --target wasm32-unknown-unknown -p engine-wasm` → **ผ่าน** (Finished release in 6.45s, incremental)
  recompile chain ที่เกี่ยวทั้งหมด: `feed → phrase → runtime → worldline-plugin → synth-cli → engine-wasm` (+ `worldline-rs`, `world-rs`)
- `wasm-bindgen --target web --out-dir web/pkg` → `web/pkg/engine_wasm.js` + `engine_wasm_bg.wasm` ใหม่ (ทำโดย run-smoke/pool/editor เองตามสคริปต์)
- **note เรื่อง ort/getrandom ที่ task ระบุ**: wasm refresh รอบนี้ไม่ต้องแตะ feature ของ `ort` เลย เพราะเส้น wasm ไม่ลาก `ort`/`diffsinger`/`moe-train` (optional, ปิดด้วย default-features=false อยู่แล้ว) — `engine-wasm` ใช้แค่ `worldline-plugin { default-features = false }` + `synth-cli { default-features = false }`
- `moe-wasm` ยัง build ได้ (cargo test ด้านล่าง compile ผ่าน)

## 2. harness + tests

รันทีละตัวด้วย `/tmp/t23-run-all.sh` (log ครบที่ `/tmp/t23-<name>.log`):

| harness | ผล | ตัวเลขสำคัญ |
|---|---|---|
| `cargo test -p domain -p engine-wasm -p moe-wasm` | **PASS** | **66 passed / 0 failed** (รวม integration ของ engine-wasm: memory_render ฯลฯ) |
| `./web/run-smoke.sh` | **PASS** | `render: 132098 samples ... peak=0.9884` · `wav bytes=264240` · aliases=2673 |
| `./web/run-pool.sh` | **FAIL** | pool 639,679 vs sequential 639,685 (len −6, max\|diff\| 0.63) — ดู §2.1 |
| `./web/run-editor.sh` | **PASS** | `audio: 639337 samples peak≈0.659` · pool workers=4 chunks=6 cached=6 · OPFS/cancel ผ่าน |
| `./web/run-vocoder.sh` | **PASS** | 32,768 samples · hop 512 · WAV 65,580 B fnv `1450536567` (เท่า T1 เป๊ะ) · peak 0.0065 · RSS tree 1,464.3 MB / process 405.4 MB |
| `./web/run-r2.sh` | **PASS** | full R2 chain · **parity vs native CPU EP = 1 LSB** (148/32,768 = 0.4517%) · mel mean/audio peak ตรงกัน 6 ตำแหน่ง |

### 2.1 pool FAIL — regression ที่เจอในรอบนี้ (ยังไม่แก้)

อาการ (จาก `native/target/pool-result.txt`):

```
pool 2274 ms | chunks rendered 4 | samples โหลด 2 | per-worker keys 2,2,2,2
plain sequential: 639685 samples
   pool vs sequential: MISMATCH ✘  (len 639679 vs 639685, max|diff| 6.3e-1)
```

สิ่งที่ตรวจแล้ว (ยังไม่พบตัวการ):
- ทั้งสองเส้นทางเรียก **ฟังก์ชัน Rust ตัวเดียวกัน**: `synth_cli::pipeline::render_planned_chunk` (`engine-wasm/src/lib.rs:342`) และ mix ด้วย `runtime::mixer::mix` (ผ่าน `Mixer::finish` `engine-wasm/src/lib.rs:551`) — ไม่ใช่ JS mixer
- sequential (`Engine::render_project`) ก็ใช้ `mix(&chunks)` ไม่มี `mix_min_duration` (`pipeline.rs:762`) ⇒ **ไม่มี padding ต่างกัน**
- plan ของ pool มาจาก `plan_project` + `chunk_meta_json` (`position_ms`/`leading_ms`) — main engine กับ worker ใช้ YAML ตัวเดียวกัน
⇒ สมมติฐานที่เหลือ: (ก) worker re-plan จาก YAML แล้วได้ chunk `own_range`/`leading` ต่างจาก main เล็กน้อย (ตัด buffer ต่างกันไม่กี่ sample)
(ข) `Engine::render_project` กับ path `plan_project + render_chunk` ให้ผลต่างกันในตัว Rust เอง (ต้องมี diagnostic ในเบราว์เซอร์ยืนยัน)
**ขั้นถัดไปที่เสนอ**: patch `web/pool-test.html` ให้พิมพ์ `chunk_meta_json()` + `render_chunk(i).length` ของ main engine เทียบกับที่ pool ได้ แล้วดูว่าต่างที่ chunk ไหน (วินิจฉัยได้ใน ~1 รอบ harness) — งานนี้หมดงบก่อน

## 3. เว็บขาดโหมดไหน / ผลกระทบ

| อย่าง | CLI | เว็บ (wasm) |
|---|---|---|
| เลือก default mode | `--phonemizer default` (flag) หรือ env `RUSTED_UTAU_PHONEMIZER=default` หรือ `track.phonemizer: default` | **เฉพาะ** `track.phonemizer: default` (ต้องแก้ .ustx) |
| env override | ใช้ได้ (ชนะ track) | **ใช้ไม่ได้** (wasm ไม่มี env) |
| UI selector | — | **ไม่มี** |
| default เมื่อไม่ระบุ | Universal | Universal (เท่ากัน) |

**ผลกระทบเมื่อเทียบกับ reference (sakura-test)**: Universal ได้ **2 phrase** (และแมป 'ya' → `A`) ส่วน default mode ได้ **3 phrase** ตรง OpenUtau (ทิ้งโน้ต 'ya' ที่ไม่มี alias) ⇒ เสียง/boundary ต่างกันจริง
(ตัวเลข: docs/planning/default-phonemizer-parity.md · phrase-segmentation-parity.md)

**ทางเลือกที่จะให้เว็บใช้โหมดนี้ได้** (ยังไม่ทำในงานนี้ — รอ Lead ตัดสิน):
1. เพิ่ม fixture/ปุ่มเลือก phonemizer ใน UI แล้วส่งค่าเข้า `track.phonemizer` ผ่าน command (ต้องมี command ใหม่ = แตะ commands/engine-wasm, ไม่ใช่แค่ web/**)
2. expose API `render_project_with_phonemizer(json, track, "default")` ใน engine-wasm (แตะ engine-wasm — อยู่ใน scope T23 ก็จริง แต่เป็นฟีเจอร์ใหม่ ควรมีเทสต์/web UI รองรับ)
3. ชั่วคราว: ทำ fixture ที่ `phonemizer: 'default'` เพื่อพิสูจน์ว่า wasm เคารพค่านี้ (ไม่ต้องแก้ Rust)

## 4. baseline ที่วัดใหม่

ตัวเลขที่เอกสารเดิมอ้างไว้ + สถานะหลัง refresh:

| baseline ในเอกสาร | ค่าเดิม | ค่ารอบนี้ (wasm ใหม่) | สถานะ |
|---|---|---|---|
| `docs/architecture/wasm-performance.md` §0 — wasm sequential/pool (scale-demo) | 6.82 s / 2.16 s | **ยังไม่วัดซ้ำในงานนี้** (ต้องใช้ `run-real.sh` + `PORT` ไม่ได้อยู่ในชุด harness ของ T23) | ⏳ ค้าง |
| `wasm-performance.md` §0.2 — Bad Apple (867 notes, 136 chunks, 504.23 s) | native vs wasm ต่าง 3/22,236,513 sample = **max\|diff\| 1 LSB** | **ยังไม่วัดซ้ำ** (task-22 กำลัง render Bad Apple; ต้อง `PROJECT=/test/golden/badapple.ustx ./web/run-real.sh`) | ⏳ ค้าง |
| `web/README.md` — smoke (demo-song) | 132,066 samples / 264,176 B / peak 0.9884 | **132,098 samples / 264,240 B / peak 0.9884** (+32 samples = mixer pre-roll fix) | ✅ อัปเดตแล้ว |
| `web/README.md` / T1 — vocoder ort-web wasm (nsf_hifigan) | 32,768 samples · hop 512 · fnv 1450536567 | **เท่าเดิมเป๊ะ** (WAV byte-identical) | ✅ ยืนยันซ้ำ |
| T5 — R2 chain parity wasm vs native CPU EP | max\|diff\| = 1 LSB (148/32,768 samples) | **เท่าเดิม: 1 LSB** · mel mean/peak ตรง 6 ตำแหน่ง | ✅ ยืนยันซ้ำ |
| `engine-wasm` in-memory vs disk render | `max\|diff\| = 0` | ยังไม่วัดซ้ำ (harness นี้ไม่มี) | ⏳ ค้าง |

⇒ ตัวเลข parity ที่เป็นเกณฑ์หลัก (vocoder 1 LSB, R2 1 LSB) **ยังจริงหลัง refresh**; ที่เปลี่ยนคือความยาวของ demo-song render (+32 samples) ตาม fix mixer และ **pool path ที่ยังไม่ตรง** (§2.1)

## 5. ไฟล์ที่แตะ

ไฟล์ที่แตะในงานนี้ (write scope T23):
- `docs/planning/wasm-refresh-2026-09.md` (เอกสารนี้ — ใหม่)
- `web/README.md` (อัปเดตตัวเลข smoke: 132066→132098 samples, 264176→264240 bytes)
- `web/pkg/` (regenerate โดยสคริปต์ harness — gitignored)

**ไม่แตะ**: `native/tools/synth-cli/**` (task-21), `native/crates/phonemizer/**`, `native/crates/runtime/src/mixer.rs`, `test/**`, `ref*/**`
(harness scripts `run-smoke/pool/editor/vocoder/r2.sh` รันตามเดิมโดยไม่แก้; `run-vocoder.sh`/`run-r2.sh` เป็นสคริปต์เดิมจาก T1/T5)
logs: `/tmp/t23-<cargo-test|smoke|pool|editor|vocoder|r2>.log` · pool page output: `native/target/pool-result.txt`
