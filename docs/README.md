# Android Voice Synth — Documentation Index

> เอกสารทั้งหมดของโปรเจกต์

เริ่มรับช่วงงาน agent ที่ [Task Board](work/BOARD.md); อ่านก่อนเริ่มงานหรือรับช่วง
จาก harness อื่น. Board ใช้ติดตาม execution ล่าสุด ส่วน planning ด้านล่างเก็บ
vision, roadmap และ milestone.

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
- [Web Render Backend](planning/web-render-backend.md) — แผนแยก renderer ให้ต่อได้ทั้ง wasm และ server (ยังไม่เริ่ม)
- [Engine vs OpenUtau Performance](planning/engine-vs-openutau-performance.md) — เทียบ wall/CPU/RSS + thread scaling (2026-09-29) และบั๊ก partial-render ของ golden harness

## Architecture

- [Architecture Overview](architecture/README.md) — ระบบหลัก
- [Known Limitations](architecture/known-limitations.md) — ทะเบียนข้อจำกัดที่รู้แล้ว พร้อมเหตุผลและวิธีตรวจ (ONNX บน wasm, audio track, capability reporting, …)
- [Engine-owned State Policy](architecture/engine-owned-state.md) — Rust session เป็น data holder กลางและขอบเขตของ host adapters
- [Shared Project Session API Draft](CONTRACT-v1.md#6-project-session-api-draft-v1--implementation-ready-partially-implemented) — logical contract ร่วม Native/Flutter/Web/WASM (core session implement แล้ว; adapter parity และ WASM stable-ID surface ยังทำต่อ)
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
