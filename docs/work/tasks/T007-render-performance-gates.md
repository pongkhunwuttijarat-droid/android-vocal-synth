# T007 — Complete render performance device and stress gates

**Status:** stale — Android target-device acceptance is deferred by the user; the missing device/SDK and workload matrix remain resume conditions
**Owner:** unassigned (Codex completed read-only audit)
**Last updated (UTC):** 2026-10-15T01:49:06Z

## Goal

Close the remaining device and stress acceptance checks for render latency,
memory use, cache behavior, and workload scaling.

## Completion criteria

- [ ] Confirm current performance targets and the device matrix.
- [ ] Run representative short and long projects on target devices, including cold/warm cache cases.
- [ ] Measure peak memory and stress behavior under documented concurrency/workload.
- [ ] Save reproducible commands and results in a durable report and update the roadmap.

## Source planning

- [Roadmap MS3.5](../../planning/roadmap/ROADMAP.md)
- [Engine vs OpenUtau performance report](../../planning/engine-vs-openutau-performance.md)

## Repository checkpoint

- Branch: `wasm-native-rust`; base commit: `e6af244abbf04d4a490fca8483295ed6418e6cad`.
- Inspected artifacts: `docs/planning/engine-vs-openutau-performance.md`, `docs/architecture/wasm-performance.md`, and `docs/planning/milestones/ms3/MS.md`. Existing scripts/source files are dirty or untracked with ownership unknown; no benchmark or product file was modified.
- Owned documentation deltas: this T007 record and the T007 board row; untracked in this checkout.
- Durable references: [MS3](../../planning/milestones/ms3/MS.md), [engine/OpenUtau performance report](../../planning/engine-vs-openutau-performance.md), and [WASM performance report](../../architecture/wasm-performance.md).

## Progress

- 2026-09-30: Added from roadmap notes that desktop benchmarks/cache checks had evidence while device and stress gates remained. Verify the newer 2026-09-29 performance report and current target criteria.
- 2026-10-14T10:37:48Z Reviewed the 2026-09-29 engine/OpenUtau benchmark and MS3 targets. Evidence is Linux x86 desktop only; the report explicitly excludes Android. MS3 still lists Mi Pad measurements and a 30-minute stability run as open, with provisional targets of <1 s per note and <1 GB RAM. The inspected docs do not define a current device/workload/concurrency matrix or provide Android peak-memory/stress evidence. No benchmark was run.
- 2026-10-14T11:13:47Z Rechecked the target environment after the user moved from Flutter toward Android WebView + local Rust server. `adb devices -l` reports no attached devices; `rustup target list --installed` contains only `wasm32-unknown-unknown` and `x86_64-unknown-linux-gnu`, so this checkout cannot build the Rust Android target as configured. The acceptance target remains the Android server/backend path, independent of whether its UI is Flutter or WebView. No benchmark was run. The current evidence still has only Linux desktop measurements; MS3's `<1 s/note`, `<1 GB`, and 30-minute run remain documented targets, but device/workload/concurrency acceptance has not been confirmed by the user.
- 2026-10-14T11:28:50Z Rechecked for a local emulator/toolchain alternative: `command -v emulator`, `sdkmanager`, and `cargo-ndk` return no executable; `emulator -list-avds` returns no AVDs, and `/opt/android-sdk`, `/usr/lib/android-sdk`, and `$HOME/Android/Sdk` are absent. Together with the empty `adb devices -l` and only Linux/WASM Rust targets, this environment cannot supply an Android device run or local emulator run. No benchmarks were run.
- 2026-10-14T12:22:22Z Revalidated the Android gate while checking the T033 wrapper bridge: `/usr/bin/adb` is now present, but `adb devices -l` still lists no devices; `sdkmanager` and Android SDK paths are absent, and `flutter doctor -v` reports “Unable to locate Android SDK.” `app/android` has no Gradle wrapper. No build, benchmark, or device run was possible; the external blocker remains the SDK plus an Android target/emulator and confirmed workload/threshold matrix.
- 2026-10-14T12:38:45Z Rechecked with current tools: `adb devices -l` remains empty; `flutter doctor -v` reports Flutter 3.47.4 and no Android SDK; `rustup target list --installed` remains Linux x86_64 + `wasm32-unknown-unknown`; `sdkmanager`, `emulator`, and `app/android/gradlew` remain absent. Linux desktop toolchain is available, but it cannot satisfy Android target-device acceptance. No build or benchmark was run. Device/SDK and workload/threshold confirmation remain external blockers.

## Verification

- Read-only review of `docs/planning/engine-vs-openutau-performance.md`, `docs/architecture/wasm-performance.md`, and `docs/planning/milestones/ms3/MS.md`.
- `adb devices -l` — no devices attached (`adb` exists at `/usr/bin/adb`). `rustup target list --installed` — WASM and Linux x86_64 only; no Android Rust target installed. Latest `flutter doctor -v` (Flutter 3.47.4) says Android SDK is not found; `sdkmanager`, emulator, and `app/android/gradlew` are unavailable.
- No benchmarks or device tests run.

Repeatable read-only target/evidence inventory command from repository root:

```sh
rg -n "1 note|1s|1 s|1 GB|30.?min|Mi Pad|Android|stress" docs/planning/milestones/ms3/MS.md docs/planning/engine-vs-openutau-performance.md docs/architecture/wasm-performance.md
```

Outcome: MS3 contains the provisional <1 s/<1 GB and 30-minute goals; the performance reports describe Linux desktop and explicitly leave Android/Mi Pad unmeasured.

## Resume blockers and dependencies

- Requires an Android SDK/NDK build environment and target Android device(s) or a provisioned emulator, a stable short/long project and bank matrix, defined cold/warm cache and concurrency cases, and confirmation that MS3's <1 s / <1 GB / 30-minute targets remain current. The WebView/local-server architecture does not remove this device gate; it changes which native server binary must be exercised.

## Next step

Do not run Android device acceptance while Android work is deferred. When the user resumes that target, provide an Android SDK/NDK environment and target device or provision an emulator, then confirm the workload matrix and whether MS3's provisional targets remain accepted. Build the WebView + local-server target and run reproducible cold/warm, peak-memory, concurrency and 30-minute stability cases.

- 2026-10-15T01:49:06Z Reclassified this item from `blocked` to `stale` to reflect the user's explicit direction that Android work is not to start yet. The Android SDK/device, target matrix, and MS3 threshold decisions remain required when this acceptance gate is resumed; Linux/WASM measurements are not substitutes. No benchmark/build was run.

## Handoff notes

Keep benchmark inputs, tool versions, device details, commands, and output artifacts together so another harness can reproduce the measurements.
