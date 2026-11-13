# T037 — E0 real voicebank ONNX model/operator inventory

**Status:** blocked — graph/operator inventory and official support screen complete; Burn weight mapping and exact runtime compatibility remain unverified
**Owner:** DSH coordinator (subagent support)
**Last updated (UTC):** 2026-10-14T12:36:32Z

## Scope and deliverables

Inventory actual supported voicebank ONNX inputs before choosing an architecture: real R2/DiffSinger vocoders first; then only neural phonemizer/G2P paths that have an actual Rust product loader. Record model hash/provenance/license, role, graph inputs/outputs, opsets, operators, static/dynamic shapes, tensor types, external data, architecture/weight mappings, and native ORT reference availability. R2 mel may be a non-blocking parser smoke fixture, never an E1 prerequisite.

## Existing task relationship

- Runs in the isolated Engine worktree from [T036](T036-engine-worktree-preparation.md).
- Uses [T015](T015-media-mixdown-and-real-project-render.md) known limitation, [T005](T005-diffsinger-acceptance.md) native DiffSinger evidence, [T006](T006-vogen-renderer.md) Vogen scope, and [T007](T007-render-performance-gates.md) device matrix as references only.
- Follows E0 in the [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md); official `burn-onnx`/`onnx-ir` are reference/reuse candidates, not assumed runtime executors.

## Dependencies and blockers

- Wave 1 inventory was partial; Wave 2 authorization supported a temporary parser/model-copy inspection only in the isolated Engine worktree. The parser venv and copy were removed. Persistent dependencies, conversion/inference, builds, tests, and product implementation remain unauthorized; exact graph-to-Burn weight mapping and runtime compatibility still block full acceptance.
- No model conversion/inference, dependency upgrade, build/test, or Path B dynamic executor is authorized here.

## Likely files / ownership

Update only the existing report in the isolated Engine worktree; do not create a duplicate report in the main checkout. No current `native/`, voicebank asset, or capability-registry changes are authorized.

## Acceptance and evidence

- [x] Inventory at least one real R2 and one real DiffSinger vocoder candidate; do not infer shared architecture from model names.
- [ ] Record exact supported operators, tensor shapes/dtypes, input/output semantics, weight mapping, external data, licensing and platform limits for each candidate.
- [x] Distinguish source-verified facts, parser evidence, measured runtime evidence, and unverified hypotheses.
- [ ] Identify which compiled Burn model family (if any) can accept weights without per-voicebank WASM rebuild; list explicit unsupported cases and fallback.
- [x] Decide whether neural G2P is an actual Rust-loaded product input; if not, E2 stays blocked with no fabricated loader scope.

## Concurrency and approvals

Read-only source/model inventory may proceed in the isolated T036 worktree during Wave 1. Any parsing that requires a new dependency, model conversion/inference, or product implementation needs separate approval; no task implementation is authorized now.

## Wave 1 inventory checkpoint (2026-10-08)

- Read-only inventory completed under subagent `b9f0b702-67bc-440f-a9e5-f429fe09e564`, with exactly one artifact `/media/DATA/project/android-voice-synth-engine-burn-onnx-wasm-investigation/T037-burn-onnx-model-inventory.md`. T036 worktree was clean at the approved base before inventory; post-write status contains only that report.
- Workspace/provenance verified: the isolated worktree has a synthetic DiffSinger ONNX fixture and checked-in OpenUtau models; actual R2/DiffSinger candidates are absent there and were only read from the main checkout. The final report states this limitation.
- Final source/model findings: R2 `models/r2/pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx`, Kiritan `test/golden/diffsinger-kiritan/nsf_hifigan/model.onnx`, and golden-bank `test/golden/diffsinger-golden-bank/nsf_hifigan/model.onnx` are each 56,616,643 bytes with SHA-256 `b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d`. OpenVPI registry/package identifies PC-NSF-HiFiGAN release 2025-02-27 under CC BY-NC-SA 4.0; notice requires noncommercial use, attribution, full acknowledgements, and license/notice.
- R2 package YAML records 44.1 kHz, hop 512, window/FFT 2048, 128 mel bins, fmin 40/fmax 16000, log base e, Slaney, pitch control. DiffSinger sidecars specify 128 bins and pitch control but omit mel_base; Rust DsVocoderConfig defaults absent mel_base to 10, so actual weight encoding is unverified. Native source passes ordered float32 mel `[1,N,128]` plus f0 `[1,N]` to ORT and consumes first float output `[1,L]`; R2 comments expect N×512 then trim/ease.
- Graph operators/attributes/names/shapes/external data remain unknown: no parser is installed (`ModuleNotFoundError: No module named 'onnx'`); no ONNX metadata/inference was performed. No supported Rust neural G2P loader was found, so E2 remains out of scope.
- Report findings are partial: the three real vocoder paths are byte-identical at 56,616,643 bytes/SHA-256 `b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d`; OpenVPI PC-NSF-HiFiGAN/CC BY-NC-SA 4.0; R2 mel-base `e` versus DiffSinger loader default `10`; actual graph metadata and Burn compatibility unknown.
- No build/test, model execution/conversion/inference, dependency changes, main-checkout asset writes, or capability/product edits were performed. T037 remains blocked until graph metadata can be inspected within approved constraints.

## Wave 2 graph/parser checkpoint (2026-10-08)

- The existing report at /media/DATA/project/android-voice-synth-engine-burn-onnx-wasm-investigation/T037-burn-onnx-model-inventory.md now contains a read-only parse of the three byte-identical candidates (56,616,643 bytes each; SHA-256 b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d). R2, Kiritan and golden-bank copies match byte-for-byte. Provenance is OpenVPI PC-NSF-HiFiGAN dated 2025-02-27, CC BY-NC-SA 4.0.
- Parsed graph: ONNX IR 8, opset 15, 272 nodes, inputs mel FLOAT [1,n_frames,128] and f0 FLOAT [1,n_frames], output waveform FLOAT [1,n_samples], symbolic frame/sample lengths; 213 embedded dense initializers (204 FLOAT, 8 INT64, 1 INT32), with no external or sparse tensor data. Operators and counts: Add 59, Conv 93, ConvTranspose 5, CumSum 1, Div 7, LeakyRelu 86, Mod 2, Mul 5, Pad 2, Reshape 1, Sin 1, Slice 4, Squeeze 1, Sub 2, Tanh 1, Transpose 1, Unsqueeze 1.
- At pinned tracel-ai/burn-onnx commit f20ba03537296cc5defefb02f1b799b4c23727d1, the official table lists support for all 17 operator families and documents opset 1–24. This is only operator/opset screening: the exact graph was not imported, compiled, run, or compared numerically; a Burn model-family/weight mapping and dynamic-length/backend compatibility remain unverified. The R2 mel-base e versus DiffSinger loader default 10 discrepancy also remains unresolved.
- Parser and disposable model copy were removed. No product source/Cargo dependencies, source model, conversion, inference, build, or test changed. Keep T037 blocked on unresolved acceptance items; T038–T043 remain blocked and no implementation is authorized by this inventory.

## Repository checkpoint and next step

- Branch/worktree: `engine/burn-onnx-wasm-investigation` at base `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`; inventory report is restricted to the isolated worktree.
- Current checkout: `wasm-native-rust`, HEAD `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`; dirty-tree ownership unknown.
- Plan reference: [Runtime ONNX Import plan](../../planning/burn-onnx-wasm-investigation-plan.md).
- Next: only after separate approval, map initialized weights to an actual Burn model family and run controlled import/compile/shape/runtime validation. Until then, exact graph compatibility remains explicitly unverified; do not add persistent dependencies or convert/infer here.

## Current-main revalidation — 2026-10-14

- `sha256sum` of the R2, Kiritan, and golden-bank PC-NSF-HiFiGAN files in the current `wasm-native-rust` checkout returns the identical hash `b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d` for all three.
- Current `native/crates/diffsinger/src/config.rs` still defaults an omitted `mel_base` to `10`; current `native/crates/worldline-plugin/src/r2_engine.rs` still forms positional float32 `[1,N,128]` mel and `[1,N]` f0 tensors. These source checks confirm the recorded mel-encoding discrepancy remains relevant to current main.
- The read-only parser report exists at `/media/DATA/project/android-voice-synth-engine-burn-onnx-wasm-investigation/T037-burn-onnx-model-inventory.md`; that isolated report is based on `e77cc1a...`, older than current main. The current-main recheck did not repeat parsing, conversion, inference, builds or tests.
- E0 remains blocked on exact Burn graph import/weight mapping/runtime compatibility and expected mel encoding. Conversion, inference, builds, dependency changes and product integration remain outside current authorization.

## Current-checkout revalidation — 2026-10-15T02:23:16Z

- Current UI checkout is branch `wasm-native-rust` at `e6af244abbf04d4a490fca8483295ed6418e6cad`, newer than the isolated Engine inventory base `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`. The relevant DiffSinger/R2 loader and model-config files and all three candidate model/config files are clean in the working tree.
- Recomputed SHA-256 for the R2, Kiritan, and golden-bank vocoder files: each remains `b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d` (56,616,643 bytes). R2 still explicitly declares `mel_base: e`; both DiffSinger vocoder YAMLs omit mel-base and the Rust `DsVocoderConfig` default remains the `10`-base path.
- No new parser, conversion, inference, build, test, dependency or product change was performed. This newer-checkout recheck confirms the prior inventory findings but does not close exact graph-to-Burn mapping, runtime compatibility, or expected mel encoding. Those experiments still require the separate approval specified by the plan.
