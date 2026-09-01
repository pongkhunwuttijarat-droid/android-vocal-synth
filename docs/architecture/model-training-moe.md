# Architecture — Universal-Phone Model Training (MoE per-Phoneme)

**วันที่:** 2026-08-09
**สถานะ:** Proposal (Draft) — รอ POC พิสูจน์

---

## Goal

Train neural voicebank models **บนมือถือทั่วไป** (universal phone, SD870-class เป็น minimum)
จาก **voicebank classic ที่มีอยู่แล้วใน ecosystem** (ไม่ต้องอัดเสียงใหม่) และแปลงกลับไปมาได้
แบบ 2-way:

```
┌─ Import door (ของคนอื่น) ─────────────────────────┐
│  DiffSinger .onnx ภายนอก → render ทุก alias       │
│  → classic bank (wav + oto.ini) = inference ล้วน   │
└───────────────────────────────────────────────────┘

┌─ Our loop (ของเรา, fix params) ──────────────────┐
│  classic bank ⇄ neural model (MoE per-phoneme)    │
│  A→B: oto.ini + wav + WORLD → train (บนเครื่อง)    │
│  B→A: render ทุก alias → analytic oto → classic    │
└───────────────────────────────────────────────────┘
```

## Design Decisions

### 1. Data factory — Distill จาก engine ตัวเอง (ไม่ใช่ raw syllables)

- **ไม่ต้อง label, ไม่ต้อง auto-oto**: alignment มาจาก timing ที่เราควบคุมเองตอน render
- **วิธี (teacher-student distillation):** classic bank (wav + oto) → engine **R2 (v2)** render
  phrase ที่เราเลือก (CV combos + phrase จริง) → **dump features จากภายใน R2 pipeline**
  (phonemes/timing/score-f0/mel — ไม่ re-analyze wav = ไม่ loss รอบสอง) → training set
- **Teacher = R2 เพราะ**: mel ของ R2 ถูก verify แล้ว (golden: mel powers identical กับ C#),
  R2 เป็น path ที่ refine กว่า (f0 err 150→32Hz — user ยืนยัน v2 default), และ contract ตรงเป๊ะ
  (student แทนที่ mel.onnx 85KB → vocoder 14M shared เดิม)
- **Student = score-based แท้**: หลัง train เสร็จ input = phonemes/timing/f0 จาก score
  → **ไม่ต้องโหลด source wav ตอน render** (เหนือกว่า R2 ที่ยังต้อง WORLD analysis จาก samples)
- **แก้ data ceiling**: พยางค์เดี่ยว → connected singing (engine ต่อ crossfade/overlap ให้แล้ว)
  → neural model เรียน "ร้องต่อเนื่อง" ไม่ใช่ "พยางค์ต่อกัน"
- **Self-consistent**: neural เรียนพฤติกรรม engine → B→A round-trip ตรงกัน
- **A→B**: alignment มาจาก timing ของ render (แม่น 100%) — oto.ini ของต้นทางใช้เป็น
  แหล่ง VOICE (เลือก sample) ไม่ใช่แหล่ง label
- **B→A**: เราสังเคราะห์เอง → รู้ timing phoneme จาก render internals → oto.ini คำนวณเชิงวิเคราะห์
- **Gap-filling (reverse) — เติม phoneme ที่ขาด** (2026-08-09): phonemizer tables
  (canonical alias set — มีใน crates/phonemizer) เทียบ oto.ini → รายการ alias ที่ขาด
  → MoE (base รู้จัก phoneme จาก bank อื่น + adapter ให้เสียงของ bank นี้) สังเคราะห์เติม
  → bank สมบูรณ์ (neural + classic export ครบ) = **bank completion service บนเครื่อง**
- **⚡ กฎเหล็ก additive-only**: เติมเฉพาะ alias ที่ขาด (wav ใหม่ + oto append) —
  **ห้ามแตะ original** (sample/oto/data เดิม): original = ground truth ของเสียง, สังเคราะห์
  ≤ original เสมอ, และสังเคราะห์**ไม่เข้า training set** (กัน self-training collapse)
- **⚠️ self-training collapse guard**: เติมเฉพาะ gap ที่ base มั่นใจ (กลุ่ม data หนาแน่น),
  quality filter หลังสังเคราะห์, **ไม่วน self-train ซ้ำ** (เติมครั้งเดียวแล้วหยุด)
- → วงจร conversion **zero audio detection** — auto-oto จำเป็นแค่ feature "อัดเสียงใหม่" (แยกต่างหาก)
- License: ใช้เอง ไม่แจกจ่าย (ไม่เป็น hub) → ความเสี่ยงต่ำ; ยังเก็บ per-bank check ใน data factory
- Data quality filter ที่ต้องมี: oto sanity (offset/consonant), normalize สภาพอัดเสียง (mic/room ต่างกัน)

### 2. MoE — General + Phoneme-group experts + Per-bank adapter

- **Routing ไม่ต้องเรียนรู้**: phoneme รู้อยู่แล้วจาก feed → group label จาก phoneme table → hard routing
  → ไม่มี router instability / load-balancing loss ของ learned MoE
- **3 ชั้น (DeepSeek-style shared-expert pattern):**

```
General expert (~5-8M)             ← always-on: ฟิสิกส์การร้อง (f0/dyn → base mel), ไม่ผูก phoneme/ภาษา
Phoneme-group experts (~3-6M × 8)  ← routed by phoneme→group (deterministic):
  vowel plosive fricative affricate nasal liquid glide special
Per-bank adapter (~1-3M LoRA)      ← THE VOICE: เอกลักษณ์เสียงของ bank นั้น — train บนเครื่อง (นาที-ชั่วโมง)
Vocoder 14M (shared, fixed)        ← mel → wav (voice-agnostic)
```

- **เหตุผลที่เลือก phoneme group (ไม่ใช่ per-phoneme / ไม่ใช่ per-language):**
  1. **Expert ข้ามภาษาได้** — กลุ่ม phoneme เป็นสากล (ทุกภาษามี V/P/F/N) → ภาษาเป็นแค่แหล่ง data
     expert "plosive" ฝึกจาก JP+EN+ZH banks รวมกัน = data ต่อ expert เยอะสุด
  2. Coarticulation จัดการได้ — expert เห็น phoneme sequence + phoneme-id embedding
     → เรียน coarticulation ระดับกลุ่ม (ไม่กลับไปปัญหา per-phoneme)
  3. phoneme ในกลุ่มมีพฤติกรรมเสียงคล้ายกัน (burst/closure, noise spectrum) → share params สมเหตุสมผล
  4. จำนวน expert ลงตัว: ~8 กลุ่ม (ไม่ใช่ 50, ไม่ใช่ 2-4 ภาษา)
- **Active ตอน render** = general + กลุ่มที่ปรากฏใน chunk (3-5 กลุ่ม) + adapter + vocoder
  ≈ 30-50M (~120-200 MB fp16)
- **Expert switching ที่ขอบ phoneme** (a→k) ต้อง handoff เนียน → ใช้ overlap machinery
  จาก chunking (มีอยู่แล้ว)
- **Train บน universal phone = adapter (1-3M)** — ชั้นที่ train บนเครื่องคือชั้นที่เล็กสุด
  → สัญญา "train บนเครื่องทุกเครื่อง" ยัง intact; group/general = desktop/ข้ามคืน

### 2b. Data source reality + base improvement (แก้ไข 2026-08-09)

- **ข้อเท็จจริง: voice data = จาก bank ที่มีอยู่แล้วเท่านั้น** — หาเสียงใหม่/อัดเสียงเพิ่มไม่ realistic
  → ไม่มี per-user continuous learning (ไม่มี signal ใหม่)
- **"เก่งขึ้น" = base ดีขึ้นเมื่อเพิ่ม bank เข้า pretrain (desktop)**: 50-100 bank × 5-15 นาที
  = data เป็นชั่วโมง → general/group experts ดีขึ้นเรื่อยๆ ตาม bank ที่เพิ่ม (หลายภาษา/หลายเสียง)
- **train บนเครื่อง = per-bank adapter one-shot** (import bank → train adapter 1-3M → เสร็จ)
  — ฟีเจอร์ยังอยู่ แต่ไม่ใช่ continuous
- user feedback (preference optimization) = อนาคต (research-heavy)
- **Performance metrics ที่ POC ต้องวัด**: (1) MoE render speed บน mid phone vs r2,
  (2) adapter train time ต่อ bank (import→train→ใช้ได้ภายในกี่นาที),
  (3) base quality vs จำนวน bank ที่ pretrain (scaling curve)

### 3. Chunking reuse — จาก render pipeline มาสู่ training

- `crates/runtime` มี `Chunker<T>`/`Scheduler<T>` + overlap + mixer stitching อยู่แล้ว
- Chunk ความยาวคงที่ (target ~512 frames) → tensor shape คงที่ → export/contract ง่าย
- **ลดขนาดโมเดลจริง**: DiffSinger มีตาราง `[1, 5000, 256]` = 1.28M params (เพลงยาว 5000 frames);
  chunk ≤ 512 frames → ตารางเล็กกว่า ~10 เท่า
- ลด "แกว่ง": overlap/stitching เดิมกันรอยต่อเพี้ยน (synthesis), shape คงที่ = batch สม่ำเสมอ (training)

### 4. Fix params contract (โมเดลที่เราผลิตเอง)

| รายการ | ค่าที่ fix |
|---|---|
| WORLD extraction | hop 512 / sr 44100 / mel bins 128 |
| Vocoder | NSF-HiFiGAN 14M (shared, fixed) |
| General expert | ~5-8M (regression, ไม่ใช่ diffusion ตอนเริ่ม) |
| Phoneme-group experts | ~3-6M × ~8 กลุ่ม (V/P/F/A/N/L/G/S) |
| Per-bank adapter | ~1-3M LoRA |
| Tensor contract | ชื่อ/shape ตายตัว — ตรงกับที่ Rust อ่าน |
| Export | ไม่ต้อง — burn format ใช้กับ moe-renderer เอง; โลกภายนอกผ่าน classic bank (wav+oto) |

→ เพราะเราผลิตเอง + fix หมด → ไม่มีทาง mismatch contract — "ได้ .onnx = จบ" เป็นจริง 100% สำหรับโมเดลเรา

### 5. Renderer integration — ใช้กับ renderer อื่นได้ (ในโปรเจกต์)

- MoE model = **renderer plugin .so ตัวใหม่** (`plugin_abi.h`: id/version/abi, singer_types,
  expressions, needs_wav_samples/oto/frq, modes) + engine adapter ใน `Box<dyn Engine>`
  (ตระกูลเดียวกับ r1/r2 — `synth-cli/src/engine.rs`)
- **Feed pipeline เดิมป้อนให้ทุก renderer เหมือนกัน** (phoneme/f0/duration → semantic arrays)
  → core ไม่ต้องแก้ เพิ่ม .so ใหม่
- Capability declaration: ไม่ต้องใช้ wav samples (neural), ใช้ tokens/durations/f0

### 6. Universal phone spec (minimum target)

```
RAM 6-8 GB (app budget ~1 GB) · GPU Adreno 6xx / Mali-G7x (Vulkan 1.1+)
CPU 8-core mid (SD 7-series / Dimensity 7xxx) · Thermal sustained ~2-3W
```

- **Per-expert checkpoint + resume** — train ทีละ expert, ขัดข้องต่อได้ (survival บน hardware ทุกชั้น)
- **Memory cap ~500 MB peak** — บังคับด้วย chunk size + expert size
- **Thermal-aware scheduler** — pause/resume ตามอุณหภูมิ, ต้องเสียบชาร์จ, background job
- **CPU fallback** — GPU ไม่มี/ช้า → train ได้ (ช้าลง) ไม่มี single point of failure
- UX promise จริง: SD870-class = "เสียบชาร์จทิ้งข้ามคืน", 8s Gen 4 = "รอ 30-60 นาที"

## Model size reality (วัดจากไฟล์จริง — 2026-08-09)

| แหล่ง | acoustic | vocoder |
|---|---|---|
| repo เรา (kiritan, OpenUtau export) | 271.6 MB / **67.85M** | 56.6 MB / **14.13M** |
| LogiAI10/diffsinger-mobile-onnx (HF) | 111.5 MB / **~28M** | 60.9 MB / ~15M |
| DiffSinger ทั่วตลาด (HF checkpoints) | ~50-90M (ckpt 392-400 MB) | 14M คงที่ |

→ DiffSinger จริง = acoustic 28-68M + vocoder 14M (≈ 42-82M รวม) — **ไม่ใช่ 500M**
→ MoE เรา: ~40-70M ทั้งระบบ (general + 8 กลุ่ม + adapter + vocoder) แต่ active แค่
  general + 3-5 กลุ่ม + vocoder ≈ 30-50M และ train บนเครื่องแค่ adapter 1-3M

## 2-way loop & compatibility

- **MoE format = เฉพาะเรา** — renderer ภายนอก (OpenUtau desktop, UTAU) อ่านไม่รู้เรื่อง
- **แต่ B→A ให้ classic bank (wav + oto.ini) = universal** — renderer ไหนก็กินได้
  → 2-way loop คือสะพาน compat ของทั้งระบบ
- Import door (standard DiffSinger → classic) ยังเหมือนเดิม — ไม่แตะ

## POC path (ลำดับ)

1. **Data extractor**: teto-english (558 wav + oto.ini ใน repo แล้ว) → training set
   (alias/offset + WORLD f0/mel) + quality filter — พิสูจน์ data pipeline บน Linux
2. **MoE POC (กลุ่ม V/P/F/N)**: general expert + 4 group experts train จาก teto-english บน Linux
   → render เทียบ baseline single model → วัด coarticulation (k+a/i/u) + คุณภาพ
3. **Adapter POC**: per-bank LoRA (1-3M) train จาก bank เดียว → พิสูจน์ "train บนเครื่องได้" (Linux ก่อน)
4. **Renderer plugin**: moe-renderer .so + engine adapter → ร้องผ่าน feed เดิม
5. **Universal-phone training**: adapter training บน device (incremental + thermal-aware + resume)

## Open questions

- [x] ~~Coarticulation: context embedding พอหรือต้อง expert ต่อ context class?~~ → **group expert เห็น sequence + phoneme-id embedding — แก้โดย design** (ยังต้อง validate ใน POC)
- [ ] Phoneme→group mapping table: 8 กลุ่ม (V/P/F/A/N/L/G/S) — ครอบคลุม JP/EN/ZH tables
- [ ] Data quality filter thresholds (oto sanity, mic/room normalize)
- [ ] fp16 training (burn issue #597) หรือ CPU fp32 ก่อน — ตัวเลขวัดจริงตัดสิน
- [ ] Pretrain ลำดับ: general (หลาย bank/หลายภาษา, desktop) → group experts → adapter
- [ ] Adapter injection: LoRA ใส่ชั้นไหน (general? group?) — วัดใน POC
- [ ] Expert switching ที่ขอบ phoneme: overlap/handoff ต้องเนียนแค่ไหน — วัดใน POC

## ดูเพิ่ม

- `docs/architecture/renderer.md` — renderer plugin interface
- `docs/architecture/runtime-engine-plugins.md` — engine plugin architecture
- `.hermes/plans/2026-08-09_moe-universal-training.md` — implementation plan

## Change History

- 2026-08-09: สร้างเอกสาร proposal — MoE per-phoneme + universal-phone training + 2-way conversion. Version 0.1.0.
