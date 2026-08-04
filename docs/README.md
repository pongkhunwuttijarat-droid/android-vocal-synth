# Android Voice Synth — Documentation Index

> เอกสารทั้งหมดของโปรเจกต์

---

## Planning

- [Planning Overview](planning/README.md) — Vision/Roadmap/Milestones
- [Product Vision](planning/vision/PRODUCT_VISION.md) — ทำไม + แนวทาง
- [Roadmap](planning/roadmap/ROADMAP.md) — MS1-MS4
- [MS1](planning/milestones/ms1/MS.md) — Proof of Concept (กำลังจะเริ่ม)

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

## Reference

- `ref(openutau+openutau mobile)/` — OpenUtau C# reference implementation
