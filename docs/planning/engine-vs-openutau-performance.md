# Engine vs OpenUtau — Performance Comparison

> วัดเมื่อ **2026-09-29** · AMD Ryzen 5 8600G (6C/12T, `nproc` = 12) · 30 GB RAM · Linux
> Rust `synth-cli` release (84 MB) · .NET **8.0.425** (`~/dotnet`) · OpenUtau ref = `ref-openutau-clean/`
> harness: [`scripts/bench-vs-openutau.py`](../../scripts/bench-vs-openutau.py) · ผลดิบ: `out/bench/results.json`
> voicebank: `test/golden/teto-english/library` (558 wav + 558 `.frq` ครบ)

---

## 1. คำถามที่ตอบ

> "สมมติฐานที่ว่า engine เราจะเร็วกว่า OpenUtau เพราะเราแบ่งงาน parallel เยอะ — จริงไหม?"

**จริง — แต่กลไกไม่ใช่ "เราทำงานน้อยลง" และไม่จริงเสมอ**

- งานรวม **ต่อหนึ่งวินาทีของ audio** พอ ๆ กัน (badapple-trunc: เรา 132.5 CPU-ms เทียบ OpenUtau 125.4 → ต่าง 6%)
- ที่ต่างคือ **จำนวน core ที่ใช้จริง**: เรา 10.4–10.7 cores, OpenUtau ~2
- ⇒ กำไร wall-clock เกือบทั้งหมดมาจาก parallel ล้วน ๆ
- **ข้อจำกัด**: เพดานไม่ใช่จำนวน core แต่คือ **จำนวน chunk** — โปรเจกต์สั้นอิ่มตัวที่ 4 threads แล้ว

---

## 2. วิธีวัด

| ประเด็น | วิธี |
|---|---|
| เทียบกับอะไร | `test/golden/golden-renderer` — OpenUtau.Core ที่ compile จริง (`OpenUtau.Core.dll`) โหมด `phrase` เรียก `renderer.Render()` ทีละ phrase = path เดียวกับ `RenderEngine.RenderRequests` ของแอปจริง |
| kernel | **ทั้งสองฝั่ง link `libworldline.so` ไฟล์เดียวกัน** (md5 `fa0cd7d33c1330d1551e330637e11413`) → คอลัมน์ `r1` เทียบ "ค่าใช้จ่ายของ host" ได้สะอาด |
| project/voicebank | `.ustx` เดียวกัน + bank เดียวกันทุกคู่ |
| cold | ล้าง cache ทุกรอบ: `/tmp/golden-renderer-{cache,data}` (OpenUtau, `XDG_CACHE_HOME`) และ `test/golden/teto-english/cache/cache/res-*.bin` (เรา, `runtime::RenderCache`) |
| warm | รันซ้ำทันทีหลัง cold ใน process เดียวกัน |
| ตัวเลข | median ของ 3 รอบ · wall clock จาก parent · CPU จาก `wait4(ru_utime+ru_stime)` · peak RSS จาก sampling `/proc/<pid>/status:VmHWM` ทุก 10 ms |
| single core | `taskset -c 0` ทั้งสองฝั่ง + `RAYON_NUM_THREADS=1` ฝั่งเรา |

**กับดักที่ต้องรู้**: `/tmp` ในสภาพแวดล้อมนี้เป็น tmpfs **แยกต่อการเรียกคำสั่งหนึ่งครั้ง** → cache ของ OpenUtau หายข้ามคำสั่ง cold/warm จึงต้องวัดใน process เดียว (badapple cold 29.4 s → warm 1.0 s คือหลักฐาน)

**สิ่งที่วัดไม่ครอบคลุม**: UI/playback/export mixdown ของแอป OpenUtau เต็มรูปแบบ (วัดเฉพาะ render path) และไม่ได้วัด wasm/Android — ดู [wasm-performance.md](../architecture/wasm-performance.md) สำหรับ native-vs-wasm

---

## 3. ผลหลัก (cold, median ของ 3 รอบ)

| project | audio OU | audio เรา | OpenUtau | **r1 (เรา)** | r1-rs | เร็วขึ้น |
|---|---|---|---|---|---|---|
| one-note | 1.64 s | 1.50 s | 0.424 s | **0.091 s** | 0.103 s | **4.7×** |
| sakura-test | 3.29 s | 3.50 s | 0.684 s | **0.291 s** | 0.303 s | **2.4×** |
| scale-demo-app | 14.57 s | 14.51 s | 1.642 s | **1.249 s** | 1.318 s | **1.3×** |
| badapple-trunc ⚠️ | 308.46 s | 401.74 s | 19.556 s | **5.128 s** | 5.507 s | **3.8×** |
| badapple ⚠️ | 401.62 s | 504.35 s | 30.691 s | **6.592 s** | 7.173 s | **4.7×** |
| r2 (neural) | — | 14.52 s | — | 3.517 s | — | (คนละเส้นทาง synthesis) |

⚠️ สองแถวล่าง **ไม่ like-for-like** — OpenUtau render ไม่ครบโปรเจกต์ ดู §6

### 3.1 Normalize ต่อวินาทีของ audio (ยุติธรรมกว่า เพราะสองฝั่ง render ไม่เท่ากัน)

| project | OpenUtau ms/audio-s | r1 ms/audio-s | ratio | OpenUtau CPU-ms/audio-s | r1 CPU-ms/audio-s |
|---|---|---|---|---|---|
| one-note | 258.5 | 60.7 | 4.3× | 366 | 60 |
| sakura-test | 207.9 | 83.1 | 2.5× | 337 | 120 |
| scale-demo-app | 112.7 | 86.1 | 1.3× | **208** | **281** |
| badapple-trunc | 63.4 | **12.8** | **5.0×** | **125.4** | **132.5** |
| badapple | 76.4 | **13.1** | **5.9×** | **151.4** | **138.1** |

**อ่านตารางนี้ให้ดี** — สองคอลัมน์ขวาคือหัวใจของคำตอบ:
บนโปรเจกต์ยาว (badapple/badapple-trunc) **CPU ต่อหน่วยงานเท่ากัน** (125 vs 133 และ 151 vs 138) แต่ wall เร็วกว่า 5–5.9× ⇒ กำไรมาจาก parallel ล้วน
บนโปรเจกต์สั้น (scale-demo) เราใช้ CPU **มากกว่า 35%** แต่ wall เร็วกว่าแค่ 1.3× ⇒ parallel หักกลบกับ overhead

### 3.2 Single core (`taskset -c 0`, cold) — ต่อ 1 core ใครเร็วกว่า

| project | OpenUtau | r1 1-thread | r1-rs 1-thread |
|---|---|---|---|
| scale-demo-app | **200.0** ms/audio-s | 276.2 | 317.4 |
| badapple-trunc | 111.2 ms/audio-s | **83.0** | 89.2 |

⇒ **ไม่มีความเสียเปรียบต่อ core แบบคงที่** — ขึ้นกับโปรเจกต์:
- โปรเจกต์สั้น: OpenUtau เร็วกว่า **1.38×** ต่อ core (fixed cost + chunk overlap ของเรากินสัดส่วนสูง)
- โปรเจกต์ยาว: **เราเร็วกว่า 1.34×** ต่อ core

### 3.3 Thread scaling (r1, cold)

| threads | scale-demo | speedup | badapple | speedup |
|---|---|---|---|---|
| 1 | 4.024 s | 1.00× | 43.325 s | 1.00× |
| 2 | 2.081 s | 1.93× | — | — |
| 4 | 1.254 s | **3.21×** | — | — |
| 12 | 1.270 s | 3.17× (ไม่ดีขึ้น) | **6.478 s** | **6.69×** |

⇒ **เพดานคือจำนวน chunk ไม่ใช่จำนวน core**: scale-demo มี 4 chunks จึงอิ่มตัวที่ 4 threads ขอ 12 ได้ 0 การปรับปรุง
⇒ badapple มี chunk เยอะพอ จึงไปได้ถึง ~10.7 cores

---

## 4. Cache ครองทุกอย่าง (สำหรับงาน interactive)

| project | เรา cold → warm | OpenUtau cold → warm |
|---|---|---|
| one-note | 0.091 → **0.010 s** (9×) | 0.424 → 0.346 s |
| sakura-test | 0.291 → **0.013 s** (22×) | 0.684 → 0.357 s |
| scale-demo-app | 1.249 → **0.019 s** (66×) | 1.642 → 0.366 s (4×) |
| badapple | 6.592 → **0.223 s** (30×) | 30.691 → 0.986 s |

⇒ งาน edit-play loop อยู่ที่ **0.01–0.23 s** (cache hit) ไม่ใช่ที่ความเร็วดิบ
⇒ **cache key ถูกต้องสำคัญกว่าจำนวน core** — ตรงกับ [BACKLOG #7](BACKLOG.md) (cache salt ไม่ผูกกับโมเดล)

---

## 5. Memory (peak RSS) — จุดที่ parallel มีราคา

| project | OpenUtau | r1 (default 12 threads) | r1 @1 thread |
|---|---|---|---|
| one-note | 94.2 MB | **27.9 MB** | — |
| sakura-test | 108.2 MB | **49.9 MB** | — |
| scale-demo-app | 195.2 MB | **168.1 MB** | 72.3 MB |
| badapple | 749.7 MB | **644.7 MB** | 409.8 MB |
| badapple-trunc | 484.7 MB | 570.8 MB ⚠️ | 353.1 MB |

⚠️ badapple-trunc ตัวเลขดิบทำให้เราดูแย่กว่า แต่ OpenUtau render แค่ 308 s จาก 401 s — คิดต่อ audio-1s: เรา 1.42 MB/s เทียบ OpenUtau 1.57 MB/s ⇒ เราต่ำกว่าเล็กน้อย

**ข้อควรระวังสำหรับ Android**: peak RSS **โตตามจำนวน thread** (badapple: 410 MB → 685 MB เมื่อ 1 → 12 threads) เพราะ chunk buffer ถูกถือพร้อมกัน
บนมือถือควรคุมเพดาน thread จาก RAM ไม่ใช่จาก `nproc`

---

## 6. บั๊กที่เจอใน golden harness (ควรแก้ ไม่ใช่ปัญหาของ engine)

`golden-renderer phrase` โหมด classic **render ไม่ครบโปรเจกต์ขนาดใหญ่**:

| fixture | span จริง | OpenUtau สร้างได้ | หายไป |
|---|---|---|---|
| badapple.ustx | 504.35 s | 401.62 s (123 phrases) | **102.7 s / 203 โน้ต** |
| badapple-trunc.ustx | 401.74 s | 308.46 s (97 phrases) | **93.3 s** |

หลักฐานว่าเป็นบั๊กของ harness ไม่ใช่ความต่างเชิง fidelity:
- โน้ตที่ตกไปมี lyric ชุดเดียวกับที่ render ผ่านก่อนหน้าใน part เดียวกัน (`na`, `no`, `ko`, `ra`, `su`, …) — ไม่ใช่ alias ที่ resolve ไม่ได้
- output ของเรา = span ของโน้ตพอดี (504.35 s, คลาด 0.00) และ tail มีสัญญาณจริง (peak 1.0) ไม่ใช่ silence
- ภายใต้ `taskset -c 0` บน badapple ล้ม **14 จาก 15 รอบ** ด้วย `phrases: 0` (rc=2)
- บน badapple-trunc ได้ 97 phrases **เหมือนกันทุกรอบ** ⇒ partial แต่ deterministic

**ต้นเหตุ** — `test/golden/golden-renderer/Program.cs`:
รอ `part0.PhonemesUpToDate` แล้วเรียก `part0.GetRenderRequest()` แต่ `renderPhrases` ถูกสร้างตอน `ValidateFull` ซึ่ง **re-trigger async phonemizer** อีกครั้ง ⇒ ได้ list ที่บางส่วน
fallback ที่สร้าง phrase ตรง ๆ (`RenderPhrase.FromPart(...)`) มีอยู่แล้วแต่ถูกกั้นด้วย `if (diffSingerMode)` (บรรทัด ~208) จึงไม่ครอบโหมด classic

**ผลกระทบ**: การเทียบ golden fidelity บนโปรเจกต์ใหญ่ที่ผ่านมา เทียบกับงานเพียง ~60–77% ของโปรเจกต์ — ควรตรวจย้อนหลัง

**แนวทางแก้ (ยังไม่ได้ทำ)**: ขยาย fallback นั้นให้ครอบ classic หรือ assert ว่า phrase list ครอบ span ของ part แล้ว fail ดัง ๆ แทนที่จะ render เงียบ ๆ ไม่ครบ

> benchmark นี้ **ไม่แก้** `Program.cs` (เป็น reference harness ของงาน fidelity) แต่ `scripts/bench-vs-openutau.py` ตั้งใจทนทาน: รอบที่ `rc≠0` ถูก retry และไม่ถูกนำเข้า median และรายงาน `n_ok`/`n_failed` กำกับเสมอ

---

## 7. ข้อจำกัดของการวัดนี้

1. **ไม่ได้วัด OpenUtau เต็มแอป** — เฉพาะ render path (ไม่มี UI/playback/export mixdown) ซึ่งเป็นส่วนที่กำไรหรือขาดทุนด้าน performance อยู่จริง
2. **สองฝั่ง render ไม่เท่ากันทุก fixture** — เพราะบั๊ก §6 จึงต้องอ่านคู่กับคอลัมน์ audio ของตัวเอง (ตาราง §3.1)
3. segmentation ต่างกันเล็กน้อย (sakura: OpenUtau 3 phrases, เรา 2 phrases) — คนละ phonemizer default
4. **single-core ของ OpenUtau ไม่ได้แปลว่า 1 เธรดบริสุทธิ์** — pin แล้ว wall ยังช้ากว่า unpinned 1.75× (badapple-trunc 19.6 → 34.3 s) ⇒ ส่วนที่เกิน 1 core คือ **GC/JIT ของ .NET ที่ทำงานคู่ขนานได้** ไม่ใช่ render ขนาน (โค้ด `RenderEngine.RenderRequests` เป็น `foreach … task.Wait()` เรียงลำดับ) — จึงเป็น "ภาษีของ platform" ที่ multicore ช่วยซ่อน
5. วัดบน x86 desktop เท่านั้น — ยังไม่ยืนยันบน ARM/Android ที่เป็นเป้าหมายจริง
6. `r2` (neural) เป็นคนละเส้นทาง synthesis เทียบตรงกับ OpenUtau classic ไม่ได้ (cold 3.517 s, 12.19 CPU-s, 718 MB RSS บน scale-demo)

---

## 8. Allocation-reuse optimization (2026-09-29)

หลัง benchmark ข้างต้น ปรับ `world-rs` ให้ reuse scratch buffers ใน CheapTrick, `LinearSmoothing`, `DCCorrection` และ D4C helper calls โดยไม่เปลี่ยนสูตรเลข และย้ายการคำนวณ window sum ซ้ำใน CheapTrick ออก

A/B: `r1-rs`, release ปกติ (ไม่ใช้ `target-cpu=native`), 1 thread, cache เย็น, 3 รอบต่อ variant; baseline binary ถูกเก็บก่อนแก้, WAV output hash ตรวจซ้ำ:

| project | baseline | optimized | ลดเวลา | CPU baseline → optimized | WAV |
|---|---:|---:|---:|---:|---|
| scale-demo-app (14.51s) | 4.270s | **4.070s** | **4.7%** | 4.209 → 4.051s | SHA-256 ตรงกัน 3/3 |
| badapple-trunc (401.74s) | 36.083s | **35.257s** | **2.3%** | 35.690 → 35.092s | SHA-256 ตรงกัน 3/3 |

Unit tests `world-rs` 23/23 และ `worldline-rs` 16/16 ผ่าน; helper workspace tests ยืนยัน output แบบ exact equality เมื่อ reuse buffers ข้ามหลาย width/F0. Peak RSS ใน A/B ใกล้เคียงกัน (scale-demo 62.6→62.9 MB; badapple-trunc 332.2→333.5 MB) — รอบนี้ optimize allocation churn ไม่ได้ลด peak memory

**ข้อสรุป**: กำไรเล็กแต่ทำซ้ำได้และ output byte-identical; ยังไม่ใช่คำตอบทั้งหมดของ 5–9% C++/Rust gap.

ผลดิบ: [`out/bench/alloc-reuse-ab.json`](../../out/bench/alloc-reuse-ab.json)

### 8.1 D4C workspace / window-weight follow-up

Refactored D4C to reuse window, complex FFT, spectrum, sorted-spectrum, cumulative, centroid, group and coarse buffers. Also reuse per-sample window coefficients on the mean-removal pass, eliminating duplicate cosine evaluations while retaining identical coefficient values. A/B against the previous allocation-reuse binary, release, 1 thread, cold cache, 3 reps:

| change | fixture | before | after | speedup | WAV |
|---|---|---:|---:|---:|---|
| workspace buffers | scale-demo-app | 4.086 s | 4.312 s | 0.948× (5.2% slower) | byte-identical 3/3 |
| workspace buffers | badapple-trunc | 35.250 s | 35.351 s | 0.997× (within noise) | byte-identical 3/3 |
| cached window weights | scale-demo-app | 4.088 s | 4.104 s | 0.996× (within noise) | byte-identical 3/3 |
| cached window weights | badapple-trunc | 35.322 s | 34.451 s | **1.025× (2.5% faster)** | byte-identical 3/3 |

**Profiling result and follow-up**: Temporary `WORLD_D4C_TIME` instrumentation on release, single-thread, cold-cache renders showed coarse-band segment preparation + FFT + sort at **49–52%** of D4C time on both real fixtures; centroid was 17–19%, group smoothing 11–12%, LoveTrain about 6%. No finer substage percentages are claimed; the attempted second-level profiler build did not complete reliably.

This identified repeated segment fill and sorting as candidates. A `sort_unstable_by` experiment was not retained. The attempted WAV comparison used a malformed silent baseline, so it cannot establish whether unstable sort changed output; production conservatively keeps stable `sort_by`. The retained change clears only the unused FFT tail after the windowed prefix has been fully overwritten; a focused regression test proves that tail-fill input equals a fresh zeroed FFT buffer exactly.

A matched no-LTO A/B against a correct baseline (stable sort, full zero-fill), release, cold cache, 1 thread, 3 reps:

| fixture | baseline | tail-fill | speedup | output |
|---|---:|---:|---:|---|
| scale-demo-app | 4.082 s | 3.851 s | **1.060× (6.0% faster)** | SHA-256 identical 3/3; peak 0.66681 both |
| badapple-trunc | 34.498 s | 34.431 s | 1.002× (noise-level) | SHA-256 identical 3/3; peak 1.0 both |

The A/B used matched no-LTO release binaries because regular thin-LTO CLI linking repeatedly stalled. Therefore the result isolates the D4C code change under the same build profile, but the gain is workload-dependent; don’t assume a 6% gain for all projects. Full output hashes and medians: [`d4c-correct-nolto-ab.json`](../../out/bench/d4c-correct-nolto-ab.json).

Earlier D4C workspace/window-weight A/B remains: neutral/slightly slower workspace-only result on scale-demo, ~2.5% faster long fixture with cached window weights, WAV identical. Results: [`d4c-workspace-ab.json`](../../out/bench/d4c-workspace-ab.json), [`d4c-window-weight-ab.json`](../../out/bench/d4c-window-weight-ab.json).

---

## 9. สรุป

| คำถาม | คำตอบ |
|---|---|
| เราเร็วกว่า OpenUtau ไหม | **เร็วกว่า** บนทุก fixture — 4.7× (one-note), 2.4× (sakura), 1.3× (scale-demo), 3.8–4.7× (badapple-class) |
| เพราะ parallel จริงไหม | **จริง** — CPU ต่อหน่วยงานเท่ากัน (badapple-trunc 125 vs 133 CPU-ms/audio-s) แต่เราใช้ 10.4–10.7 cores เทียบ ~2 |
| parallel ได้เต็มที่เสมอไหม | **ไม่** — เพดานคือจำนวน chunk: scale-demo อิ่มตัวที่ 4 threads (3.21×), t12 ไม่ดีขึ้น |
| ต่อ 1 core เราเร็วกว่าไหม | **ไม่เสมอ** — สั้น: ช้ากว่า 1.38× · ยาว: เร็วกว่า 1.34× |
| อะไรสำคัญที่สุดสำหรับ interactive | **cache** — 0.01–0.23 s เทียบ 0.29–6.6 s cold |
| ราคาที่จ่าย | peak RSS โตตาม thread (410 → 685 MB, 1 → 12 threads) — สำคัญบนมือถือ |

**ข้อเสนอ**: (1) ตั้งเพดาน thread จาก RAM/อุณหภูมิบน Android ไม่ใช่ `nproc` (2) ลด chunk overlap ที่ทำให้งานซ้ำ — เป็นจุดที่เราช้ากว่า OpenUtau ต่อ core บนโปรเจกต์สั้น (3) แก้ §6 ก่อนใช้ golden harness เทียบโปรเจกต์ใหญ่ต่อ
