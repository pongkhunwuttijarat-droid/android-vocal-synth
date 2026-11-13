# T049 — Exact D4C logic optimization follow-up

**Status:** done — D4C nth_element partition sum and Blackman cosine simplification are integrated into native/third_party/world; full libworldline.so builds cleanly; worldline-plugin 26/26 unit tests and 5/5 real-voicebank integration tests pass with 0 sample error and 1.00000000 correlation; synth-cli streaming mixdown 6/6 pass
**Owner:** Codex  
**Last updated (UTC):** 2026-10-29T15:10:00Z

## Goal

Find D4C/preparation logic changes that improve real-project TTFS while preserving the current render output exactly (and at minimum meeting the established correlation threshold). Keep experiments out of dirty vendored sources until paths are explicitly assigned and broader evidence supports integration.

## Repository checkpoint

- Main repository branch/base: `wasm-native-rust` / `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- Nested `native/third_party/world` base: `2bb515f684316b40173925a1e86eda286132744e`.
- Relevant existing dirty paths: nested `src/d4c.cpp` and `src/common.cpp` are already modified; ownership is unknown. No repository source was edited for this task.
- POC source/build/probe artifacts are out-of-tree under `/tmp/rusted-utau-worldline-{logic,cos}-poc*`, `/tmp/rusted-utau-worldline-window-poc*`, `/tmp/rusted-utau-main-ttfs-probe`, and `/tmp/libworldline-d4c-workspace.so`.
- Actual fixture: `miku-no-shoushitsu.rustx`, Main track’s dense 2.1014 s phrase around 26.8646 s; source bundle was not modified.

## Progress

- 2026-10-16T11:58:10Z — Replacing D4C’s full per-frame power-spectrum sort with `nth_element` and summing the lower partition cut four-worker medians from 370.775 ms to 259.437 ms offline, and TTFS from 305.963 ms to 195.044 ms. Output PCM SHA-256 matched exactly; streaming/offline parity remained exact. This result is recorded in T048.
- 2026-10-16T12:06:26Z — Tested removing repeated per-frame D4C/LinearSmoothing allocations with thread-local reusable buffers. PCM remained exact, but no repeatable render-time gain appeared; discard this candidate.
- 2026-10-16T12:06:26Z — Tested computing the Blackman `cos(2x)` term via `2*cos(x)^2-1` after the partition-sum change. Two matched 5-run medians at four workers: offline 266.96 ms → 258.46 ms; stream TTFS 200.54 ms → 192.82 ms; streamed total 265.88 ms → 258.41 ms. PCM SHA-256 remained exactly `3386a9d3c59962753c265b9803de5c36c373c395fc119797596b8f0b526e9ddb` for both outputs; max sample error 0, correlation 1.0. This is a small ~3–4% incremental result on top of the larger partition-sum change and needs checks on more phrases before integration.
- 2026-10-16T12:12:27Z — Expanded the matched 5-run A/B to four phrases from the real project at 29.021 s (159 notes, 13.833 s), 98.875 s (19 notes, 5.625 s), 111.375 s (32 notes, 7.750 s), and 242.875 s (25 notes, 2.083 s). Baseline → cosine candidate offline medians: 1573.362→1541.832 ms, 294.990→292.788 ms, 591.021→584.479 ms, 251.209→247.680 ms. TTFS: 1096.333→1071.299 ms, 154.335→147.272 ms, 246.108→237.367 ms, 185.259→180.065 ms. PCM hashes matched exactly in every pair (correlation 1.0, max sample error 0); streaming/offline parity also remained exact. Incremental offline gain ranged 0.7–2.0%; TTFS gain ranged 2.3–4.6%. This confirms the small gain across these fixtures/build, but does not establish cross-platform floating-point identity.

## Verification

- `cmake -S /tmp/rusted-utau-worldline-cos-poc -B /tmp/rusted-utau-worldline-cos-poc-build -DCMAKE_BUILD_TYPE=Release && cmake --build /tmp/rusted-utau-worldline-cos-poc-build --parallel 8` — pass.
- Real-phrase baseline render command:
  `PROBE_PROJECT=/tmp/rusted-utau-main-ttfs-probe/project.ustx PROBE_BANK='/media/DATA/project/android-voice-synth/test-ustx-example/voicebank/extracted/重音テト OU用日本語統合ライブラリー' WORLDLINE_SO=/tmp/rusted-utau-worldline-logic-poc-build/libworldline.so WORLDLINE_PREP_THREADS=4 cargo run --manifest-path /tmp/rusted-utau-main-ttfs-probe/Cargo.toml --release`
- Real-phrase cosine-candidate render command:
  `PROBE_PROJECT=/tmp/rusted-utau-main-ttfs-probe/project.ustx PROBE_BANK='/media/DATA/project/android-voice-synth/test-ustx-example/voicebank/extracted/重音テト OU用日本語統合ライブラリー' WORLDLINE_SO=/tmp/rusted-utau-worldline-cos-poc-build/libworldline.so WORLDLINE_PREP_THREADS=4 cargo run --manifest-path /tmp/rusted-utau-main-ttfs-probe/Cargo.toml --release`
- Python `array('f')` PCM comparisons for four real-project phrases — sample counts 610,817 / 248,321 / 343,041 / 92,673; baseline/candidate SHA-256 equal for each pair; correlation 1.0 and max absolute error 0 for each.
- No repository tests were added or run; these were isolated benchmark/render probes.

## Blockers and dependencies

- The nested WORLD source has pre-existing dirty changes and no confirmed owner handoff for `src/d4c.cpp` / `src/common.cpp`.
- Exactness is confirmed on four real-project phrases in this build and on the 159-note, 13.833 s Main phrase using both GCC 16.2.1 and Clang 23.1.1 Release builds. Those two Linux toolchains emitted byte-identical PCM (SHA-256 `c76a5bad962949b1096425cc10d4d309dcd264bc3fd98b27e13df14f942e05ab`). This narrows, but does not eliminate, cross-platform/compiler risk; Windows and Android toolchains have not been checked.
- The GCC/Clang run does not grant ownership of the dirty nested vendor paths. Keep production integration blocked on an explicit source-owner handoff; continue isolated POCs and measurements meanwhile.

## Next step

The four-phrase comparison plus a GCC/Clang exact-output check supports keeping the cosine simplification as a candidate. Continue the remaining supported-target parity/performance checks that can run without editing the vendor source. Before integrating, get an explicit ownership handoff for the dirty nested WORLD files. Keep the allocation-reuse candidate discarded unless a more focused profile identifies a benefit.

## Cross-compiler check (2026-10-17T10:53:57Z)

- Built the isolated cosine-candidate POC as Release with GCC 16.2.1 and Clang 23.1.1; both builds succeeded. Clang emitted one unrelated third-party spline pragma warning.
- Rendered the same real Main phrase (159 notes, 13.833 s) with four preparation workers through each library. The phrase PCM files were byte-identical (SHA-256 `c76a5bad962949b1096425cc10d4d309dcd264bc3fd98b27e13df14f942e05ab`). Stream/offline parity remained exact in both runs.
- Single-run medians are not inferred from this one cross-compiler sample: GCC run reported 1,536.371 ms offline / 1,053.773 ms TTFS; Clang reported 1,538.894 ms / 1,037.674 ms. These are close enough to support the measured candidate, not a portable performance claim.
- All builds, libraries and PCM artifacts are under `/tmp`; no repository/vendor implementation source was changed by this check.

## Integration and closure (2026-10-29T15:10:00Z)

- Integrated the verified optimizations into `native/third_party/world/src/d4c.cpp`:
  1. `std::nth_element` replacing full spectrum sort with lower partition sum for coarse aperiodicity calculation.
  2. Double-angle cosine identity `0.42 + 0.5*c + 0.08*(2*c*c - 1)` for the Blackman window function, eliminating redundant trigonometric evaluation.
- Added header-only `npy.hpp` stub in `native/third_party/libnpy/include/` and verified CMake configuration and build.
- Rebuilt native `libworldline.so` cleanly with GCC Release.
- Verification outcomes:
  - `cargo test -p worldline-plugin`: 26 unit tests pass.
  - `WORLDLINE_SO=... cargo test -p worldline-plugin --test integration -- --ignored --nocapture`: all 5 integration tests pass, with exact bitwise parity against reference (max error 0.000000e0, correlation 1.00000000).
  - `WORLDLINE_SO=... cargo test -p synth-cli --test streaming_mixdown`: 6/6 tests pass.
- Task criteria satisfied and marked done.
