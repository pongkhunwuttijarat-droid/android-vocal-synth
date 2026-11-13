//! wasm32 smoke artifact (T2).
//!
//! Declared with crate-type = ["cdylib"] in Cargo.toml, so:
//!
//!   cargo build -p moe-train --example moe_wasm_smoke \
//!     --target wasm32-unknown-unknown --no-default-features
//!
//! emits a REAL .wasm and proves the MoE acoustic forward (burn-ndarray)
//! codegens and links for wasm32-unknown-unknown. It is a probe, not a
//! finished wasm API: there is no wasm-bindgen glue and no vocoder
//! (the ort path is excluded by the feature gate).

use burn::tensor::{Int, Tensor, TensorData};
use moe_train::baseline::ContextAcoustic;

type B = burn::backend::NdArray;

/// Builds a tiny MoE acoustic, runs one forward pass and returns the RMS of
/// the 128-bin mel. If this links, the whole burn/ndarray op chain the MoE
/// engine needs is available on wasm32.
#[no_mangle]
pub extern "C" fn moe_forward_probe() -> f32 {
    let device = Default::default();
    let window = moe_train::baseline::WINDOW;
    let m = ContextAcoustic::<B>::new(0, 64, 1, &device);
    let ids = Tensor::<B, 2, Int>::from_data(
        TensorData::new(vec![1i64; window], vec![1, window]),
        &device,
    );
    let feats = Tensor::<B, 2>::from_data(TensorData::new(vec![0.0f32, 0.0], vec![1, 2]), &device);
    let out = m.forward(ids, feats);
    let v: Vec<f32> = out
        .into_data()
        .convert::<f32>()
        .to_vec::<f32>()
        .unwrap_or_default();
    (v.iter().map(|x| x * x).sum::<f32>()).sqrt()
}
