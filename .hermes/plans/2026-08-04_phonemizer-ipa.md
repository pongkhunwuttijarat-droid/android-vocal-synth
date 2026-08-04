# Phonemizer Multi-Layer (IPA) Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** แยก phonemizer ออกจาก voicebank — lyric → **detect ภาษา → IPA (universal)** → **voicebank phonemes ผ่าน manifest** — แทนที่ English/Japanese phonemizer ที่ map ตรงเข้า aliases ปัจจุบัน

**Architecture:** 3 layers — (L1) G2p/detect ให้ IPA tokens กลาง, (L2) IPA→bank adapter ใช้ manifest, (L3) voicebank manifest (phoneme set + type + pair structure) derive จาก oto.ini — เลียนแบบ OpenUtau (G2p → phonemizer ต่อ singer)

**Tech Stack:** Rust (crates/phonemizer), voicebank crate (oto), domain (UNote/UPhoneme)

---

## Current Context

- `crates/phonemizer/src/english.rs` (418L): `EnglishCvvcPhonemizer::process` — hint → tokens → `pair()` map ตรงเป็น CV/CVVC aliases (เช่น "la[l A]" → [l, A] → pairs "l A")
- `crates/phonemizer/src/japanese.rs` (345L): `JapaneseVcvPhonemizer` — VCV ตรง + translit fallback (`translit.rs` ja→en mora)
- `crates/phonemizer/src/g2p.rs` (195L): `G2p` trait (is_valid_symbol/is_vowel/is_glide/query/unpack_hint) + `G2pFallbacks`; `lilt_dict.rs` (84L): `lilt_demo_g2p()` demo dictionary
- `crates/phonemizer/src/phonemizer.rs`: `Phonemizer` trait — `process(notes, singer) -> Vec<UPhoneme>`
- `crates/voicebank/src/oto.rs`: `Oto` struct (alias/offset/consonant/cutoff/preutter/overlap) + `OtoSet`; `Voicebank::oto_map: HashMap<String, Oto>` (aliases ทั้งหมด — 2673 ตัวใน Teto English)
- **จุดอ่อนปัจจุบัน:** phonemizer แต่ละตัว map ตรงเข้า bank aliases — bank ใหม่/ภาษาต้องเขียน phonemizer ใหม่; ไม่มี "ภาษากลาง"

## Assumptions

- Hint (`word[ph]`) ยังเป็น authoritative (ไม่เปลี่ยน) — IPA layer ใช้กับ **ไม่มี hint**
- voicebank manifest derive จาก oto.ini (ไม่ต้องไฟล์ใหม่) — เหมือน OpenUtau
- เป้าแรก: Teto English (CVVC) + Teto Japanese (VCV) ผ่าน layer เดียวกัน

---

## Proposed Approach (ลำดับ L3 → L1 → L2)

### L3: Voicebank Manifest + Capability Manager (ทำก่อน — พิสูจน์ bank ประกาศตัวเองได้)
`Voicebank::phoneme_manifest()` — derive จาก `oto_map`:
- `phoneme_set: BTreeSet<String>` — alias tokens เดี่ยว (เช่น "3", "h3", "r3", "A" ...)
- `types: HashMap<String, PhonemeType>` — classify จาก oto: vowel (consonant==0 & มีสระใน wav), consonant, glide — ใช้ heuristics: alias ตัวเดียว + wav ชื่อ/oto consonant/offset
- `pairs: HashSet<(String,String)>` — alias 2-token (เช่น "l A") — CV/CVVC/VCV structure
- **Test:** Teto English → phoneme_set มี "A"/"l"/"r3" ไม่มี "a"/"r" (ข้อเท็จจริงที่รู้แล้ว); Teto Japanese → มี "ra" VCV

`CapabilityManager` — **รวม negotiation ฝั่ง voicebank + engine แต่ผ่าน abstraction (ไม่ผูก engine ใด)**:
- **`EngineCapabilities` trait** (นิยามใน `domain` หรือ crate กลาง — ไม่ใช่ worldline): `name()`, `modes()`, `needs_oto()`, `needs_frq()`, `expressions()`, `sample_rate()` — `WorldlineCapabilities` เป็นแค่ impl หนึ่งของ trait นี้ (มีอยู่แล้ว — แค่ implement trait)
- **`PhonemeManifest`** (voicebank side — engine-agnostic ล้วน): phoneme set/types/pairs
- API: `can_render(phoneme) -> bool` (จาก manifest), `missing(phonemes) -> Vec<String>`, `nearest(phoneme) -> Option<String>` (NearestPhoneme — ใช้ manifest เท่านั้น ไม่แตะ engine)
- **การแยก:** phonemizer layer (L1/L2) ใช้ **manifest อย่างเดียว** (pure phoneme domain — ไม่รู้จัก engine); engine capability gate (`needs_oto`/`needs_frq`/modes) ตรวจที่ **render time โดย host** (Engine trait — ที่ทำไว้แล้ว) — CapabilityManager แค่รวม 2 แหล่งให้ host เรียกสะดวก

### L1: IPA Layer (universal)
เพิ่ม IPA dictionary + detect:
- `g2p.rs`: `IpaDictionary` (ใหม่ — โครงสร้างเดียวกับ G2pDictionary): en ARPABET→IPA (เช่น "l"→/l/, "A"→/ɑ/, "aI"→/aɪ/) + ja mora→IPA (เช่น "ら"→/ɾa/) — เริ่มจาก subset ที่ Teto ใช้ (ดู lilt_dict + translit)
- `detect.rs` (ใหม่): detect ภาษา lyric — hiragana/katakana → ja, latin → en (ใช้ translit.rs logic ที่มี)
- Output: `Vec<IpaToken>` — phoneme IPA + stress/type hints

### L2: IPA → Voicebank Adapter
- `adapter.rs` (ใหม่): `IpaAdapter { caps: &CapabilityManager }` — map IPA token → alias เดียว (manifest lookup) + รวมเป็น pairs ตาม manifest.pairs
- fallback chain: IPA→alias → (ไม่มี) **NearestPhoneme** → (ยังไม่มี) hint เหลือ → lyric verbatim (พฤติกรรมเดิม)
- `Phonemizer` trait ใหม่: `UniversalPhonemizer { g2p, adapter }` — process ใช้ L1+L2 แทน map ตรง

### L2.5: NearestPhoneme mapper (map ไปใกล้สุดเมื่อ phoneme ไม่มี)
`nearest.rs` (ใหม่) — **ระบบ map phoneme ที่ bank ไม่มี → ตัวที่ใกล้สุด** (แทน manual a→A, r→r3):
- **Feature vectors ต่อ IPA symbol** (ใน `ipa_dict.rs` เก็บคู่กับ symbol):
  - สระ: `[height(0-3), backness(0-3), rounding(0/1), length(0/1)]` — เช่น ɑ=[2,2,0,0], A(ɑ)≈เดียวกัน, ə=[1,1,0,0]
  - พยัญชนะ: `[place(0-7), manner(0-6), voicing(0/1), nasal(0/1)]` — เช่น r=[4,4,1,0] (alveolar≈approx), l=[4,3,1,0] (alveolar lateral), r3≈[4,4,1,0]
- **Distance:** weighted Euclidean — `missing phoneme` → หา min distance ใน `manifest.phoneme_set` (จำกัด type เดียวกันก่อน: สระ→สระ, พยัญชนะ→พยัญชนะ)
- **Threshold:** distance ≤ T (เช่น 1.0) → ใช้ nearest; > T → ไม่ map (fallback lyric เหมือนเดิม) — ป้องกัน map ผิด (เช่น "v"→"b" ใกล้แต่ผิด)
- เก็บ fallback chain: nearest → lyric — log เหตุผล (เช่น `'a' → 'A' (nearest vowel, d=0.2)`)
- **Test:** Teto EN: "a"→"A" (d≈0), "r"→"r3" (d≈0), "R" (ไม่มี) → ไม่ map; "v"→"v" มี → ไม่ต้อง fallback

---

## Step-by-Step Tasks

### Task 1: PhonemeManifest struct + derive จาก oto
**Files:** Create `crates/voicebank/src/manifest.rs`; Modify `crates/voicebank/src/lib.rs` (pub mod + `Voicebank::phoneme_manifest()`); Test `crates/voicebank/tests/manifest_test.rs`
**Step 1:** เขียน test — `Teto English: manifest.phoneme_set` มี "A","l","r3" / ไม่มี "a","r"
**Step 2:** `cargo test -p voicebank` → FAIL (ไม่มี manifest)
**Step 3:** implement — iterate `oto_map`, split alias บน whitespace, classify type (consonant>0 → consonant; glide list; else vowel), build sets
**Step 4:** PASS + `cargo test -p voicebank` ทั้งหมดผ่าน

### Task 2: IpaDictionary (en subset)
**Files:** Modify `crates/phonemizer/src/g2p.rs` (หรือ Create `ipa_dict.rs`); Modify `lilt_dict.rs` (ขยาย); Test `crates/phonemizer/tests/ipa_test.rs`
**Step 1:** test — `ipa("l") == /l/`, `ipa("A") == /ɑ/`, `ipa("aI") == /aɪ/`, `ipa("3") == /ə/`
**Step 2:** FAIL → implement (HashMap ARPABET→IPA — subset จาก lilt_dict + Teto ใช้)
**Step 3:** PASS

### Task 3: Detect ภาษา
**Files:** Create `crates/phonemizer/src/detect.rs`; Test ใน ipa_test.rs
**Step 1:** test — "ら"→Japanese, "hello"→English, "la"→English
**Step 2:** implement (hiragana/katakana range check — reuse translit.rs)
**Step 3:** PASS

### Task 4: IpaAdapter (IPA→alias ผ่าน CapabilityManager)
**Files:** Create `crates/phonemizer/src/adapter.rs`; Create `crates/phonemizer/src/nearest.rs`; Test `crates/phonemizer/tests/adapter_test.rs`
**Step 1:** test — manifest Teto EN: IPA [l, ɑ] → aliases ["l","A"] + pair ("l","A") ∈ pairs; IPA [a] (ไม่มี "a") → nearest "A"
**Step 2:** FAIL → implement (CapabilityManager lookup + NearestPhoneme + pair assembly + fallback chain)
**Step 3:** PASS

### Task 4.5: CapabilityManager + NearestPhoneme (engine-agnostic)
**Files:** Create `crates/phonemizer/src/capability.rs` (CapabilityManager + `EngineCapabilities` trait กลาง); Create `crates/phonemizer/src/nearest.rs` (feature vectors + distance); Modify `crates/worldline-plugin/src/capabilities.rs` (implement `EngineCapabilities` trait); Test `crates/phonemizer/tests/nearest_test.rs`
**Step 1:** test — `nearest("a", manifest EN) == Some("A")`, `nearest("r", manifest EN) == Some("r3")`, `nearest("R", manifest EN) == None` (> threshold), `can_render("v") == true`; **CapabilityManager** รับ `Box<dyn EngineCapabilities>` (ไม่ใช่ WorldlineCapabilities ตรง) — `missing(["a","v"]) == ["a"]`
**Step 2:** FAIL → implement (feature vectors ใน ipa_dict + weighted distance + threshold; EngineCapabilities trait ใน domain/crate กลาง + WorldlineCapabilities implement; CapabilityManager composition ผ่าน trait)
**Step 3:** PASS

### Task 5: UniversalPhonemizer (wire L1+L2 เข้า Phonemizer trait)
**Files:** Modify `crates/phonemizer/src/phonemizer.rs` (เพิ่ม variant/struct); Modify `english.rs`/`japanese.rs` (delegate หรือ deprecate); Test integration: render scale demo + senbonzakura ผ่าน phonemizer ใหม่
**Step 1:** test — `process()` กับ "la[l A]" hint → phonemes เดิม (ไม่ regression); "ら" lyric (ไม่มี hint) → VCV ผ่าน IPA
**Step 2:** implement — UniversalPhonemizer: hint→parse_hint (เดิม); ไม่มี hint→detect→IPA→adapter
**Step 3:** PASS + `cargo test` ทั้ง workspace (397 ตัวเดิมยังผ่าน)

### Task 6: แก้ตัว Engine (host side — ทำให้ agnostic จริง)
**Files:** Modify `tools/synth-cli/src/engine.rs`; Modify `tools/synth-cli/src/pipeline.rs` (PhonemizerKind); Modify `tools/synth-server/src/render_service.rs`; Test `tools/synth-cli` tests + E2E
**Step 1:** test — `Engine::capabilities()` คืน `&dyn EngineCapabilities` (ไม่ใช่ WorldlineCapabilities ตรง) — สลับ impl ได้; pipeline มี `PhonemizerKind::Universal` (รับ manifest + CapabilityManager)
**Step 2:** FAIL → implement:
- `Engine` trait: `fn capabilities(&self) -> &dyn EngineCapabilities` — `WorldlineEngine` คืน `&WorldlineCapabilities` (ที่ implement trait แล้ว)
- `pipeline.rs`: `PhonemizerKind` เพิ่ม `Universal { manifest: Arc<PhonemeManifest>, caps: Arc<CapabilityManager> }` — render path ใช้ `UniversalPhonemizer` แทน English/Japanese ตรง
- `WorldlineEngine::phonemizer_kind` → สร้าง `Universal` จาก `voicebank.phoneme_manifest()` + engine caps
- server worker: ส่ง manifest ผ่าน (ไม่ผูก worldline)
**Step 3:** PASS + regression: scale-demo + ml-ph2 render ผ่าน Universal path เทียบ golden (RMS 0.995×) + senbonzakura (ja)

### Task 7: E2E + Golden
**Step 1:** render scale demo + machine-love (phoneme-level ustx) ผ่าน phonemizer ใหม่ → เทียบ RMS กับ golden (0.995× เดิม)
**Step 2:** senbonzakura (ja) render ผ่าน IPA path
**Step 3:** ตรวจไม่ regression: `cargo test` + hermes-verify script

---

## Files Likely to Change

- Create: `crates/voicebank/src/manifest.rs`, `crates/phonemizer/src/detect.rs`, `crates/phonemizer/src/adapter.rs`, `crates/phonemizer/src/nearest.rs`, `crates/phonemizer/src/capability.rs` (+ `EngineCapabilities` trait — domain หรือ crate กลาง), `crates/phonemizer/src/ipa_dict.rs` (ถ้าแยก), tests
- Modify: `crates/voicebank/src/lib.rs`, `crates/phonemizer/src/g2p.rs`, `crates/phonemizer/src/phonemizer.rs`, `crates/phonemizer/src/english.rs` (delegate), `crates/phonemizer/src/japanese.rs` (delegate), `crates/phonemizer/src/lilt_dict.rs`, `crates/worldline-plugin/src/capabilities.rs` (implement EngineCapabilities trait), `tools/synth-cli/src/engine.rs` (Engine::capabilities → `&dyn EngineCapabilities`), `tools/synth-cli/src/pipeline.rs` (PhonemizerKind::Universal), `tools/synth-server/src/render_service.rs` (ส่ง manifest)
- Docs: `docs/architecture/` (feed-data-flow)

## Tests / Validation

- Unit: manifest (EN/JP), ipa dict, detect, adapter — cargo test per crate
- Regression: workspace 397 tests ผ่าน
- E2E: scale-demo + ml-ph2 render RMS เทียบ golden; senbonzakura ja render

## Risks / Tradeoffs / Open Questions

- **IPA mapping quality:** ARPABET→IPA subset ต้องตรงกับที่ Teto ใช้จริง — เริ่มจาก known-good (l/A/r3/3...) แล้วค่อยขยาย
- **Nearest mapping ผิดพลาด:** threshold สำคัญ (ป้องกัน v→b) — feature vectors ต้องถูกต้องสำหรับ subset แรก; เริ่ม test ที่ known-good (a→A, r→r3) แล้วค่อยเพิ่ม
- **pair structure ต่าง bank:** CVVC (Teto EN) vs VCV (Teto JP) — adapter ต้องรู้ structure จาก manifest (pairs เป็น 2-token) — ถ้า VCV ต้องการ (prev,cur) context → manifest ต้องเก็บ prev-alias ด้วย (open question — อาจต้อง L2 extension)
- **hint ยังเป็น king:** ไม่เปลี่ยนพฤติกรรม hint (ไม่ regression)
- **YAGNI:** ไม่ทำ full IPA (100+ symbols) — subset ที่ bank ใช้ก่อน; feature vectors เริ่มจาก phoneme ที่ Teto ใช้
- **ไฟล์ .frq/`r`/`a` ปัญหาเดิม:** manifest จะจับได้อัตโนมัติ (bank ไม่มี → fallback) — เป็น benefit ที่คาดหวัง

## Open Questions

1. VCV (Japanese) ต้องการ context (prev mora) — manifest เก็บแค่ pairs พอ หรือต้อง prev-alias map?
2. เก็บ IPA dictionary ใน code (Rust) หรือไฟล์ data? — POC: code (เหมือน lilt_dict)
