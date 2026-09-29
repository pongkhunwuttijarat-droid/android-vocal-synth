# Disk Hygiene — คุมขนาด build cache

บริบท: โปรเจกต์นี้อยู่บน NTFS (`/media/DATA`, 142 GB) พาร์ทิชันเดียวกับ workspace
อื่น ๆ ทั้งหมด — build cache เคยโตจนดิสก์เหลือไม่ถึง 7 GB (ใช้ไป 97%) และรอบล่าสุด
(2026-09-23) ใช้ไป 103 GB = 73% ก่อนล้าง

> เดิมโปรเจกต์อยู่บน exfat (`/run/media/seal/shared`, 218 GB) — ข้อ 7 เป็นข้อจำกัด
> ที่เจอบน exfat ตอนนั้น ปัจจุบัน NTFS ยังมีอาการ `rm` ล้มกลางทางแบบเดียวกัน
> สคริปต์จึงยังจัดการให้เหมือนเดิม

## 1. ทำไม `native/target` เคยโต ~50 GB

`native/Cargo.toml` เดิมไม่มี `[profile.dev]` เลย → Cargo ใช้ค่ามาตรฐาน
`debug = true` ซึ่งฝัง DWARF debug info เต็มรูปแบบ วัดจากไฟล์จริงในตอนนั้น:

| ไฟล์ | ขนาดรวม | debug info | สัดส่วน |
|---|---|---|---|
| `trainer-gui` | 813 MB | 608 MB | 75% |
| `moe-train` | 790 MB | 594 MB | 75% |
| `synth-cli` | 748 MB | 567 MB | 75% |

`native/target/debug/deps` อย่างเดียว = 41 GB จาก 4,374 ไฟล์
(`native/target` รวมทั้งหมด ~52 GB)

## 2. สิ่งที่ตั้งค่าไว้แล้ว

ใน `native/Cargo.toml`:

| profile | ค่า | ผล |
|---|---|---|
| `[profile.dev]` | `debug = 1` | เก็บแค่ line tables — backtrace ยังบอก `ไฟล์:บรรทัด` ได้ แต่เล็กลงหลายเท่า |
| `[profile.dev.package."*"]` | `debug = false` | dependency ภายนอก (wgpu / ort / image …) ไม่มี debug info เลย — ก้อนนี้คือส่วนที่กินพื้นที่มากสุด |
| `[profile.test]` | เหมือน dev | `cargo test` ไม่บาน |
| `[profile.release]` | `strip = "debuginfo"` | binary ที่ copy ไป `jniLibs` (APK) เล็กลง |

และ `native/.cargo/config.toml` เพิ่ม alias:

```bash
cd native
cargo size          # ดูว่า target กินเท่าไร
cargo clean-cache   # ดูรายการที่จะลบ (dry-run เสมอถ้าไม่เติม -- --apply)
```

> หมายเหตุ: `[profile.*]` ต้องอยู่ใน workspace manifest (`native/Cargo.toml`)
> ไม่ใช่ใน `.cargo/config.toml` — และ `[workspace.targets]` ที่เคยมีอยู่นั้น
> ไม่ใช่ key ของ Cargo จึงไม่มีผลอะไร (ลบออกแล้ว)

### ผลที่วัดได้จริง (full workspace build, 12 cores)

| | ก่อน | หลัง | ลดลง |
|---|---|---|---|
| `native/target` (ทั้งก้อน) | 52 GB | **13 GB** | 4× |
| `native/target/debug/deps` | 41 GB | **6.2 GB** | 6.6× |
| `.debug_info` ใน `trainer-gui` | 608 MB | **7.7 MB** | 79× |
| `.debug_line` ใน `trainer-gui` | ~14 MB | ~14 MB | คงไว้ (backtrace ยังบอกบรรทัด) |

ขนาด binary ที่ได้:

| binary | ก่อน | หลัง |
|---|---|---|
| `trainer-gui` | 813 MB | 342 MB |
| `moe-train` | 790 MB | 330 MB |
| `synth-server` | 774 MB | 301 MB |
| `world-oracle` | 601 MB | 160 MB |
| `train-prep` | 590 MB | 149 MB |
| `dict-check` | 16 MB | 9.4 MB |

build ยังใช้งานได้ปกติ (`cargo build` ผ่าน, binary รันได้) และ dependency ทุกตัว
ไม่มี `.debug_info` เหลืออยู่

## 3. ล้าง cache ด้วยสคริปต์

`scripts/clean-cache.sh` — **dry-run เป็นค่า default** ต้องเติม `--apply` จึงลบจริง

```bash
scripts/clean-cache.sh                                   # ดูขนาดเฉย ๆ
scripts/clean-cache.sh --all --apply                     # ล้าง cache ทุกชนิด
scripts/clean-cache.sh --rust --apply                    # เฉพาะ Rust
scripts/clean-cache.sh --flutter --apply                 # app/build + gradle
scripts/clean-cache.sh --dotnet --apply                  # .NET bin/obj (รวม ref copies)
scripts/clean-cache.sh --poc --apply                     # output กลางทางใน test/poc
scripts/clean-cache.sh --stale --apply                   # artifact เก่ากว่า 7 วัน (debug/wasm)
scripts/clean-cache.sh --orphan --apply                  # target/cargo ของ agent ที่ค้าง
scripts/clean-cache.sh --auto --threshold 20G --apply     # สำหรับ timer/cron
```

| flag | ลบอะไร | สร้างใหม่ด้วย |
|---|---|---|
| `--rust` | `native/target`, `native/build-debug`, `native/crates/*/target` | `cargo build` |
| `--flutter` | `app/build`, `app/android/.gradle`, `app/android/.kotlin` | `flutter build` |
| `--dotnet` | `golden-renderer/{bin,obj}` + `ref*/core/*/{bin,obj}` | `dotnet build` |
| `--poc` | `test/poc/renders`, `student-*`, `*teacher-wavs` | รัน eval pipeline ใหม่ |
| `--stale` | ไฟล์ใน `target/debug` + `target/wasm32-unknown-unknown` ที่เก่ากว่า `--stale-days` (default 7) | `cargo build` |
| `--orphan` | `.<name>-target` / `.<name>-cargo*` ที่ค้างใน workspace root (เก่ากว่า `--orphan-days`, default 1) — ข้ามโฟลเดอร์ที่มีไฟล์ใหม่ | ไม่ต้อง — ของทิ้ง |
| `--include-release` | ให้ `--stale` ครอบ `target/release` ด้วย (ปิด default เพราะ lto แพง) | `cargo build --release` |
| `--auto` | ทุกชนิดข้างบน **เฉพาะเมื่อรวมแล้วเกิน `--threshold`** (default 20G) | — |

`--poc` **ไม่แตะ** dataset (`train-*.jsonl`, `*.tsbin`) และโมเดล (`*.mpk`)

### รูรั่ว 3 ทางที่ทำให้ cache บาน (2026-09-23)

| รูรั่ว | ขนาดที่พบ | ปิดด้วย |
|---|---|---|
| session ของ agent ตั้ง `CARGO_TARGET_DIR`/`CARGO_HOME` เป็น dot-dir แล้วทิ้งไว้ | 21 GB (5 target + 6 cargo) | `--orphan` + กฎใน `AGENTS.md` |
| artifact เก่าใน `target/debug` ไม่มีใครลบ (cargo เก็บ 1 ชุดต่อ fingerprint ไม่ทิ้งของเก่า) | 40 GB จาก 40,366 ไฟล์ | `--stale` |
| ไม่มีอะไรรันอัตโนมัติ | — | systemd timer (ข้อ 4) |

> **กับดัก locale — ห้ามใช้ date string กับ `find -newermt`**
> เครื่องนี้ `LANG=th_TH.UTF-8` ทำให้ `date` พ่นปี พ.ศ. (`2569-09-22`)
> แต่ `find` ตีความตัวเลขนั้นเป็น ค.ศ. => cutoff ไปอยู่ปี 2569 จริง ๆ =>
> ทุกไฟล์กลายเป็น "เก่ากว่า" และถูกลบทั้งก้อน (เกิดจริง: `target/debug` หาย 40 GB
> แทนที่จะเป็น 30 GB) สคริปต์จึงใช้ epoch: `find -newermt "@$ref"`
> เทียบให้เห็น: cutoff แบบ epoch -> 9,586 ไฟล์ถูกเก็บ / แบบ date string -> 0 ไฟล์

## 4. ล้างอัตโนมัติ (systemd user timer)

**สถานะ: ติดตั้งและเปิดใช้งานแล้ว** (2026-09-23) — ครั้งถัดไป 24 ก.ย. ~04:00
รันทุกวัน 04:00 (± สุ่มไม่เกิน 1 ชม.) เพราะรอบนี้ cache โต 21 GB ใน 6 วัน
รายสัปดาห์จึงช้าเกินไป

unit อยู่ใน `scripts/systemd/` มี `ExecStart` 2 บรรทัด:

1. `--orphan --apply` — ลบของที่ session ทิ้งไว้ **ทุกครั้ง** (ไม่ต้องรอเกิน threshold)
2. `--auto --threshold 20G --apply` — ล้าง cache ชนิดอื่นเมื่อรวมแล้วเกิน 20 GB

path ใน unit อ้างตำแหน่ง checkout ปัจจุบัน ถ้าย้ายโปรเจกต์ต้องแก้ `ExecStart` ด้วย

```bash
# ใช้ symlink — repo เป็น source of truth แก้ไฟล์ใน repo แล้วมีผลเลย
mkdir -p ~/.config/systemd/user
ln -sfn "$PWD/scripts/systemd/avs-clean-cache.service" ~/.config/systemd/user/
ln -sfn "$PWD/scripts/systemd/avs-clean-cache.timer"   ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now avs-clean-cache.timer
systemctl --user list-timers avs-clean-cache.timer
loginctl enable-linger "$USER"     # ให้ทำงานแม้ไม่ล็อกอิน
```

ทดสอบทันทีด้วย `systemctl --user start avs-clean-cache.service`
(ถ้า cache ไม่เกิน 20 GB จะไม่ลบอะไร แล้วจบแบบ success)

```bash
systemctl --user show avs-clean-cache.service -p Result   # Result=success
loginctl show-user "$USER" -p Linger                      # Linger=yes
```

(`Linger=yes` แล้ว จึงรันแม้ไม่ล็อกอิน — ไม่ต้อง `sudo`)

ทางเลือกถ้าไม่ใช้ systemd — ใส่ cron รายวัน:

```cron
0 4 * * * /media/DATA/project/android-voice-synth/scripts/clean-cache.sh --orphan --apply --quiet
30 4 * * * /media/DATA/project/android-voice-synth/scripts/clean-cache.sh --auto --threshold 20G --apply --quiet
```

## 5. ห้ามลบ (ไม่ใช่ cache)

| path | เหตุผล |
|---|---|
| `native/build/build-linux`, `native/build/build-android` | `libworldline.so` / `libmixerfx.so` ที่ app ต้องใช้ (`bundle-android.sh`) |
| `native/third_party/world/build` | ไลบรารี WORLD ที่ผูกกับ `worldline-sys` |
| `test/golden/teto-english`, `diffsinger-*` | voicebank + model (input จริง ไม่ได้ regenerate ฟรี) |
| `app/android/app/src/main/assets/engine/**` | voicebank/model ที่ bundle เข้า APK แล้ว |
| `test/poc/**/*.mpk`, `train-*.jsonl`, `*.tsbin` | โมเดลและ dataset ที่เทรนไว้ |

## 6. ถ้ายังไม่พอ — ตัวเลือกเพิ่ม

**ก. ตัด debug info ทิ้งหมด** (backtrace จะเหลือแค่ชื่อฟังก์ชัน ไม่มีบรรทัด):
แก้ `native/Cargo.toml` → `[profile.dev] debug = false`

**ข. ย้าย target ออกจากพาร์ทิชันที่เต็ม** — `/home` (btrfs) เหลือพื้นที่มากกว่า
และเร็วกว่า exfat มาก แต่ต้องแก้ path ที่ hardcode ไว้ 4 จุดก่อน:

- `app/lib/project/ustx_codec.dart:42`
- `app/test/ustx_codec_test.dart:16`
- `app/test/file_project_store_test.dart:15`
- `test/poc/eval_pipeline.sh` (ใช้ `./target/debug/...`)

จากนั้นตั้งใน `native/.cargo/config.toml`:

```toml
[build]
target-dir = "/home/seal/.cache/android-voice-synth-target"
```

> บน exfat ทำ symlink `native/target` แทนไม่ได้ — ระบบไฟล์ไม่รองรับ hard link/symlink

## 7. ข้อจำกัดของ exfat ที่เจอจริง

**ก. `rm -rf native/target` อาจล้มเหลวกลางทาง** — เมื่อมี entry ค้างใน
`target/debug/deps` (stat ไม่ได้) `rm` จะลบโฟลเดอร์แม่ไม่ได้ทั้งที่ข้อมูล
ถูกปล่อยไปแล้ว จะเห็นข้อความ `rm: cannot remove ...: Directory not empty`
และ `du` เหลือ ~1 GB

`clean-cache.sh` จัดการให้แล้วด้วยการไล่ `find -depth -delete` แล้วลอง `rm` ซ้ำ
ถ้ายังเหลือจะเตือนให้รันสคริปต์ซ้ำอีกครั้ง

**ข. build ครั้งแรกหลังลบ target อาจล้มด้วย `Text file busy (os error 26)`**
เวลา exec build script ที่เพิ่งเขียนเสร็จ — เป็นข้อจำกัดของ exfat ไม่ใช่บั๊ก
ของโปรเจกต์ แค่รัน `cargo build` ซ้ำอีกครั้งก็ผ่าน

**ค. hard link / symlink ใช้ไม่ได้** — เครื่องมือที่เขียนไฟล์แบบ
"เขียน temp แล้ว link ทับ" (รวมถึงไฟล์ writer บางตัว) จะได้ `EPERM`
ให้เขียนไฟล์นั้นด้วยวิธีอื่นแทน (เช่น symlink ของ C++ plugin
`native/plugins/worldline/src/worldline` → `.` ที่ถูกลบไปแล้ว; กู้คืนได้บน
ไฟล์ระบบปกติด้วย `ln -s . native/plugins/worldline/src/worldline`)

**ง. `git status` โชว์ไฟล์ "แก้ไข" 3,000+ ไฟล์ทั้งที่เนื้อหาเดิม** — exfat ไม่เก็บ
permission bit ไฟล์ทุกตัวจึงอ่านได้เป็น `0755` ขณะที่ index เป็น `100644` → git
รายงาน mode change ทุกไฟล์ (`git diff --stat` ขึ้น `| 0`)

```sh
git config core.fileMode false     # ตั้งครั้งเดียวต่อ clone (อยู่ใน .git/config)
```

หลังตั้งแล้ว working tree ของ repo นี้เหลือเฉพาะการแก้จริง ~50 ไฟล์ (จาก 3,689)
ค่านี้เป็น local config ไม่ถูก commit — clone ใหม่บน exfat ต้องตั้งซ้ำ


## 8. บันทึกการล้างรอบแรก

| ครั้ง | คำสั่ง | คืนพื้นที่ | พื้นที่ว่างหลังล้าง |
|---|---|---|---|
| 1 | `clean-cache.sh --all --apply` | 58 GB | 4.6 GB → 63 GB |
| 2 | build ใหม่ทั้ง workspace (วัดผล profile ใหม่) | — | target กลับมาแค่ 13 GB |
| 3 | `clean-cache.sh --poc --apply` | 7.6 GB | 51 GB → 59 GB |
| 4 (2026-09-23) | ลบ `.<name>-target` + `.<name>-cargo` ของ agent ที่ค้าง | 21 GB | 40 GB → 61 GB |
| 5 (2026-09-23) | `clean-cache.sh --stale` (**ลบเกินแผน** — ดูหมายเหตุ) | 39 GB | 61 GB → 100 GB |
| 6 (2026-09-23) | `cargo build --workspace` หลังลบ | target กลับมา 13 GB | 100 GB → 94 GB |

หลังรอบที่ 3 `test/poc` เหลือ 6.5 GB ซึ่งเป็น **dataset และโมเดลที่ตั้งใจเก็บไว้**
(`train-*.jsonl` / `*.tsbin` / `*.mpk` — ไม่อยู่ใน git จึงกู้คืนไม่ได้)

สิ่งที่รอบที่ 3 ลบไปเป็น output ที่สร้างซ้ำได้: `renders/`, `student-*` 9 ชุด,
`*teacher-wavs/` — สร้างใหม่ด้วย `test/poc/eval_pipeline.sh`

### หมายเหตุรอบที่ 5 — ลบเกินแผน

ตั้งใจจะลบเฉพาะ artifact เก่ากว่า 7 วัน (30.7 GB) แต่ด้วยกับดัก locale ที่อธิบายใน
ข้อ 3 สคริปต์เวอร์ชันเก่าจึงลบ `target/debug` **ทั้งก้อน 40 GB** รวมของที่บิลด์วันนั้น

ผลกระทบ: ไม่มีข้อมูลสูญหาย (เป็น build output ล้วน) แต่ต้อง `cargo build --workspace`
ใหม่ทั้งรอบ — ใช้เวลา 56 วินาที เพราะ deps ถูกคอมไพล์ขนานกัน

รอบนี้ทำให้เพิ่ม `--stale` ที่ใช้ epoch และเพิ่ม regression test ในหัวข้อ 3
ของเอกสารนี้ สิ่งที่ **ไม่** ถูกแตะเลย: `native/build/**/*.so`,
`target/release`, `target/wasm32-unknown-unknown`, source ทั้งหมด
