# Disk Hygiene — คุมขนาด build cache

บริบท: โปรเจกต์นี้อยู่บน exfat (`/run/media/seal/shared`, 218 GB) ซึ่งเป็นพาร์ทิชัน
เดียวกับข้อมูลอื่นในเครื่อง — build cache เคยโตจนดิสก์เหลือไม่ถึง 7 GB (ใช้ไป 97%)

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
scripts/clean-cache.sh --auto --threshold 20G --apply     # สำหรับ timer/cron
```

| flag | ลบอะไร | สร้างใหม่ด้วย |
|---|---|---|
| `--rust` | `native/target`, `native/build-debug`, `native/crates/*/target` | `cargo build` |
| `--flutter` | `app/build`, `app/android/.gradle`, `app/android/.kotlin` | `flutter build` |
| `--dotnet` | `golden-renderer/{bin,obj}` + `ref*/core/*/{bin,obj}` | `dotnet build` |
| `--poc` | `test/poc/renders`, `student-*`, `*teacher-wavs` | รัน eval pipeline ใหม่ |
| `--auto` | rust + flutter + dotnet **เฉพาะเมื่อเกิน `--threshold`** (default 20G) | — |

`--poc` **ไม่แตะ** dataset (`train-*.jsonl`, `*.tsbin`) และโมเดล (`*.mpk`)

## 4. ล้างอัตโนมัติ (systemd user timer)

unit อยู่ใน `scripts/systemd/` — path ข้างในอ้างตำแหน่ง checkout ปัจจุบัน
ถ้าย้ายโปรเจกต์ให้แก้ `ExecStart` ด้วย

```bash
mkdir -p ~/.config/systemd/user
cp scripts/systemd/avs-clean-cache.{service,timer} ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now avs-clean-cache.timer
systemctl --user list-timers avs-clean-cache.timer
loginctl enable-linger "$USER"     # ให้ทำงานแม้ไม่ล็อกอิน
```

ทดสอบทันทีด้วย `systemctl --user start avs-clean-cache.service`
(ถ้า cache ไม่เกิน 20 GB จะไม่ลบอะไร แล้วจบแบบ success)

ทางเลือกถ้าไม่ใช้ systemd — ใส่ cron รายสัปดาห์:

```cron
0 4 * * 0 /run/media/seal/shared/project/android-voice-synth/scripts/clean-cache.sh --auto --threshold 20G --apply --quiet
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
ให้เขียนไฟล์นั้นด้วยวิธีอื่นแทน


## 8. บันทึกการล้างรอบแรก

| ครั้ง | คำสั่ง | คืนพื้นที่ | พื้นที่ว่างหลังล้าง |
|---|---|---|---|
| 1 | `clean-cache.sh --all --apply` | 58 GB | 4.6 GB → 63 GB |
| 2 | build ใหม่ทั้ง workspace (วัดผล profile ใหม่) | — | target กลับมาแค่ 13 GB |
| 3 | `clean-cache.sh --poc --apply` | 7.6 GB | 51 GB → 59 GB |

หลังรอบที่ 3 `test/poc` เหลือ 6.5 GB ซึ่งเป็น **dataset และโมเดลที่ตั้งใจเก็บไว้**
(`train-*.jsonl` / `*.tsbin` / `*.mpk` — ไม่อยู่ใน git จึงกู้คืนไม่ได้)

สิ่งที่รอบที่ 3 ลบไปเป็น output ที่สร้างซ้ำได้: `renders/`, `student-*` 9 ชุด,
`*teacher-wavs/` — สร้างใหม่ด้วย `test/poc/eval_pipeline.sh`
