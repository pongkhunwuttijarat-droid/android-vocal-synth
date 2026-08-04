//! Mixer FX plugin — loads `libmixerfx.so` (dlopen) and runs the FX chain
//! on the final mixed samples. Mirrors the worldline plugin pattern
//! (FFI .so + wrapper). The chain lives in C++ (gain -> 3-band EQ ->
//! compressor -> soft clip); Rust only passes samples through.
//!
//! POC contract (see `plugins/mixer-fx/mixer_fx.cpp`):
//! ```c
//! void* MxFxCreate(const MxFxConfig* cfg, const char* params_json);
//! void  MxFxProcess(void* fx, float* samples, int n, double pos_ms);
//! void  MxFxDestroy(void* fx);
//! ```

use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use std::ptr;

use libloading::{Library, Symbol};

/// Mirrors `struct MxFxConfig` in mixer_fx.cpp (layout is C-compatible).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MxFxConfig {
    pub sample_rate: f64,
    pub channels: i32,
}

/// dlopen'd mixer plugin handle (keeps the `Library` alive).
pub struct MixerFx {
    _lib: Library,
    handle: *mut c_void,
}

impl std::fmt::Debug for MixerFx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MixerFx")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

unsafe impl Send for MixerFx {}

type CreateFn = unsafe extern "C" fn(*const MxFxConfig, *const c_char) -> *mut c_void;
type ProcessFn = unsafe extern "C" fn(*mut c_void, *mut f32, i32, f64);
type DestroyFn = unsafe extern "C" fn(*mut c_void);

impl MixerFx {
    /// dlopen `so_path` and create an FX instance with `params_json`
    /// (e.g. `{"gain":1.0,"clip_enabled":1}` — see mixer_fx.cpp keys).
    pub fn open(so_path: impl AsRef<Path>, params_json: &str) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(so_path.as_ref())
                .map_err(|e| format!("dlopen {}: {e}", so_path.as_ref().display()))?;
            let create: Symbol<CreateFn> = lib
                .get(b"MxFxCreate\0")
                .map_err(|e| format!("symbol MxFxCreate: {e}"))?;
            let cfg = MxFxConfig {
                sample_rate: 44100.0,
                channels: 1,
            };
            let params = CString::new(params_json).map_err(|e| format!("params: {e}"))?;
            let handle = create(&cfg, params.as_ptr());
            if handle.is_null() {
                return Err("MxFxCreate returned null".to_string());
            }
            Ok(MixerFx { _lib: lib, handle })
        }
    }

    /// Run the FX chain in place. `pos_ms` is the chunk position (unused
    /// by the POC chain, reserved for time-based effects).
    pub fn process(&mut self, samples: &mut [f32], pos_ms: f64) -> Result<(), String> {
        unsafe {
            let process: Symbol<ProcessFn> = self
                ._lib
                .get(b"MxFxProcess\0")
                .map_err(|e| format!("symbol MxFxProcess: {e}"))?;
            if !samples.is_empty() {
                process(self.handle, samples.as_mut_ptr(), samples.len() as i32, pos_ms);
            }
            Ok(())
        }
    }
}

impl Drop for MixerFx {
    fn drop(&mut self) {
        unsafe {
            if let Ok(destroy) = self._lib.get::<DestroyFn>(b"MxFxDestroy\0") {
                destroy(self.handle);
            }
            self.handle = ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// dlopen failure is reported cleanly (no panic).
    #[test]
    fn open_missing_so_errors() {
        let err = MixerFx::open("/nonexistent/libmixerfx.so", "").unwrap_err();
        assert!(err.contains("dlopen"), "unexpected error: {err}");
        // err is String — no Debug bound needed on MixerFx itself.
        let _ = err;
    }
}
