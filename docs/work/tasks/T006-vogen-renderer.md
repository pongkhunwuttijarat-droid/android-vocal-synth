# T006 — Implement and validate Vogen renderer

**Status:** blocked — VocV1 vocoder weights exist upstream, but a licensed singer/acoustic-duration package and runtime scope remain unresolved
**Owner:** unassigned (Codex completed read-only audit)
**Last updated (UTC):** 2026-10-15T05:16:49Z

## Goal

Integrate the Vogen `f0_man` and VSVocoder path into the engine and establish
repeatable correctness and product acceptance.

## Completion criteria

- [ ] Confirm model format, licensing/provenance, runtime, and target hardware requirements.
- [ ] Implement engine integration and the required input/output adaptation.
- [ ] Add representative tests or golden comparisons for timing and audio output.
- [ ] Document device performance, memory limits, and supported configuration.

## Source planning

- [Roadmap MS3.4](../../planning/roadmap/ROADMAP.md)
- [MS3 milestone](../../planning/milestones/ms3/MS.md)

## Repository checkpoint

- Branch: `wasm-native-rust`; base commit: `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- Inspected artifacts: `native/`, `models/`, `docs/architecture/{rendering-systems,feed-data-flow,runtime-engine-plugins}.md`, MS3 plans, and T013's phonemizer matrix. Existing source/model files are dirty or untracked with ownership unknown; none were modified.
- Owned documentation deltas: this T006 record and the T006 board row; untracked in this checkout.
- Durable references: [MS3](../../planning/milestones/ms3/MS.md), [runtime engine plugins](../../architecture/runtime-engine-plugins.md), and [T013 phonemizer matrix](T013-openutau-official-phonemizer-matrix.md).

## Progress

- 2026-09-30: Added from roadmap snapshot marking Vogen not started. Check for newer implementation work before treating that as current.
- 2026-10-14T10:37:48Z Read-only search found no `native/crates/vogen` renderer or Vogen model artifact in this checkout. Vogen-related docs/feed structures and OpenUtau phonemizer references exist, but do not establish a renderer. The existing phonemizer inventory reports native ONNX loading requirements and Browser/WASM failure; this is not Vogen renderer evidence. Model format/provenance, redistribution terms and a representative Mandarin/Cantonese model remain unverified. No implementation or model download was performed.
- 2026-10-14T10:59:12Z Official upstream research narrows the gap: [VogenSVS](https://github.com/aqtq314/VogenSVS) identifies VocV1/VogenVoc demo code and a breathiness-generator training repo, but lists the acoustic and duration models as still under construction. The repo has no release/package entry or visible top-level license in its published file list. Its [BreathGen repo](https://github.com/aqtq314/VocV1-BreathGen) lists an MIT code license and external pretrained RMVPE/separator weights; those weight terms and the actual `f0_man` singer model are not established by that code license. Therefore this is a vocoder research reference, not a complete legally cleared singer fixture. No downloads or executions were performed.
- 2026-10-14T11:13:22Z Rechecked the live official repository and current checkout. `git ls-remote https://github.com/aqtq314/VogenSVS.git HEAD refs/heads/main refs/tags/'*'` returns main/HEAD `860f09ba6950326fd04bbdf843d3766c9b187a85` and no tags. GitHub lists 25 commits, no releases, VocV1/VogenVoc demo code and Breathiness Generator training code, while acoustic and duration model entries remain blank/under construction. The repository root listing has no license file. `rg --files native models test | rg -i 'vogen|vsvocoder|f0[_-]?man|vocv1'` returned no files in the current checkout. At that point, the task record did not inspect the tracked upstream model blobs, so the absence claim was too broad; see the 11:25 checkpoint below.
- 2026-10-14T11:25:35Z Corrected the upstream artifact inventory using a shallow no-checkout clone at `/tmp/vogen-svs-tree` (model blobs were not checked out/downloaded). The same pinned commit `860f09ba6950326fd04bbdf843d3766c9b187a85` tracks a VocV1 inference set: [TorchScript Breathiness Generator](https://github.com/aqtq314/VogenSVS/blob/860f09ba6950326fd04bbdf843d3766c9b187a85/vogensvs/assets/%5Bvocv1-brgen%5D220801a-d1%2BWNorm-c16c12/220801a-d1%2BWNorm-c16c12_0127.jit.pt) (38,907 bytes, blob `00edd8f5e1264ccd96c1d489a95ab37551d0c17a`), companion `.pt` (17,754 bytes, blob `20b6a459f8c0c4208818a82bdfe451420a6f994f`), and [harmonic/breathiness decomposition ONNX](https://github.com/aqtq314/VogenSVS/blob/860f09ba6950326fd04bbdf843d3766c9b187a85/vogensvs/assets/%5Bvocv1-hbdecomp%5D%5B220228-001211%5D%5Bimg73%5Dmodelv5c/model.onnx) (2,261,979 bytes, blob `ba123a0b5c723388d5e3feb66d7b2cb3c52a71db`). The demo notebook documents 44.1 kHz-only pretrained vocoder components; `Analyzer` loads the harmonic/breathiness-decomposition ONNX, and the PyTorch synthesizer loads the TorchScript Breathiness Generator. These are vocoder assets, not a Vogen acoustic/duration singer model or a `.vogen`/`f0_man` package. The repository tree has no root LICENSE, no acoustic/duration model artifacts, no `f0_man` references in source/metadata, and no releases/tags; the separate BreathGen repo labels its training code MIT but references separately hosted RMVPE weights. Model-weight and training-data provenance/redistribution rights remain unverified. No model weights were downloaded, loaded, or executed.
- 2026-10-14T12:38:13Z Revalidated official upstream after the earlier check: `git ls-remote` still resolves VogenSVS HEAD/main to `860f09ba6950326fd04bbdf843d3766c9b187a85`, with no tags returned. The official repository page still labels Acoustic Model and Duration Model as unfinished, lists only VocV1 demo code/BreathGen training code, and shows no releases; the root file list has no LICENSE. BreathGen is still presented as a training-code repository under MIT, and its README points to separately hosted RMVPE/separator weights and an OpenSinger dataset. No `.vogen`/`f0_man` singer package or verified model/data redistribution grant has appeared in this current-state check. No assets were downloaded or run. **T006 remains blocked pending either a licensed acoustic/duration singer package plus its provenance, or a separately approved VocV1-only spike with weight/data rights resolved.**
- 2026-10-15T01:14:51Z Rechecked the official upstream pages live. The [VogenSVS repository](https://github.com/aqtq314/VogenSVS) still marks VocV1 as demo code and has blank Acoustic Model/Duration Model entries; its [releases page](https://github.com/aqtq314/VogenSVS/releases) still lists no releases. The [BreathGen repository](https://github.com/aqtq314/VocV1-BreathGen) identifies MIT for its training code while separately requiring pretrained RMVPE/separator weights and OpenSinger training data. This does not establish redistribution rights for those weights/data or provide the missing score-to-acoustic singer package. No files, assets, or dependencies were changed or downloaded. T006 remains blocked on the same concrete external input: a licensed model package with provenance, or explicit scope authorization plus cleared rights for a VocV1-only spike.

## Verification

- Read-only file inventory/search of `native/`, `models/`, `docs/architecture/`, MS3 plans and T013 phonemizer matrix.
- Read-only official upstream pages: [VogenSVS](https://github.com/aqtq314/VogenSVS) (project status includes VocV1 demo while acoustic/duration models are blank) and [VogenSVS releases](https://github.com/aqtq314/VogenSVS/releases) (no releases); `git ls-remote` pinned main/HEAD to `860f09ba6950326fd04bbdf843d3766c9b187a85`. The shallow metadata-only clone identifies three tracked VocV1 inference weight blobs listed above; the README/notebook state that pretrained vocoder works at 44.1 kHz. [VocV1-BreathGen](https://github.com/aqtq314/VocV1-BreathGen) identifies its training code as MIT but references separate pretrained weights. Model-weight/data licenses are not established. No model blob or dataset was downloaded or executed.
- No model was downloaded, built or executed; no tests run.
- 2026-10-14T12:38:13Z Current upstream verification: `git ls-remote https://github.com/aqtq314/VogenSVS.git HEAD refs/heads/main refs/tags/'*'` returned HEAD/main `860f09ba6950326fd04bbdf843d3766c9b187a85` and no tags. Official [VogenSVS repository](https://github.com/aqtq314/VogenSVS) page lists vocoder demo code, blank Acoustic Model/Duration Model entries, and no root license; [releases page](https://github.com/aqtq314/VogenSVS/releases) reports no releases. [VocV1-BreathGen](https://github.com/aqtq314/VocV1-BreathGen) shows MIT for the training code, while its README references separate pretrained weights and OpenSinger dataset. This revalidates source availability only; it is not a licensing opinion or inference result.
- 2026-10-15T01:14:51Z Live official page check reproduced the same state. This is current availability/README evidence only; it does not decide legal rights or demonstrate a model's runtime.

## Official upstream revalidation (2026-10-15T01:48:09Z)

- Re-opened the official [VogenSVS repository](https://github.com/aqtq314/VogenSVS) and [release page](https://github.com/aqtq314/VogenSVS/releases). The README still lists only VocV1 demo code and the BreathGen training-code link; Acoustic Model and Duration Model have no linked implementation, and Releases/Packages are empty. This does not provide the end-to-end singer package T006 requires.
- Re-opened the official [VocV1-BreathGen repository](https://github.com/aqtq314/VocV1-BreathGen). Its repository license is MIT, but its README separately requires pretrained RMVPE weights, a harmonics/breathiness separator model, and OpenSinger data. The README does not assign those separately downloaded artifacts the repository's MIT license. This reinforces the existing asset-rights blocker; do not download or load them as a product dependency.
- The README's multi-GPU training resource figures describe training/preprocessing, not inference requirements, so they do not establish target-device feasibility for Rusted UTAU.
- Result: no blocker cleared and no implementation authorization inferred. Full renderer still needs a licensed acoustic/duration singer package; a VocV1-only spike remains a distinct scope requiring explicit authorization and artifact-rights evidence.

Repeatable read-only inventory command from repository root:

```sh
rg --files native models docs test | rg -i 'vogen|vsvocoder|vogen.*onnx'
rg -n -i "vogen" native docs test models
```

Outcome: no Vogen renderer crate or model file; matches are architecture/feed planning and phonemizer references only. No inference/runtime build attempted.

## Blockers and dependencies

- No `.vogen`/`f0_man` singer package or acoustic/duration model was found in the workspace or current official upstream tree at `860f09ba6950326fd04bbdf843d3766c9b187a85`. That tree does contain a separate pretrained VocV1 vocoder and analysis-weight set, but its weight/data rights are not documented in a root license or model card. The full task needs a representative legally usable score-to-acoustic singer package and provenance/license before end-to-end renderer scope can be accepted. A vocoder-only adapter could be scoped from the existing VocV1 assets only after explicit authorization to retrieve/load them and rights review; runtime and target-hardware requirements are still unverified.

## Next step

For the full Vogen renderer, obtain a representative legally usable `.vogen`/`f0_man` singer or acoustic/duration package and its license/provenance. For a separate VocV1-only spike, first authorize model retrieval/loading and establish the weights/data rights, then pin the inference/runtime inputs. Do not infer that the MIT training-code license clears the pretrained weights.

## Handoff notes

Keep renderer/model execution behind the engine renderer interface and host adapters.


## Current official recheck (2026-10-15T03:49:18Z)

- `git ls-remote https://github.com/aqtq314/VogenSVS.git HEAD refs/heads/main 'refs/tags/*'` returned HEAD/main `860f09ba6950326fd04bbdf843d3766c9b187a85` and no tag refs. The [official repository](https://github.com/aqtq314/VogenSVS) README still marks VocV1 as the vocoder demo and leaves Acoustic Model and Duration Model entries blank; its [Releases page](https://github.com/aqtq314/VogenSVS/releases) says there are no releases.
- The [official BreathGen README](https://github.com/aqtq314/VocV1-BreathGen) describes a training-script repository with an MIT code license, and separately requires pretrained RMVPE/separator ONNX weights and OpenSinger training data. Those separately supplied artifacts are not assigned rights by the repository's code license.
- This refresh confirms the recorded blocker; it does not supply a licensed score-to-acoustic singer package or clear the VocV1 weights/data. No downloads, model execution, or source changes. Next work scope is awaiting the user's T006 choice between full renderer after a licensed singer package and a separately scoped VocV1-only rights/input audit.

## Current official recheck (2026-10-15T04:10:54Z)

- Opened the official VogenSVS repository and Releases page again. The README still lists VocV1 demo code, with Acoustic Model and Duration Model entries blank; the repository shows no releases. The separate BreathGen repository remains a training-code repository and does not establish rights for separately fetched pretrained models or training data.
- No repository or model files were downloaded or executed. This confirms the prior state but does not clear the model, license, or runtime gates. Asked the user to choose whether T006 should wait for a licensed end-to-end singer package or move to a bounded VocV1-only research spike that does not retrieve/run uncleared weights.

## Current official recheck (2026-10-15T04:46:58Z)

- Re-opened the official [VogenSVS repository](https://github.com/aqtq314/VogenSVS), [releases](https://github.com/aqtq314/VogenSVS/releases), and [VocV1-BreathGen repository](https://github.com/aqtq314/VocV1-BreathGen). VogenSVS still lists VocV1 demo code while its Acoustic Model and Duration Model entries remain blank, and GitHub still lists no releases. BreathGen still identifies MIT for its training code and separately instructs users to obtain RMVPE and separator weights plus OpenSinger data; those references do not establish redistribution rights for those artifacts.
- No new end-to-end singer package, model provenance/license, or runtime/device evidence surfaced. No files or model assets were downloaded or executed. T006 remains blocked on a licensed acoustic/duration singer package or explicit scope approval plus cleared inputs for a separately bounded VocV1-only spike.

## Current official recheck (2026-10-15T05:16:49Z)

- Re-opened the official [VogenSVS repository](https://github.com/aqtq314/VogenSVS) and [releases page](https://github.com/aqtq314/VogenSVS/releases). The README still lists only VocV1 as vocoder demo code, leaves the Acoustic Model and Duration Model entries blank, and the releases page has no releases. This still cannot supply a complete score-to-acoustic singer package.
- Re-opened the official [VocV1-BreathGen repository](https://github.com/aqtq314/VocV1-BreathGen). It labels its training-code repository MIT, but separately instructs users to fetch pretrained RMVPE and harmonic/breathiness-separator weights and to use the OpenSinger dataset. The repository page does not extend its code license to those separately sourced artifacts.
- No assets were downloaded or executed, and no implementation or test was run. The external decision remains whether to wait for a licensed end-to-end Vogen singer package, or explicitly scope a VocV1-only research spike after its specific model/data rights are established.
