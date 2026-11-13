# T008 — Profile and optimize D4C coarse-band bottleneck

**Status:** done  
**Owner:** DSH session (unassigned human owner)  
**Last updated (UTC):** 2026-09-30T05:56:18Z

## Goal

Profile the Rust `world-rs` D4C stage, identify the dominant remaining hotspot, and apply only optimizations that preserve exact render output and have repeatable A/B evidence.

## Completion criteria

- [x] Identify the dominant D4C stage on representative short and long fixtures.
- [x] Add a focused regression test for any semantic-sensitive optimization.
- [x] Compare a valid baseline against the candidate with exact WAV hashes and repeated timing.
- [x] Document the result and rejected ideas.

## Repository checkpoint

- Branch: `wasm-native-rust`
- Base commit: `e77cc1a63a29ede04dfbe2c2c6ae768d94470a7f`
- Relevant working-tree changes: `native/crates/world-rs/src/d4c.rs` is modified by this task; `docs/planning/engine-vs-openutau-performance.md` and `docs/work/` are untracked/pre-existing from earlier work in the session and not claimed by this task except the T008 record/board update.
- Durable artifacts: `out/bench/d4c-profile-scale-demo-app.wav`, `out/bench/d4c-profile-badapple-trunc.wav`, `out/bench/d4c-correct-nolto-ab.json`, `out/bench/d4c-tailfill-nolto-ab.json`, `out/bench/bin/synth-cli-d4c-baseline-correct-nolto`, `out/bench/bin/synth-cli-d4c-tailfill-nolto`.

## Progress

- 2026-09-30T04:45:00Z — Profiled `WORLD_D4C_TIME=1` on 70 D4C phrase calls (scale-demo) and 864 (badapple-trunc). Aggregate coarse-band fill+FFT+sort cost: 52.0% and 50.9% respectively; centroid 17–18%, group smoothing 11–12%, LoveTrain 6%.
- 2026-09-30T05:05:00Z — Attempted to split synthetic D4C coarse timing; do not rely on those substage percentages because the finer instrumentation/build path was not validated end-to-end.
- 2026-09-30T05:15:00Z — Tried unstable sort; not retained. A full WAV comparison was attempted, but its baseline was later found malformed (silent output), so it cannot establish unstable-sort parity. A focused synthetic equivalence test was removed as insufficient evidence; stable `sort_by` remains in production.
- 2026-09-30T05:25:00Z — Tail-only zeroing candidate passed focused equivalence test and matched no-LTO CLI output A/B against a correctly built stable-sort/full-fill baseline, three reps each. scale-demo: 4.082→3.851s, 1.060×; badapple-trunc: 34.498→34.431s, 1.002×. WAV hashes identical 3/3 and output peaks equal/nonzero.
- 2026-09-30T05:46:00Z — Confirmed invalid earlier A/B attempts came from a baseline that zeroed the segment after writing the signal prefix (silent WAV); excluded them. Saved the correct baseline binary separately and reran a valid matched-profile comparison.
- 2026-09-30T05:54:40Z — Corrected report links and handoff to use only `d4c-correct-nolto-ab.json`; removed the misleading unstable-sort test. Unstable sort remains unverified because the attempted comparison used a malformed silent baseline; production keeps stable sorting.
- 2026-09-30T05:56:18Z — Final full WAV hashes match across all 3 reps on scale-demo and badapple-trunc using corrected baseline; reported 6.0% short-fixture improvement and 0.2% long-fixture difference. Temporary D4C example removed.

## Verification

- `rustfmt --edition 2021 --check crates/world-rs/src/d4c.rs crates/world-rs/src/helpers.rs crates/world-rs/src/cheaptrick.rs` — passed 2026-09-30.
- `cargo test -p world-rs -p worldline-rs -p synth-cli --lib` — world-rs 24/24, worldline-rs 16/16, synth-cli 9/9 passed 2026-09-30.
- `cargo test -p world-rs d4c::tests -- --nocapture` — focused D4C tests passed (includes `coarse_band_tail_fill_preserves_fft_input`).
- `cargo test -p world-rs -p worldline-rs -p synth-cli --lib` — world-rs 24/24, worldline-rs 16/16, synth-cli 9/9 passed after removing the misleading unstable-sort test.
- Matched-profile render A/B: no-LTO baseline and candidate, 3 cold runs per fixture, full WAV SHA-256 repeated and identical between binaries; output peak checks 0.66681 (scale-demo) and 1.0 (badapple-trunc).
- Full workspace suite — not run.
- Candidate no-LTO CLI build completed; corrected A/B JSON `out/bench/d4c-correct-nolto-ab.json` verifies full WAV hashes and nonzero peaks across both fixtures.

## Blockers and dependencies

- Unstable sort is not validated for exact full-render parity: the attempted comparison used a malformed silent baseline. Do not reintroduce without a correct matched-baseline A/B.
- Regular thin-LTO CLI relink stalled repeatedly. The accepted A/B therefore uses matched no-LTO release binaries, which isolates the source change but is not the workspace's default LTO build profile.

## Next step

Continue profiling other stages (D4C centroid/FFT or `CheapTrick`) only if requested; keep the verified tail-only fill optimization, and do not claim candidate gains beyond the measured 6.0% short / noise-level long results.

## Handoff notes

`d4c-correct-nolto-ab.json` is the valid A/B. `d4c-matched-nolto-ab.json`, `d4c-final-nolto-ab.json`, and `d4c-tailfill-nolto-ab.json` are invalid: their baseline zeroed the signal segment after writing the prefix, yielding silent audio. Do not use them. Unstable sort has no valid full-render parity result; the corrected matched A/B shows byte-identical output with stable sort and tail-fill only. The accepted change is clearing `segment[window_len..]` after the prefix is written.
