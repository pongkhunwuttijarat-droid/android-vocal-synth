# worldline-sys

Raw FFI bindings to the **worldline** voice synthesis library
(`libworldline.so`), plus a small safe wrapper around the `PhraseSynth`
object API.

## Layout

```
src/lib.rs        extern "C" bindings (13 symbols) + PhraseSynth wrapper
tests/smoke.rs    #[ignore]d smoke tests against the prebuilt .so
```

## Usage

```rust
use worldline_sys::{PhraseSynth, SynthRequest};

let ps = PhraseSynth::open("/path/to/libworldline.so")?;
ps.add_request(&req, pos_ms, skip_ms, length_ms, fade_in_ms, fade_out_ms);
ps.set_curves(&f0, Some(&gender), None, None, None);
let samples: Vec<f32> = ps.synth(); // mono f32, ready for playback
```

The library is dlopen'd at runtime (`libloading`) — **no link-time
dependency**, so the same crate works against both the linux-x64 desktop
build and the android arm64-v8a build. Locating the `.so` (env var, asset
path, ...) is the host's job.

## Bindings

Mirrors `worldline.h` + `synth_request.h` exactly:

| C symbol | Rust binding | Notes |
|---|---|---|
| `PhraseSynthNew` / `PhraseSynthDelete` | `PhraseSynth::open` / `Drop` | handle never NULL after open |
| `PhraseSynthAddRequest` | `PhraseSynth::add_request` | request copied during the call |
| `PhraseSynthSetCurves` | `PhraseSynth::set_curves` | 5 parallel double arrays, 1 value / 10 ms frame |
| `PhraseSynthSynth` | `PhraseSynth::synth` | returns sample count; `float* y` copied to `Vec<f32>` and freed |
| `Resample` | raw `Resample` | single-request resampling |
| `F0` / `DecodeMgc` / `DecodeBap` / `InitAnalysisConfig` / `WorldAnalysis` / `WorldAnalysisF0In` / `WorldSynthesis` | raw | full WORLD analysis/synthesis |

## ABI notes (verified against the prebuilt .so)

* **Types**: `int32_t` → `i32`, `double` → `f64`, `char*` → `*mut c_char`.
  `SynthRequest` layout is asserted by unit test: **144 bytes, align 8**
  (6×i32 + 8×f64 + 2×ptr + 6×i32).
* `SynthRequest.sample` is `double*` in the header — **not** `float*`.
* `WorldSynthesis(is_mgc, is_bap)` takes C++ `bool`s; exposed as `u8`
  (C ABI-safe, 1 byte each).
* **Buffer ownership**: output buffers are allocated inside the library
  with `new float[]`. On Linux (glibc) and Android (Bionic) `operator new`
  routes to `malloc`, so `worldline_sys::free_buffer` (libc `free`) is the
  matching deallocator. The library exports no dedicated free function.
  `PhraseSynth::synth` copies and frees internally; raw callers must call
  `free_buffer` themselves.
* **Return values**: `PhraseSynthSynth` and `Resample` return the sample
  count as their `int` return value.
* **Logging**: `LogCallback` (`Option<unsafe extern "C" fn(*const c_char)>`)
  is supported by the raw bindings; the safe wrapper passes `None`.
* **SoundCurve**: no such struct exists in the shipped headers
  (`worldline.h`, `synth_request.h`, `phrase_synth.h`) — the curve API is
  the five parallel arrays of `PhraseSynthSetCurves`.
* **Threading**: `PhraseSynth` is `!Send + !Sync`; one object per thread.
* Verified symbols on the linux-x64 prebuilt: `PhraseSynth*` ×5,
  `Resample`, `World*`, `F0`, `Decode*`, `InitAnalysisConfig` — all
  present (also on the android arm64-v8a build). 1594 exported defined
  symbols total.

## Smoke tests

Ignored by default (need the prebuilt library):

```sh
WORLDLINE_SO=/path/to/libworldline.so cargo test -- --ignored --nocapture
```

Defaults to the desktop reference build:
`ref(openutau+openutau mobile)/desktop-ref/runtimes/linux-x64/native/libworldline.so`.
