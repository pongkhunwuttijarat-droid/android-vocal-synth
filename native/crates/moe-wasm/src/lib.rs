//! moe-wasm — run the burn MoE acoustic forward **inside wasm32** (task-4).
//!
//! The engine lives in `engine.rs` and is shared with the native reference binary,
//! so the browser run and the native run execute the exact same code on the exact
//! same checkpoint; only the host differs.
//!
//! Build (see web/run-moe.sh):
//!   cargo build -p moe-wasm --target wasm32-unknown-unknown --release --lib
//!   wasm-bindgen --target web --out-dir native/crates/moe-wasm/www/pkg \
//!     target/wasm32-unknown-unknown/release/moe_wasm.wasm

pub mod engine;

pub use engine::{
    max_abs_diff, stats, synth_feats, synth_feats_alt, synth_ids, synth_ids_alt, Engine, Stats, B,
};

#[cfg(target_arch = "wasm32")]
mod wasm_api {
    use super::engine::Engine;
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    thread_local! {
        static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
    }

    /// Load a burn NamedMpk checkpoint (`pc-nsf-hifigan.mpk` style / `test/poc/bigset/*.mpk`)
    /// from raw bytes. The bytes are copied into wasm linear memory by the wasm-bindgen glue.
    #[wasm_bindgen]
    pub fn load_checkpoint(bytes: Vec<u8>) -> Result<(), JsValue> {
        let e = Engine::from_mpk_bytes(bytes).map_err(|s| JsValue::from_str(&s))?;
        ENGINE.with(|c| *c.borrow_mut() = Some(e));
        Ok(())
    }

    /// One MoE acoustic forward pass: `ids` = rows*9 phoneme ids, `feats` = rows*2
    /// (f0_norm, pos_frac) -> mel Float32Array of rows*128 values.
    #[wasm_bindgen]
    pub fn forward(ids: Vec<i32>, feats: Vec<f32>, rows: usize) -> Result<Vec<f32>, JsValue> {
        let ids: Vec<i64> = ids.into_iter().map(|v| v as i64).collect();
        ENGINE.with(|c| {
            let b = c.borrow();
            let e = b
                .as_ref()
                .ok_or_else(|| JsValue::from_str("load_checkpoint() has not been called"))?;
            e.forward(&ids, &feats, rows)
                .map_err(|s| JsValue::from_str(&s))
        })
    }

    /// Number of parameter elements that `load_checkpoint` actually materialised
    /// inside this wasm instance (must equal the checkpoint element count).
    #[wasm_bindgen]
    pub fn loaded_param_count() -> Result<usize, JsValue> {
        ENGINE.with(|c| {
            let b = c.borrow();
            let e = b
                .as_ref()
                .ok_or_else(|| JsValue::from_str("load_checkpoint() has not been called"))?;
            Ok(e.param_count())
        })
    }

    /// Current size of the wasm linear memory in bytes (RAM evidence, not a guess).
    #[wasm_bindgen]
    pub fn wasm_memory_bytes() -> usize {
        core::arch::wasm32::memory_size::<0>() * 65536
    }

    // The synthetic input generators are exported so the browser and the native
    // reference binary cannot drift apart: both call this same Rust code.
    #[wasm_bindgen]
    pub fn wasm_synth_ids(rows: usize) -> Vec<i32> {
        super::engine::synth_ids(rows)
            .into_iter()
            .map(|v| v as i32)
            .collect()
    }

    #[wasm_bindgen]
    pub fn wasm_synth_feats(rows: usize) -> Vec<f32> {
        super::engine::synth_feats(rows)
    }

    #[wasm_bindgen]
    pub fn wasm_synth_ids_alt(rows: usize) -> Vec<i32> {
        super::engine::synth_ids_alt(rows)
            .into_iter()
            .map(|v| v as i32)
            .collect()
    }

    #[wasm_bindgen]
    pub fn wasm_synth_feats_alt(rows: usize) -> Vec<f32> {
        super::engine::synth_feats_alt(rows)
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_api::*;
