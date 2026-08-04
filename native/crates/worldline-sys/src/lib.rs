//! Raw FFI bindings to the worldline synthesis library (`libworldline.so`),
//! plus a small safe wrapper around the `PhraseSynth` object API.
//!
//! # Loading model
//!
//! The library is **dlopen'd at runtime** via [`libloading`] — there is no
//! link-time dependency on the `.so`. The same bindings therefore work with
//! both the desktop build (`linux-x64`) and the Android build (`arm64-v8a`).
//! The host is responsible for locating the library file (env var, asset
//! path, ...); see the `PhraseSynth::open` docs and `tests/smoke.rs`.
//!
//! # ABI notes
//!
//! * Types mirror `worldline.h` / `synth_request.h` exactly:
//!   `int32_t` → `i32`, `double` → `f64`, `char*` → `*mut c_char`.
//! * `WorldSynthesis` takes two C++ `bool` parameters (`is_mgc`, `is_bap`).
//!   C++ `bool` is 1 byte; we expose them as `u8` (0/1) to keep the
//!   bindings C-ABI-safe.
//! * Output buffers (`float** y`) are allocated inside the library with
//!   `new float[]`. On Linux (glibc) and Android (Bionic) `operator new`
//!   routes to `malloc`, so [`free_buffer`] (libc `free`) is the matching
//!   deallocator. The library exports no dedicated free function.
//! * `PhraseSynthSynth` returns the sample count as its `int` return value.
//! * There is no `SoundCurve` struct in the shipped headers
//!   (`worldline.h`, `synth_request.h`, `phrase_synth.h`); the curves API
//!   is the five parallel arrays of `PhraseSynthSetCurves`.
//!
//! # Safety
//!
//! All `extern "C"` items are `unsafe` as usual for FFI. The
//! [`PhraseSynth`] wrapper encapsulates the unsafe parts: it owns the
//! `Library` (keeping the `.so` alive), calls `PhraseSynthDelete` on drop,
//! and is `!Send + !Sync` (the C++ object must not be shared across
//! threads).

#![allow(non_camel_case_types)]

use libloading::{Library, Symbol};
use std::error::Error as StdError;
use std::ffi::{c_char, c_void};
use std::fmt;
use std::path::Path;

// ---------------------------------------------------------------------------
// Types (mirror worldline.h / synth_request.h)
// ---------------------------------------------------------------------------

/// Log callback the library invokes with diagnostic strings.
///
/// Matches `worldline::LogCallback` (`typedef void(*)(const char*)`).
/// `None` (NULL) disables logging.
pub type LogCallback = Option<unsafe extern "C" fn(log: *const c_char)>;

/// `struct SynthRequest` from `synth_request.h`.
///
/// Layout verified by the `synth_request_layout` unit test:
/// size 144, align 8 (see test below).
// Field names keep the exact utau flag spelling from the C header
// (flag_g/flag_O/...), hence `non_snake_case`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case)]
pub struct SynthRequest {
    pub sample_fs: i32,
    pub sample_length: i32,
    /// Input waveform samples (note: `double*` in the header, not `float*`).
    pub sample: *mut f64,
    /// Length of `frq` in bytes; 0 = no frq data (auto pitch estimate).
    pub frq_length: i32,
    /// Raw .frq file bytes.
    pub frq: *mut c_char,
    /// MIDI note number (0..127).
    pub tone: i32,
    /// Consonant velocity (0..1).
    pub con_vel: f64,
    /// Offset into the source sample, ms.
    pub offset: f64,
    /// Required rendered length, ms.
    pub required_length: f64,
    /// Consonant position, ms.
    pub consonant: f64,
    /// Cut-off, ms.
    pub cut_off: f64,
    /// Volume (1.0 = unity).
    pub volume: f64,
    /// Modulation (vibrato).
    pub modulation: f64,
    /// Tempo multiplier.
    pub tempo: f64,
    /// Length of `pitch_bend`; 0 = none.
    pub pitch_bend_length: i32,
    /// Pitch bend curve, cents-per-... values.
    pub pitch_bend: *mut i32,
    // flags: utau-style g-O-P and Mt/Mb/Mv toggles
    pub flag_g: i32,
    pub flag_O: i32,
    pub flag_P: i32,
    pub flag_Mt: i32,
    pub flag_Mb: i32,
    pub flag_Mv: i32,
}

impl Default for SynthRequest {
    /// Mirrors the C++ default member initializers: `frq_length = 0`,
    /// `frq = nullptr`, `pitch_bend_length = 0`, `pitch_bend = nullptr`,
    /// everything else zeroed.
    fn default() -> Self {
        SynthRequest {
            sample_fs: 0,
            sample_length: 0,
            sample: std::ptr::null_mut(),
            frq_length: 0,
            frq: std::ptr::null_mut(),
            tone: 0,
            con_vel: 0.0,
            offset: 0.0,
            required_length: 0.0,
            consonant: 0.0,
            cut_off: 0.0,
            volume: 1.0,
            modulation: 0.0,
            tempo: 1.0,
            pitch_bend_length: 0,
            pitch_bend: std::ptr::null_mut(),
            flag_g: 0,
            flag_O: 0,
            flag_P: 0,
            flag_Mt: 0,
            flag_Mb: 0,
            flag_Mv: 0,
        }
    }
}

/// `struct AnalysisConfig` from `worldline.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AnalysisConfig {
    pub fs: i32,
    pub hop_size: i32,
    pub fft_size: i32,
    pub f0_floor: f32,
    pub frame_ms: f64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        AnalysisConfig {
            fs: 44100,
            hop_size: 0,
            fft_size: 0,
            f0_floor: 71.0,
            frame_ms: 5.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Raw extern "C" bindings (symbols verified against the linux-x64 and
// arm64-v8a prebuilt libworldline.so via `nm -D`).
// ---------------------------------------------------------------------------

extern "C" {
    /// Estimate F0 from raw samples. Returns the number of frames;
    /// allocates `*f0` (free with libc `free`).
    pub fn F0(
        samples: *mut f32,
        length: i32,
        fs: i32,
        frame_period: f64,
        method: i32,
        f0: *mut *mut f64,
    ) -> i32;

    /// Decode MGC coefficients to a spectrogram. Allocates `*spectrogram`.
    pub fn DecodeMgc(
        f0_length: i32,
        mgc: *mut f64,
        mgc_size: i32,
        fft_size: i32,
        fs: i32,
        spectrogram: *mut *mut f64,
    ) -> i32;

    /// Decode BAP coefficients to aperiodicity. Allocates `*aperiodicity`.
    pub fn DecodeBap(
        f0_length: i32,
        bap: *mut f64,
        fft_size: i32,
        fs: i32,
        aperiodicity: *mut *mut f64,
    ) -> i32;

    /// Initialize an `AnalysisConfig` for the given fs/hop/fft.
    pub fn InitAnalysisConfig(config: *mut AnalysisConfig, fs: i32, hop_size: i32, fft_size: i32);

    /// Full WORLD analysis. Allocates `*f0_out`, `*sp_env_out`, `*ap_out`
    /// (each `num_frames` × frame arrays; free with libc `free`).
    pub fn WorldAnalysis(
        config: *const AnalysisConfig,
        samples: *mut f32,
        num_samples: i32,
        f0_out: *mut *mut f64,
        sp_env_out: *mut *mut f64,
        ap_out: *mut *mut f64,
        num_frames: *mut i32,
    );

    /// WORLD analysis with a precomputed F0 curve.
    pub fn WorldAnalysisF0In(
        config: *const AnalysisConfig,
        samples: *mut f32,
        num_samples: i32,
        f0_in: *mut f64,
        num_frames: i32,
        sp_env_out: *mut f64,
        ap_out: *mut f64,
    );

    /// WORLD synthesis. `is_mgc`/`is_bap` are C++ `bool`s -> `u8` (0/1).
    /// Allocates `*y` (free with libc `free`); returns the sample count.
    /// gender/tension/breathiness/voicing are per-frame curves [0..1];
    /// `null` means "use default".
    #[allow(clippy::too_many_arguments)]
    pub fn WorldSynthesis(
        f0: *mut f64,
        f0_length: i32,
        mgc_or_sp: *mut f64,
        is_mgc: u8,
        mgc_size: i32,
        bap_or_ap: *mut f64,
        is_bap: u8,
        fft_size: i32,
        frame_period: f64,
        fs: i32,
        y: *mut *mut f64,
        gender: *mut f64,
        tension: *mut f64,
        breathiness: *mut f64,
        voicing: *mut f64,
    ) -> i32;

    /// Resample one request. Allocates `*y` (free with [`free_buffer`]);
    /// returns the sample count.
    pub fn Resample(request: *const SynthRequest, y: *mut *mut f32) -> i32;

    /// Create a phrase synth. Returns NULL on failure.
    pub fn PhraseSynthNew() -> *mut c_void;

    /// Destroy a phrase synth created by [`PhraseSynthNew`].
    pub fn PhraseSynthDelete(phrase_synth: *mut c_void);

    /// Add one note/sample request to the phrase. `request` is copied
    /// during the call and need not outlive it.
    pub fn PhraseSynthAddRequest(
        phrase_synth: *mut c_void,
        request: *const SynthRequest,
        pos_ms: f64,
        skip_ms: f64,
        length_ms: f64,
        fade_in_ms: f64,
        fade_out_ms: f64,
        log_callback: LogCallback,
    );

    /// Set global expression curves (one value per 10 ms frame).
    /// Each array is `length` doubles; `null` = default curve.
    pub fn PhraseSynthSetCurves(
        phrase_synth: *mut c_void,
        f0: *mut f64,
        gender: *mut f64,
        tension: *mut f64,
        breathiness: *mut f64,
        voicing: *mut f64,
        length: i32,
        log_callback: LogCallback,
    );

    /// Synthesize the phrase. Allocates `*y` (free with [`free_buffer`]);
    /// returns the sample count.
    pub fn PhraseSynthSynth(
        phrase_synth: *mut c_void,
        y: *mut *mut f32,
        log_callback: LogCallback,
    ) -> i32;
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from loading/using the worldline library.
#[derive(Debug)]
pub enum Error {
    /// `dlopen`/`dlsym` failed.
    Library(libloading::Error),
    /// A required exported symbol was not found.
    MissingSymbol(&'static str),
    /// `PhraseSynthNew()` returned NULL.
    NullHandle,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Library(e) => write!(f, "worldline library error: {e}"),
            Error::MissingSymbol(name) => write!(f, "worldline symbol not found: {name}"),
            Error::NullHandle => write!(f, "PhraseSynthNew returned NULL"),
        }
    }
}

impl StdError for Error {}

impl From<libloading::Error> for Error {
    fn from(e: libloading::Error) -> Self {
        Error::Library(e)
    }
}

// ---------------------------------------------------------------------------
// Safe wrapper
// ---------------------------------------------------------------------------

/// Safe wrapper around the C++ `worldline::PhraseSynth` object.
///
/// Owns the dlopen'd `Library` (the `.so` stays loaded for the lifetime of
/// the wrapper) and the raw C++ handle, which is released via
/// `PhraseSynthDelete` on drop. `PhraseSynth` is `!Send + !Sync`: the
/// underlying C++ object must be used from one thread at a time.
pub struct PhraseSynth {
    /// Kept alive for the lifetime of the wrapper (dlclose on drop).
    #[allow(dead_code)]
    lib: Library,
    handle: *mut c_void,
    delete: Symbol<'static, unsafe extern "C" fn(*mut c_void)>,
    add_request: Symbol<'static, unsafe extern "C" fn(*mut c_void, *const SynthRequest, f64, f64, f64, f64, f64, LogCallback)>,
    set_curves: Symbol<'static, unsafe extern "C" fn(*mut c_void, *mut f64, *mut f64, *mut f64, *mut f64, *mut f64, i32, LogCallback)>,
    synth: Symbol<'static, unsafe extern "C" fn(*mut c_void, *mut *mut f32, LogCallback) -> i32>,
}

impl PhraseSynth {
    /// dlopen `path`, resolve every required symbol, and create the C++
    /// object (`PhraseSynthNew`).
    ///
    /// # Safety model
    ///
    /// `open` is the single unsafe entry point: loading an untrusted or
    /// ABI-mismatched `.so` can crash the process. Everything after a
    /// successful `open` is safe to call.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        // SAFETY: dlopen of a user-supplied library — see the doc comment.
        let lib = unsafe { Library::new(path) }?;

        // Resolve a symbol and extend its borrow to 'static. Sound because
        // the Library is moved into the returned struct, so it outlives
        // every Symbol stored alongside it (libloading's documented
        // pattern for owning wrapper structs).
        //
        // SAFETY: see above; the caller must keep `lib` alive.
        unsafe fn sym<T>(
            lib: &Library,
            name: &[u8],
        ) -> Result<Symbol<'static, T>, libloading::Error> {
            let s = lib.get::<T>(name)?;
            // SAFETY: the library outlives this wrapper (caller contract).
            Ok(std::mem::transmute::<Symbol<'_, T>, Symbol<'static, T>>(s))
        }

        // SAFETY: dlsym lookups; failure yields Err (no UB).
        unsafe {
            let new = sym::<unsafe extern "C" fn() -> *mut c_void>(&lib, b"PhraseSynthNew")?;
            let delete = sym::<unsafe extern "C" fn(*mut c_void)>(&lib, b"PhraseSynthDelete")?;
            let add_request = sym::<
                unsafe extern "C" fn(*mut c_void, *const SynthRequest, f64, f64, f64, f64, f64, LogCallback),
            >(&lib, b"PhraseSynthAddRequest")?;
            let set_curves = sym::<
                unsafe extern "C" fn(*mut c_void, *mut f64, *mut f64, *mut f64, *mut f64, *mut f64, i32, LogCallback),
            >(&lib, b"PhraseSynthSetCurves")?;
            let synth = sym::<
                unsafe extern "C" fn(*mut c_void, *mut *mut f32, LogCallback) -> i32,
            >(&lib, b"PhraseSynthSynth")?;

            let handle = new();
            if handle.is_null() {
                return Err(Error::NullHandle);
            }
            Ok(PhraseSynth {
                lib,
                handle,
                delete,
                add_request,
                set_curves,
                synth,
            })
        }
    }

    /// The raw C++ `PhraseSynth*` handle.
    pub fn raw_handle(&self) -> *mut c_void {
        self.handle
    }

    /// Add one request (note) to the phrase. `request` is copied by the
    /// library during the call and may be dropped afterwards.
    ///
    /// `pos_ms`/`skip_ms`/`length_ms`/`fade_in_ms`/`fade_out_ms` place the
    /// request on the phrase timeline (see `worldline.h`).
    pub fn add_request(
        &self,
        request: &SynthRequest,
        pos_ms: f64,
        skip_ms: f64,
        length_ms: f64,
        fade_in_ms: f64,
        fade_out_ms: f64,
    ) {
        // SAFETY: handle is valid; request is a valid reference; callback NULL.
        unsafe {
            (self.add_request)(
                self.handle,
                request as *const SynthRequest,
                pos_ms,
                skip_ms,
                length_ms,
                fade_in_ms,
                fade_out_ms,
                None,
            )
        }
    }

    /// Set the global expression curves (one value per 10 ms frame).
    ///
    /// `f0` must be non-empty; `gender`/`tension`/`breathiness`/`voicing`
    /// may be `None` (default curve) or slices of at least `f0.len()`.
    /// # Panics
    /// Panics if an optional curve is shorter than `f0`.
    pub fn set_curves(
        &self,
        f0: &[f64],
        gender: Option<&[f64]>,
        tension: Option<&[f64]>,
        breathiness: Option<&[f64]>,
        voicing: Option<&[f64]>,
    ) {
        let len = f0.len();
        assert!(len > 0, "set_curves: f0 must be non-empty");
        for (name, curve) in [
            ("gender", gender),
            ("tension", tension),
            ("breathiness", breathiness),
            ("voicing", voicing),
        ] {
            if let Some(c) = curve {
                assert!(
                    c.len() >= len,
                    "set_curves: {name} curve has {} values, need at least {len}",
                    c.len()
                );
            }
        }
        let curve = |slice: Option<&[f64]>| -> *mut f64 {
            match slice {
                Some(s) => s.as_ptr() as *mut f64,
                None => std::ptr::null_mut(),
            }
        };
        // SAFETY: all slices outlive the call; length matches f0.len().
        unsafe {
            (self.set_curves)(
                self.handle,
                f0.as_ptr() as *mut f64,
                curve(gender),
                curve(tension),
                curve(breathiness),
                curve(voicing),
                len as i32,
                None,
            )
        }
    }

    /// Synthesize the phrase. Returns the rendered samples (mono f32).
    ///
    /// The library-allocated buffer is copied into the returned `Vec` and
    /// freed with [`free_buffer`]. A phrase with no requests yields an
    /// empty `Vec`.
    pub fn synth(&self) -> Vec<f32> {
        let mut y: *mut f32 = std::ptr::null_mut();
        // SAFETY: handle valid; y is a valid out-pointer.
        let len = unsafe { (self.synth)(self.handle, &mut y, None) };
        if len <= 0 {
            if !y.is_null() {
                // Paranoia: free whatever was allocated even on len <= 0.
                unsafe { free_buffer(y) };
            }
            return Vec::new();
        }
        let len = len as usize;
        let mut out = Vec::with_capacity(len);
        // SAFETY: the library allocated exactly `len` floats at y.
        unsafe {
            std::ptr::copy_nonoverlapping(y, out.as_mut_ptr(), len);
            free_buffer(y);
            out.set_len(len);
        }
        out
    }

    /// Free a buffer allocated by the library (`new float[]`).
    ///
    /// Safe to call on pointers returned via `PhraseSynthSynth`/`Resample`
    /// (or `raw_handle`-level calls). On Linux/Android `operator new`
    /// routes to `malloc`, so libc `free` matches.
    ///
    /// # Safety
    /// `ptr` must come from this library and must not be freed twice.
    pub unsafe fn free_buffer(ptr: *mut f32) {
        free_buffer(ptr)
    }
}

/// Free a buffer allocated by the library (`new float[]`). See
/// [`PhraseSynth::free_buffer`].
///
/// # Safety
/// `ptr` must come from the worldline library and must not be freed twice.
pub unsafe fn free_buffer(ptr: *mut f32) {
    if !ptr.is_null() {
        // SAFETY: ptr was allocated with operator new[] -> malloc on
        // Linux/Android; libc::free is the matching deallocator.
        unsafe { libc::free(ptr as *mut libc::c_void) };
    }
}

impl Drop for PhraseSynth {
    fn drop(&mut self) {
        // SAFETY: handle is valid (never null after open); the delete
        // symbol is resolved; the Library is still loaded (dropped after
        // this destructor runs).
        unsafe { (self.delete)(self.handle) };
    }
}

// ---------------------------------------------------------------------------
// Layout tests (run with plain `cargo test`)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    /// The `SynthRequest` struct must match the C++ layout exactly.
    /// Hand-computed from synth_request.h: 6 int32 + 8 double + 2 ptr +
    /// 6 int = 144 bytes, align 8.
    #[test]
    fn synth_request_layout() {
        assert_eq!(size_of::<SynthRequest>(), 144);
        assert_eq!(align_of::<SynthRequest>(), 8);

        assert_eq!(offset_of!(SynthRequest, sample_fs), 0);
        assert_eq!(offset_of!(SynthRequest, sample_length), 4);
        assert_eq!(offset_of!(SynthRequest, sample), 8);
        assert_eq!(offset_of!(SynthRequest, frq_length), 16);
        assert_eq!(offset_of!(SynthRequest, frq), 24);
        assert_eq!(offset_of!(SynthRequest, tone), 32);
        assert_eq!(offset_of!(SynthRequest, con_vel), 40);
        assert_eq!(offset_of!(SynthRequest, offset), 48);
        assert_eq!(offset_of!(SynthRequest, required_length), 56);
        assert_eq!(offset_of!(SynthRequest, consonant), 64);
        assert_eq!(offset_of!(SynthRequest, cut_off), 72);
        assert_eq!(offset_of!(SynthRequest, volume), 80);
        assert_eq!(offset_of!(SynthRequest, modulation), 88);
        assert_eq!(offset_of!(SynthRequest, tempo), 96);
        assert_eq!(offset_of!(SynthRequest, pitch_bend_length), 104);
        assert_eq!(offset_of!(SynthRequest, pitch_bend), 112);
        assert_eq!(offset_of!(SynthRequest, flag_g), 120);
        assert_eq!(offset_of!(SynthRequest, flag_O), 124);
        assert_eq!(offset_of!(SynthRequest, flag_P), 128);
        assert_eq!(offset_of!(SynthRequest, flag_Mt), 132);
        assert_eq!(offset_of!(SynthRequest, flag_Mb), 136);
        assert_eq!(offset_of!(SynthRequest, flag_Mv), 140);
    }

    /// AnalysisConfig: int x3 + float + double = 24 bytes, align 8.
    #[test]
    fn analysis_config_layout() {
        assert_eq!(size_of::<AnalysisConfig>(), 24);
        assert_eq!(align_of::<AnalysisConfig>(), 8);
        assert_eq!(offset_of!(AnalysisConfig, fs), 0);
        assert_eq!(offset_of!(AnalysisConfig, hop_size), 4);
        assert_eq!(offset_of!(AnalysisConfig, fft_size), 8);
        assert_eq!(offset_of!(AnalysisConfig, f0_floor), 12);
        assert_eq!(offset_of!(AnalysisConfig, frame_ms), 16);
    }

    /// Defaults mirror the C++ default member initializers.
    #[test]
    fn synth_request_defaults() {
        let r = SynthRequest::default();
        assert_eq!(r.frq_length, 0);
        assert!(r.frq.is_null());
        assert_eq!(r.pitch_bend_length, 0);
        assert!(r.pitch_bend.is_null());
        assert_eq!(r.volume, 1.0);
        assert_eq!(r.tempo, 1.0);
    }
}
