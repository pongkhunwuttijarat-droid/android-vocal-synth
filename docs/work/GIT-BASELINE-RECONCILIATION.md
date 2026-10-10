# Git baseline reconciliation

## Scope and guardrails
This record documents a file-by-file review of the 347 untracked paths present before staging. Only explicitly approved, reviewed documentation/source candidates were staged. Existing modified and deleted tracked files were not staged. Private profiles, local projects/settings, caches, models, training data, personal music, generated artifacts, uncertain license/provenance, and unknown-owner files remain unstaged or ignored. No commit, push, merge, cleanup, reset, checkout, or ignore-rule change was performed.

## Repository baseline
- Checkout: main Android Vocal Synth worktree
- Branch: wasm-native-rust
- HEAD: e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f
- Upstream: origin/wasm-native-rust; ahead/behind: 0/0
- Worktrees: main checkout and Engine investigation worktree; both were at the recorded HEAD at audit time.
- Before staging: 94 modified, 9 deleted, 347 untracked, 0 staged; 43,672 ignored paths.
- After staging 22 reviewed candidates plus this manifest: 94 modified, 9 deleted, 325 untracked, 23 staged additions. No tracked modification/deletion was staged.

## Inventory disposition
| Group | Reviewed | Safe candidates staged | Excluded | Held / unresolved |
| --- | ---: | ---: | ---: | ---: |
| docs | 87 | 19 | 36 | 32 |
| ui-mock/react-dock | 72 | 2 | 57 (including 10 private browser-profile files) | 13 |
| app/native/web | 88 | 1 | — | 87 not safe to stage; owner/license/privacy/provenance dispositions remain mixed |
| other POC/scripts/tests/local artifacts | 100 | 0 | 15 | 85 |

The safe source candidate web/app/curve-stroke-transport.js is transport-only and owned by T009; Rust remains authoritative. UI documentation candidates are owned by T004. Shared UI code, T022/T023 overlap, private profiles, model/training assets, personal projects, local caches, OpenUtau-derived content, and uncertain-license material were not staged. No private browser-profile contents were opened. The docs board and Wave 2 ownership report were excluded as-is because they contain local/private project, voicebank, server, HEAD, dirty-tree, model, or profile details.

## Staged files
The exact 22 reviewed candidate additions, plus this manifest, are staged:

- docs/architecture/web-session-manager.md
- docs/architecture/web-session-render-adapter.md
- docs/planning/contract-v2-freeze-proposal.md
- docs/planning/high-frequency-editing-flow.md
- docs/planning/phoneme-compose-contract-proposal.md
- docs/planning/contract-v2-extension-t019-media-mixdown.md
- docs/work/TASK_TEMPLATE.md
- docs/work/tasks/T002-flutter-saf-import.md
- docs/work/tasks/T005-diffsinger-acceptance.md
- docs/work/tasks/T006-vogen-renderer.md
- docs/work/tasks/T007-render-performance-gates.md
- docs/work/tasks/T010-contract-adapter-conformance.md
- docs/work/tasks/T012-rust-part-project-commands.md
- docs/work/tasks/T038-burn-vocoder-runtime-import.md
- docs/work/tasks/T039-burn-opfs-runtime-cache.md
- docs/work/tasks/T040-burn-onnx-parity.md
- docs/work/tasks/T041-neural-g2p-runtime-support.md
- docs/work/tasks/T042-diffsinger-runtime-compatibility.md
- docs/work/tasks/T043-burn-cross-platform-product-gates.md
- ui-mock/react-dock/README.md
- ui-mock/react-dock/tests/manual-checklist.md
- web/app/curve-stroke-transport.js
- docs/work/GIT-BASELINE-RECONCILIATION.md (this manifest)

## Safety scans and review findings
- Credential-literal scan: no matches in staged candidates.
- Email/personal absolute-path scan: no actual matches. The broad /media detector hit docs/planning/contract-v2-extension-t019-media-mixdown.md:26 only because the documented route is PUT /api/v2/media/{asset_id}.
- Loopback scan: ui-mock/react-dock/README.md:17 and ui-mock/react-dock/tests/manual-checklist.md:26 contain intentional localhost development URLs.
- Original 347 untracked paths total 14,640,675 bytes. Three exceeded 1 MiB (a personal project and two private profile cache files); eight exceeded 256 KiB, all excluded. Models/training data and other ignored paths were left untouched.
- git diff --cached --check returned exit 2 for intentional Markdown hard-break spaces; no whitespace normalization was applied. Findings: docs/architecture/web-session-render-adapter.md:3; docs/work/TASK_TEMPLATE.md:3-4; docs/work/tasks/T002-flutter-saf-import.md:3-4; docs/work/tasks/T005-diffsinger-acceptance.md:3-4; docs/work/tasks/T006-vogen-renderer.md:3-4; docs/work/tasks/T007-render-performance-gates.md:3-4; docs/work/tasks/T038-burn-vocoder-runtime-import.md:3-4; docs/work/tasks/T039-burn-opfs-runtime-cache.md:3-4; docs/work/tasks/T040-burn-onnx-parity.md:3-4; docs/work/tasks/T041-neural-g2p-runtime-support.md:3-4; docs/work/tasks/T042-diffsinger-runtime-compatibility.md:3-4; docs/work/tasks/T043-burn-cross-platform-product-gates.md:3-4.

## Stop point
All remaining untracked, modified, deleted, ignored, and held files remain untouched. This is a staged baseline only; do not commit or publish without a separate user instruction.
