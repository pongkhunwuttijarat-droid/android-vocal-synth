# T005 — Finish DiffSinger timing and device acceptance

**Status:** stale / deferred — prior Flutter/Android target assumptions are no longer current
**Owner:** unassigned (Codex completed read-only diagnostic audit)
**Last updated (UTC):** 2026-10-14T11:07:38Z

## Goal

Preserve the DiffSinger timing and device acceptance work as a future task. The
previous plan assumed a Flutter/Android product target; the current direction
is Web wrapper + local Rust server. Revalidate the supported DiffSinger host
and acceptance targets before resuming implementation or device gates.

## Completion criteria

- [ ] Identify the current timing mismatches and the agreed reference/acceptance tolerance.
- [ ] Verify representative projects and voicebanks on target Android hardware.
- [ ] Record runtime, memory, and output evidence for the supported configuration.
- [ ] Update the milestone status with exact evidence and remaining limits.

## Source planning

- [Roadmap MS3.3](../../planning/roadmap/ROADMAP.md)
- [DiffSinger milestone](../../planning/milestones/ms3/DIFFSINGER.md)

## Repository checkpoint

- Branch: `wasm-native-rust`; base commit: `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- Inspected artifacts: `docs/planning/milestones/ms3/{MS,DIFFSINGER}.md`, `native/crates/diffsinger/`, `native/tools/synth-cli/tests/render_real.rs`, and `test/golden/diffsinger-{kiritan,golden-bank}/`. Existing source/model files are dirty or untracked with ownership unknown; none were modified.
- Owned documentation deltas: this T005 record and the T005 board row; untracked in this checkout.
- Durable references: [MS3](../../planning/milestones/ms3/MS.md), [DiffSinger plan](../../planning/milestones/ms3/DIFFSINGER.md), and [T007 performance audit](T007-render-performance-gates.md).
- Steering checkpoint 2026-10-14: user stated T005 is not to be done now and is stale because Flutter is no longer the intended target. Keep the current paired-render diagnostics below, but do not treat Android acceptance or the current implementation path as an active gate until the host/target is revalidated.

## Progress

- 2026-09-30: Added from roadmap snapshot describing tensor-to-WAV as working while timing/device acceptance remained. Verify current implementation and reports.
- 2026-10-14T10:37:48Z Read-only inventory confirms the real Kiritan/golden-bank acoustic and NSF-HiFiGAN models are present (about 272 MB and 57 MB respectively), but milestone MS3 still records phrase/timing wav-level mismatch and Android install/listening as open. The inspected evidence does not record an approved timing tolerance or current target-device measurement. No DiffSinger render, test or device run was performed.
- 2026-10-14T11:05:34Z Paired current-tree reproduction on the same 13-note `test/golden/sakura-test-backup.ustx` and DiffSinger bank changes the diagnosis: C# OpenUtau renders **one** DIFFSINGER phrase with 26 phonemes, 178,176 mono samples / 4,040.3 ms at 44.1 kHz, peak 0.6749. Rust `synth-cli render --engine diffsinger` selects the generic `Universal` phonemizer from the stored track name, splits into two phrases, then skips both because no derived phoneme maps to an oto alias (`s a k u ...`). The Rust command produces no WAV. `DsEngine::render_project` currently delegates to the generic UTAU `pipeline::render_project`; C# DiffSinger instead uses `DiffSingerChinesePhonemizer` and its singer's `TryGetOto` accepts dummy oto entries. Therefore the 15% WAV mismatch/tolerance cannot be evaluated as acoustic parity yet: the Rust and C# render inputs do not match, and Rust does not currently produce a comparison signal on this fixture.

## Verification

- Read-only inspection of `docs/planning/milestones/ms3/{MS,DIFFSINGER}.md`, DiffSinger task plan, test inventory and `test/golden/diffsinger-{kiritan,golden-bank}` model files.
- C# reference harness build: `OPENUTAU_REF=/media/DATA/project/android-voice-synth/ref-openutau-clean WORLDLINE_SO=/media/DATA/project/android-voice-synth/native/build/build-linux/libworldline.so DOTNET_ROOT=/home/seal/dotnet ./build.sh` from `test/golden/golden-renderer` — PASS, 0 errors, 1,042 upstream warnings.
- C# reference render: after linking the fixture's `nsf_hifigan` directory into `/tmp/golden-renderer-data/OpenUtau/Dependencies/nsf_hifigan`, ran `DIFFSINGER_SINGER=/media/DATA/project/android-voice-synth/test/golden/diffsinger-golden-bank ./run.sh phrase /tmp/t005-diffsinger-csharp.wav /media/DATA/project/android-voice-synth/test/golden/sakura-test-backup.ustx` — PASS, one phrase, 26 phonemes, 178,176 mono samples at 44.1 kHz, 4,040.3 ms, peak 0.6749.
- Rust reproduction from `native/`: `cargo run -p synth-cli --features cli -- render --engine diffsinger --ds-model ../test/golden/diffsinger-golden-bank --project ../test/golden/sakura-test-backup.ustx --voicebank ../test/golden/diffsinger-golden-bank --so build/build-linux/libworldline.so --out /tmp/t005-diffsinger-rust.wav --phonemizer universal --verbose` — command exits 1, both phrases skipped for absent oto aliases; no WAV produced. This is a negative integration reproduction, not an acoustic parity result.
- No Android device run/listening or accepted tolerance is recorded. `/tmp` outputs and cache/dependency symlink are diagnostic only and are not deliverables.

Repeatable read-only inventory command from repository root:

```sh
find test/golden/diffsinger-kiritan test/golden/diffsinger-golden-bank -maxdepth 3 -type f -printf '%p %s bytes\n'
rg -n "DiffSinger|diffsinger" native/crates/diffsinger native/tools/synth-cli/tests docs/planning/milestones/ms3
```

Outcome: the C# real-model reference produces audio, while the current Rust project pipeline cannot render this DiffSinger fixture because it uses generic Universal/oto phrase derivation. MS3 timing and target-device/listening work remain open.

## Blockers and dependencies

- The active product host/target for DiffSinger has not been revalidated after moving away from Flutter toward Web wrapper + local Rust server.
- Existing DiffSinger source/phrase findings are diagnostic only. The Rust/C# inputs differ, so no acoustic comparison or tolerance can be claimed from this run.
- Android device acceptance is deferred with the prior Flutter-target assumption; do not use it as a current release gate until the product target is explicitly re-scoped.

## Next step

When this task is resumed, first update the DiffSinger roadmap for the current Web wrapper + local server target and decide whether the required acceptance host is browser/WASM, local native server, or both. Then re-scope parity and device gates; do not resume the old Flutter-specific implementation plan unchanged.

## Handoff notes

The paired C#/Rust render above is historical diagnostic evidence only. It does not authorize Flutter-specific work or establish current product acceptance criteria.
