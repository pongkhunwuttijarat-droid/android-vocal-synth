# DiffSinger Renderer — Rust Port Plan

**วันที่:** 2026-08-09
**เป้า:** renderer ครบทุกตัว (MS3) — เริ่ม DiffSinger (MS3.3) → Vogen (MS3.4) → Perf (MS3.5)
**Reference:** `ref-openutau-clean/core/OpenUtau.Core/DiffSinger/` (C# — ~3,500 บรรทัด)

## สถาปัตยกรรม

```
ustx → phonemizer (Universal — reuse) → DsSingerEngine (new crate: diffsinger)
  ├─ DsConfig (dsconfig.yaml) — mel specs, embeds, diffusion params
  ├─ PhonemeTable (phonemes.txt) — token ↔ phoneme index
  ├─ tensor build (C# DiffSingerRenderer.cs:234-265):
  │    tokens[1,N]   = phonemes + SP/SP padding → indices (Int64)
  │    durations[1,N]= PaddedPhoneDurations (headFrames=8, tailFrames=8 — DiffSingerUtils.cs:67)
  │    f0[1,T]       = SampleCurve(pitches, frameMs) — shifted by toneShift (pitch_controllable)
  │    + depth/steps/speedup (diffusion acceleration, optional)
  ├─ variance predictors (DiffSingerVariance.cs) — f0/vuv/dur (use_variable_depth ฯลฯ)
  ├─ acoustic.onnx → mel (128 bins, slaney/htk, base 10/e — checks ตรง C#:144-220)
  └─ vocoder.onnx → PCM (pitch_controllable → shifted f0)
```

**Crate:** `native/crates/diffsinger` (ใหม่ — ไม่พึ่ง worldline-plugin; reuse `ort` + `feed`/`voicebank`/`phonemizer`)
**Engine adapter:** `tools/synth-cli/src/engine.rs` — `WorldlineDsSingerEngine` (หรือ `DsSingerEngine`) — `Engine` trait เดิม
**Voicebank layout:** DiffSinger bank = `character.yaml` + `dsconfig.yaml` + `phonemes.txt` + `acoustic.onnx` + `vocoder.onnx` (ไม่ใช่ oto.ini/wav)

## C# files → Rust mapping

| C# | Rust | ขนาด |
|---|---|---|
| DiffSingerConfig.cs (DsConfig) | `config.rs` — serde_yaml | 75 |
| DiffSingerSinger.cs (load phonemes/dsconfig/sessions) | `singer.rs` | — |
| DiffSingerRenderer.cs (Layout/Render/tensors/acoustic) | `render.rs` | 643 |
| DiffSingerUtils.cs (head/tail frames, PaddedPhoneDurations, SampleCurve) | `utils.rs` | — |
| DiffSingerVariance.cs | `variance.rs` | 353 |
| DiffSingerPitch.cs | `pitch.rs` | 354 |
| DiffSingerVocoder.cs (DsVocoderConfig — pitch_controllable) | `vocoder.rs` | 100 |
| DiffSingerScript.cs / Phonemizers | reuse `phonemizer` (Universal) | — |

## Milestones

- **M1** crate skeleton + DsConfig parser + PhonemeTable + singer load (validate dsconfig keys — C#:141-148)
- **M2** tensor build: tokens/durations/f0 + utils (PaddedPhoneDurations, SampleCurve, toneShift) — unit tests vs C# numbers
- **M3** variance predictors (f0/vuv/dur) — feature flags ตาม config (predict_*)
- **M4** acoustic.onnx run → mel → vocoder.onnx → PCM; mel spec validation (C#:151-220)
- **M5** engine adapter + synth-cli `--engine diffsinger` + `--ds-model <dir>` + render E2E
- **M6** golden vs C# (GoldenUtils — C# `DiffSingerRenderer` เป็น reference) — test model: ต้องหา DiffSinger ONNX test bank (opencpop/diffsinger test — ตัวเล็ก; ยังไม่มีใน repo — download)
- **M7** device (android .so — ort มีอยู่แล้วจาก R2) + app engine dropdown + cache salt

## Open questions

- test model: หา DiffSinger ONNX ตัวเล็ก (openvpi/DiffSinger releases — opencpop mini? หรือ OpenUtau test bank) — ถ้าไม่มี → ใช้ C# golden กับ model ที่หาได้
- `ort` version — R2 ใช้อยู่แล้ว (android .so มี) — reuse
- speaker embedding (speakers list + voice color) — M1 อ่าน config ก่อน, wire ทีหลัง
