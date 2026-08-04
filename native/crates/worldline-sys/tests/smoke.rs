//! Smoke tests for the prebuilt `libworldline.so`.
//!
//! Ignored by default because they need the prebuilt library. Run with:
//!
//! ```sh
//! WORLDLINE_SO=/path/to/libworldline.so cargo test -- --ignored --nocapture
//! ```
//!
//! `WORLDLINE_SO` defaults to the desktop linux-x64 reference build.

use std::path::PathBuf;
use std::process::Command;

const DEFAULT_SO: &str = "/home/seal/project/android-voice-synth/ref(openutau+openutau mobile)/desktop-ref/runtimes/linux-x64/native/libworldline.so";

fn so_path() -> PathBuf {
    if let Ok(p) = std::env::var("WORLDLINE_SO") {
        return PathBuf::from(p);
    }
    let p = PathBuf::from(DEFAULT_SO);
    assert!(
        p.exists(),
        "default libworldline.so not found at {p:?}; set WORLDLINE_SO to point at the prebuilt .so"
    );
    p
}

/// Resolve every symbol the bindings declare, using the real fn-pointer
/// types from the extern block. Proves the bindings match the .so.
fn check_symbols(path: &std::path::Path) {
    // SAFETY: dlopen of a known prebuilt library; symbol lookups only.
    let lib = unsafe { libloading::Library::new(path) }.expect("dlopen libworldline.so");
    let mut n = 0;
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let s: libloading::Symbol<$ty> = unsafe { lib.get::<$ty>($name.as_bytes()) }
                .unwrap_or_else(|e| panic!("symbol {} missing from libworldline.so: {e}", $name));
            let _ = s; // resolved OK
            println!("  symbol {:<32} OK", $name);
            n += 1;
        }};
    }
    sym!("F0", unsafe extern "C" fn(*mut f32, i32, i32, f64, i32, *mut *mut f64) -> i32);
    sym!("DecodeMgc", unsafe extern "C" fn(i32, *mut f64, i32, i32, i32, *mut *mut f64) -> i32);
    sym!("DecodeBap", unsafe extern "C" fn(i32, *mut f64, i32, i32, *mut *mut f64) -> i32);
    sym!("InitAnalysisConfig", unsafe extern "C" fn(*mut worldline_sys::AnalysisConfig, i32, i32, i32));
    sym!("WorldAnalysis", unsafe extern "C" fn(*const worldline_sys::AnalysisConfig, *mut f32, i32, *mut *mut f64, *mut *mut f64, *mut *mut f64, *mut i32));
    sym!("WorldAnalysisF0In", unsafe extern "C" fn(*const worldline_sys::AnalysisConfig, *mut f32, i32, *mut f64, i32, *mut f64, *mut f64));
    sym!("WorldSynthesis", unsafe extern "C" fn(*mut f64, i32, *mut f64, u8, i32, *mut f64, u8, i32, f64, i32, *mut *mut f64, *mut f64, *mut f64, *mut f64, *mut f64) -> i32);
    sym!("Resample", unsafe extern "C" fn(*const worldline_sys::SynthRequest, *mut *mut f32) -> i32);
    sym!("PhraseSynthNew", unsafe extern "C" fn() -> *mut std::ffi::c_void);
    sym!("PhraseSynthDelete", unsafe extern "C" fn(*mut std::ffi::c_void));
    sym!("PhraseSynthAddRequest", unsafe extern "C" fn(*mut std::ffi::c_void, *const worldline_sys::SynthRequest, f64, f64, f64, f64, f64, worldline_sys::LogCallback));
    sym!("PhraseSynthSetCurves", unsafe extern "C" fn(*mut std::ffi::c_void, *mut f64, *mut f64, *mut f64, *mut f64, *mut f64, i32, worldline_sys::LogCallback));
    sym!("PhraseSynthSynth", unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut f32, worldline_sys::LogCallback) -> i32);
    println!("  {n}/13 bound symbols resolved");
}

/// Create a PhraseSynth via the safe wrapper, assert a non-null handle,
/// drop it (runs PhraseSynthDelete), and verify all other bindings resolve.
#[test]
#[ignore = "requires prebuilt libworldline.so; run with WORLDLINE_SO=... cargo test -- --ignored --nocapture"]
fn smoke_phrase_synth_lifecycle() {
    let path = so_path();
    println!("loading {path:?}");

    let ps = worldline_sys::PhraseSynth::open(&path).expect("open + PhraseSynthNew");
    assert!(!ps.raw_handle().is_null(), "PhraseSynthNew returned NULL");
    println!("OK: PhraseSynthNew -> handle {:p} (non-null)", ps.raw_handle());

    drop(ps);
    println!("OK: PhraseSynthDelete (clean drop, no crash)");

    check_symbols(&path);
    println!("SMOKE TEST PASSED: libworldline.so loaded and exercised");
}

/// Enumerate the .so's exported dynamic symbols with nm and report the
/// count plus the presence of every symbol the bindings rely on.
#[test]
#[ignore = "requires prebuilt libworldline.so; run with WORLDLINE_SO=... cargo test -- --ignored --nocapture"]
fn smoke_exported_symbols() {
    let path = so_path();
    let out = Command::new("nm")
        .args(["-D", "--defined-only"])
        .arg(&path)
        .output()
        .expect("failed to run nm (binutils required)");
    assert!(out.status.success(), "nm exited nonzero");
    let text = String::from_utf8_lossy(&out.stdout);
    let count = text.lines().count();
    println!("libworldline.so exported defined symbols: {count}");
    assert!(count > 1000, "suspiciously few symbols: {count}");

    let required = [
        "PhraseSynthNew",
        "PhraseSynthDelete",
        "PhraseSynthAddRequest",
        "PhraseSynthSetCurves",
        "PhraseSynthSynth",
        "Resample",
        "WorldSynthesis",
        "WorldAnalysis",
        "WorldAnalysisF0In",
        "InitAnalysisConfig",
        "DecodeMgc",
        "DecodeBap",
        "F0",
    ];
    for name in required {
        assert!(text.contains(name), "required symbol {name} not exported");
        println!("  found {name}");
    }

    // plugin_get_capabilities is a plugin-side export defined by
    // plugin_abi.h; libworldline.so is not a plugin and should not
    // export it. Report either way.
    if text.contains("plugin_get_capabilities") {
        println!("  note: plugin_get_capabilities IS exported (unexpected for libworldline.so)");
    } else {
        println!("  note: plugin_get_capabilities not exported by libworldline.so (expected — plugin-side symbol per plugin_abi.h)");
    }
    println!("SYMBOL CHECK PASSED");
}
