# Android Voice Synth — Documentation Index

> เอกสารทั้งหมดของโปรเจกต์

---

## Planning

- [Planning Overview](planning/README.md) — Vision/Roadmap/Milestones
- [Product Vision](planning/vision/PRODUCT_VISION.md) — ทำไม + แนวทาง
- [Roadmap](planning/roadmap/ROADMAP.md) — MS1-MS4 + WASM/Web track (อัปเดต 2026-09-12)
- [MS1](planning/milestones/ms1/MS.md) — Engine core (เสร็จแล้ว)
- [MS2](planning/milestones/ms2/MS.md) — WORLDLINE integration + Android/golden (เสร็จระดับ acceptance ปัจจุบัน)
- [MS3](planning/milestones/ms3/MS.md) — Renderers (กำลังทำ)
- [MS4](planning/milestones/ms4/MS.md) — Flutter/Web productization (กำลังทำ)
- [WASM Migration Status](planning/wasm-migration-status.md) — สถานะ engine/browser ล่าสุด
- [Web UI Design](planning/web-ui-design.md) — spec + implementation checkpoint ของ editor

## Architecture (design เสร็จแล้ว)

- [Architecture Overview](architecture/README.md) — ระบบหลัก
- [Engine](architecture/engine.md) — core domain
- [Renderer](architecture/renderer.md) — plugin renderers
- [Runtime](architecture/runtime.md) — orchestration
- [Data Contracts](architecture/data-contracts.md) — DTOs
- [Feed Data Flow](architecture/feed-data-flow.md) — data transformation tree
- [Rendering Systems](architecture/rendering-systems.md) — voicebank/model/experimental
- [Runtime Engine Plugins](architecture/runtime-engine-plugins.md) — feasibility ราย engine
- [Decision: Native .so](architecture/decision-native-so-engines.md) — Rust core + C++ kernel
- [Worldline Rust Port](architecture/worldline-rust-port.md) — `r1-rs` pure-Rust + parity
- [WASM Target](architecture/wasm-target.md) — pure-Rust engine บน browser
- [WASM Parallel](architecture/wasm-parallel.md) — Web Worker fan-out + chunk cache
- [WASM Performance](architecture/wasm-performance.md) — benchmark และข้อจำกัด
- [Model Training (MoE)](architecture/model-training-moe.md) — universal-phone training + 2-way conversion (Proposal)

## Reference

- `ref(openutau+openutau mobile)/` — OpenUtau C# reference implementation
