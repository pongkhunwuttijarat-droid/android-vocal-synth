//! The wasm host cannot run ONNX engines, by design — this test locks it in.
//!
//! Background: `synth-cli`'s `default = ["ffi", "r2", "parallel"]` turns on the
//! `r2` feature, which is what pulls in `ort` (ONNX Runtime), `diffsinger`,
//! `moe-train` and `burn`. `engine-wasm` depends on `synth-cli` with
//! `default-features = false`, so none of that is compiled into the browser
//! build: WORLDLINE-R2, DiffSinger and MoE are **not** renderable by the
//! wasm engine. This is a deliberate architecture boundary (browser has no
//! dlopen, no thread pool, and `ort` does not build for
//! `wasm32-unknown-unknown`), recorded in
//! `docs/architecture/known-limitations.md`.
//!
//! If someone re-enables `r2` for wasm "just to try", the capability the UI
//! advertises and the audio it can actually produce stop matching. These tests
//! fail first.
//!
//! Source of truth for the facts below is the manifests themselves, read at
//! test time — not a copy pasted into this file.

use std::path::{Path, PathBuf};

fn engine_wasm_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read(relative: &str) -> String {
    let path = engine_wasm_dir().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Direct dependency names in `engine-wasm/Cargo.toml` `[dependencies]`.
fn engine_wasm_dependency_names() -> Vec<String> {
    let manifest = read("Cargo.toml");
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]" || line == "[dev-dependencies]";
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `name = { ... }` or `name = "version"` (no `optional`/`workspace` here).
        if let Some((name, _)) = line.split_once('=') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            names.push(name.to_string());
        }
    }
    assert!(
        !names.is_empty(),
        "no dependencies parsed from engine-wasm/Cargo.toml — the parser drifted"
    );
    names
}

#[test]
fn engine_wasm_declares_no_onnx_runtime() {
    let deps = engine_wasm_dependency_names();
    for banned in [
        "ort",
        "onnx",
        "onnxruntime",
        "diffsinger",
        "moe-train",
        "burn",
        "tract",
    ] {
        assert!(
            !deps.iter().any(|d| d == banned),
            "engine-wasm must not depend on '{banned}': the wasm host has no ONNX \
             runtime by design. Found in deps: {deps:?}. If ONNX in the browser is \
             wanted, that is an architecture decision, not a Cargo edit — see \
             docs/architecture/known-limitations.md (L1)."
        );
    }
}

#[test]
fn synth_cli_is_depended_on_with_default_features_off() {
    let manifest = read("Cargo.toml");
    let line = manifest
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("synth-cli"))
        .unwrap_or_else(|| {
            panic!("no synth-cli dependency line in engine-wasm/Cargo.toml; deps are {manifest}")
        });
    assert!(
        line.contains("default-features = false"),
        "synth-cli must be pulled in with `default-features = false`, otherwise the \
         `r2` feature re-enables ort/burn inside the wasm build. Found: {line}"
    );
}

#[test]
fn synth_cli_r2_feature_is_what_gates_the_onnx_engines() {
    // Guards the explanation itself: if `r2` stops being the feature that pulls
    // in ort/diffsinger/moe-train/burn, this doc-and-test pair is stale and the
    // next reader would be misled about why wasm has no ONNX.
    let manifest = read("../../tools/synth-cli/Cargo.toml");
    let r2 = manifest
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("r2 = ["))
        .unwrap_or_else(|| panic!("no `r2 = [...]` feature line in synth-cli/Cargo.toml"));
    for dep in ["dep:ort", "dep:diffsinger", "dep:moe-train", "dep:burn"] {
        assert!(
            r2.contains(dep),
            "expected the `r2` feature to gate {dep} (the known-limitation note and \
             this test both assert it). Actual: {r2}"
        );
    }
}

/// The wasm engine can still do the *feature extraction* half of the R2 chain
/// (pure-Rust WORLD kernel) — what it cannot do is inference. This pins the
/// narrow claim so the limitation cannot be quoted as "R2 support is absent"
/// (too strong) or "R2 works in the browser" (too weak).
#[test]
fn wasm_engine_exposes_r2_feature_extraction_but_not_inference() {
    let src = read("src/lib.rs");
    assert!(
        src.contains("fn render_r2_features_chunk"),
        "engine-wasm should keep exposing render_r2_features_chunk (Rust-side WORLD \
         feature extraction for an ORT-Web worker)"
    );
    // No engine type in the wasm build may perform ONNX inference: there is no
    // ort session constructor anywhere in engine-wasm's own source.
    for banned in ["Session::builder", "ort::", "onnx"] {
        assert!(
            !src.contains(banned),
            "engine-wasm source must not construct an ONNX session ({banned})"
        );
    }
}
