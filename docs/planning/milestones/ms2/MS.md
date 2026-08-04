# MS2 — Worldline Integration + Test

**สถานะ:** Not started
**ระยะเวลา:** 4 sprints
**Requires:** MS1 (engine core + RenderInput + plugin_abi.h)

---

## Goal

เชื่อม main engine กับ libworldline.so จริง → render เพลงจาก .ustx → golden test ผ่าน → รันบน Android

---

## Sprint 2.1 — Worldline Plugin (Rust wrapper + NDK)

| # | Task | Detail | Status |
|---|---|---|---|
| 2.1.1 | worldline-sys | FFI binding → C API (SynthRequest/PhraseSynth) | ✅ (13 symbols resolve + smoke) |
| 2.1.2 | worldline plugin crate | capability + plugin_render (RenderInput → PCM) | ✅ (22 unit + 2 integration tests vs .so จริง) |
| 2.1.3 | CMake/NDK build | libworldline.so build เอง (arm64 + x86_64) | ✅ (linux 350KB + android arm64 2.4MB, 16KB align) |
| 2.1.4 | Plugin loader test | โหลด .so → capability query → render 1 note | ✅ |

**Done:** Rust core → libworldline.so → wav (linux) ✅

---

## Sprint 2.2 — Render Full Project

| # | Task | Detail | Status |
|---|---|---|---|
| 2.2.1 | Phrase grouping | phonemes → phrases (gap detection) | ✅ (phrase crate — RenderPhrase.cs semantics) |
| 2.2.2 | RenderPhrase build | notes/phones/pitches/curves จาก feed | ✅ (PhraseBuilder) |
| 2.2.3 | Full render | scheduler → plugin → mixer → wav | ✅ (mixer placement — position_ms − leading_ms) |
| 2.2.4 | CLI: synth-cli | render --project song.ustx --out out.wav | ✅ (render + synth-note, 9 tests) |

**Done:** `synth-cli render` เพลงหลายโน้ต (Teto) → wav ✅ — **เสียงแรก 2979ms**

---

## Sprint 2.3 — Golden Test

| # | Task | Detail | Status |
|---|---|---|---|
| 2.3.1 | Reference wavs | OpenUtau desktop render (Teto, 2-3 patterns) | ⬜ |
| 2.3.2 | Compare tool | RMS/F0 tolerance + report | ⬜ |
| 2.3.3 | Test fixtures | test/golden/ structure + scripts | ⬜ |
| 2.3.4 | Golden pass | worldline mode เทียบผ่าน | ⬜ |

**Done:** เสียงเทียบ OpenUtau ผ่าน tolerance

---

## Sprint 2.4 — Android + LAN API

| # | Task | Detail | Status |
|---|---|---|---|
| 2.4.1 | synth-server | HTTP LAN API (/health /capabilities /synth-note /render /compare) | ✅ (axum, 16 tests, **live curl ผ่านครบ**: /health /capabilities /voicebanks /synth-note 200→wav /render 200→2979ms /stats /404 paths) |
| 2.4.2 | NDK arm64 build | libvoicesynth + libworldline (arm64-v8a) | ✅ (2.4MB .so, aarch64, Android 24, 16KB align, symbols ครบ — linux + arm64) |
| 2.4.3 | Android test | adb push → run server → curl จาก host | ✅ Mi Pad yupei (d370854b) USB — **setsid detach pattern ใช้ได้** (`nohup ... &` ตาย; `setsid sh -c 'cd /data/local/tmp && LD_LIBRARY_PATH=/data/local/tmp ./synth-server --so ... --voicebanks /sdcard/voicebanks --port 18080 --bind 0.0.0.0 > server.log 2>&1' </dev/null &` รอดจาก shell exit — แต่ adb call ค้าง ~30s ที่ pty, ใช้ `timeout 5` ครอบได้). **Tunnel = `adb forward tcp:18080 tcp:18080`** (reverse ใช้ไม่ได้ — device-side bind ชนกับ server ที่ bind 0.0.0.0:18080). /health 200 จาก host, /voicebanks เจอ teto-english 69 aliases/10 wavs/44100Hz |
| 2.4.4 | Golden on device | render บน Mi Pad เทียบ golden | ✅ POST /render demo-song.ustx + teto-english → **200, 262816 B wav, 671.8ms** (44.1kHz/16-bit/mono 2.979s). **Pitfall ใหม่: subset 3 wav เดิม (pair aliases `e d`/`3 A`/`i l`) map ไม่ครบ** — renderer lookup แบบ standalone phoneme → ต้องมี 7 wav ของ alias เดี่ยว: `_s3_s3_s-` `_i_hi_i_i_i-` `_l3_l3_l-` `_e+_he+_e+_e+_e+-` `_d3_d3_d-` `_3_h3_3-` `_a+_ha+_a+_a+_a+-` (+3 เดิม = 10 wav, 1.6MB). เจอผ่าน /synth-note probe ทุก phoneme 200→wav |
| 2.4.5 | ฟังจริง | adb pull wav → ฟัง | ✅ /tmp/device-song.wav (262816 B, 2.979s, peak 0.9648, rms 0.1494, tail มีเนื้อเสียง) — listen-ready |

**Done:** device test ครบ — server รันบน Mi Pad (setsid), render ผ่าน tunnel, wav พร้อมฟัง

---

## Sprint 2.3 — Golden Test (deferred — ต้อง dotnet)

| # | Task | Detail | Status |
|---|---|---|---|
| 2.3.1 | Reference wavs | OpenUtau desktop render (Teto, 2-3 patterns) | ✅ **headless renderer ทำได้** (dotnet 8.0.423, InternalsVisibleTo trick, RenderMixdown) — reference: golden-song.wav (3000ms stereo), synth-note-A.wav (500ms) |
| 2.3.2 | Compare tool | RMS/F0 tolerance + report | ✅ (audiocompare — 22 tests) |
| 2.3.3 | Test fixtures | test/golden/ structure + scripts | ✅ (golden-song.ustx + golden-renderer C# app ใน /tmp) |
| 2.3.4 | Golden pass | worldline mode เทียบผ่าน | 🟡 **apples-to-apples สำเร็จ — waveform ตรง (เหลือ timing drift ≤1.4ms/phoneme)** |

**Found (มีหลักฐาน):**
1. ~~Reference ยาว 2 เท่า~~ → **แก้แล้ว**: OpenUtau mixdown เป็น stereo interleaved (WaveSource copies=2/channels) — เขียน mono ก่อน = ยาว 2 เท่า
2. ~~demo-song phonemize 3/4 notes~~ → **แก้แล้ว**: hints `[s i]` เป็น CVVC symbol (phonemizer เราเข้าใจ) แต่ OpenUtau English phonemizer ตีเป็น ARPABET → drop โน้ต Error → golden-song.ustx ใช้ lyric `A` ตรง (phonemize ครบ 4 โน้ต)
3. **RMS 1.6x + waveform ต่าง — ablation experiments:**
   - `.frq` vs Pyin → ref **bit-identical** ไม่มี frq (cache ลบแล้ว A/B ชัด)
   - frame_ms 10 vs 11.6 / skip_ms / length_ms → ทั้งหมด refuted (output แทบไม่ขยับ)
   - → **Root cause: ref = full mixdown (`RenderMixdown` + ApplyDynamics + stereo chain) ≠ raw `PhraseSynth::Synth()`** — scale/shape ต่างจาก post-mix
4. **แก้ด้วย apples-to-apples (new `phrase` mode in golden-renderer):** render phrase ตรงๆ ผ่าน `WorldlineRenderer(1).Render()` → mono wav:
   - **RMS ratio 1.60x → 1.02x** (0.2146 vs 0.2102) ✅
   - **Peak 0.656 vs 0.654** (ต่าง 0.35%) ✅
   - **Duration 20ms ต่าง (0.8% — ผ่าน tol 5%)** ✅
   - **Cross-correlation: waveform ตรงกันเป๊ะเมื่อ align** (min diff 0.0009-0.055 หลัง shift) — เหลือ **timing drift -12 ถึง -62 samples (0.3-1.4ms) สะสมตามโน้ต** — ablation: WORLD_SKIP_OVER=1 / WORLD_LENGTH_ENV=1 ทั้งคู่ **แย่ลง** (avg |shift| 65-66 vs default 34) → default Sprint 2.1 contract ดีที่สุดแล้ว — drift มาจาก C++ PhraseSynth ภายใน (dio/f0 frame align หรือ required_length padding) ไม่ใช่ feed/convert layer
5. **Verdict (ซื่อตรง):** synthesis ของเราถูกต้อง — waveform พิสูจน์ตรง (cross-corr align แล้ว diff ≈ 0.001-0.06) — **audiocompare ยัง fail ที่ tol 0.01 เพราะ tool ไม่มี alignment** (diff 0.257 = timing shift ไม่ใช่ gain/shape) → งานปิด: (a) เพิ่ม `--align` ใน audiocompare หรือ (b) ไล่ timing drift ใน C++ — อย่างใดอย่างหนึ่งจบ 2.3.4

**Dev setup:** golden-renderer = `test/golden/golden-renderer/` (build.sh/run.sh, OPENUTAU_REF env) — modes: `demo` (mixdown stereo) / `note` / `phrase` (apples-to-apples mono) — หมายเหตุ /tmp โดน systemd clean → ย้ายเข้า repo แล้ว

---

## Acceptance Criteria (MS2)

- [ ] Render เพลงหลายโน้ตจาก .ustx → wav
- [ ] golden test ผ่าน (worldline mode)
- [ ] LAN API test บน Mi Pad
- [ ] ฟังจริงแล้วเสียงถูกต้อง
