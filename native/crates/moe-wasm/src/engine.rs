//! Shared MoE forward engine used by BOTH the wasm module (src/lib.rs) and the
//! native reference binary (src/bin/moe_ref.rs), so their outputs can be compared.
//!
//! The checkpoint is loaded **from memory** with `NamedMpkBytesRecorder`
//! (LoadArgs = Vec<u8>) - the file-path recorder cannot work on wasm32.

use burn::backend::NdArray;
use burn::module::Module;
use burn::record::{FullPrecisionSettings, NamedMpkBytesRecorder, Recorder};
use burn::tensor::{Int, Tensor, TensorData};
use moe_train::baseline::{ContextAcoustic, WINDOW};

pub type B = NdArray;

pub struct Engine {
    model: ContextAcoustic<B>,
}

impl Engine {
    /// Load a burn NamedMpk checkpoint from raw bytes.
    ///
    /// `hidden`/`depth` of the dummy only seed the shapes: burn 0.21 replaces
    /// every parameter with the tensor found in the file (it does not validate
    /// shapes), so the file decides the real sizes. What MUST match is
    /// `WINDOW = 9` (fc_in input dim = 9*64+2 = 578), because the forward pass
    /// builds the input from that constant.
    pub fn from_mpk_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let device = Default::default();
        let recorder = NamedMpkBytesRecorder::<FullPrecisionSettings>::new();
        let dummy = ContextAcoustic::<B>::new(0, 1024, 1, &device);
        let record = recorder
            .load(bytes, &device)
            .map_err(|e| format!("NamedMpkBytesRecorder::load failed: {e}"))?;
        let model: ContextAcoustic<B> = dummy.load_record(record);
        Ok(Self { model })
    }

    /// Total number of parameter **elements** held by the loaded module
    /// (`Module::num_params`). Proves the checkpoint really became tensors
    /// inside the wasm instance, independently of how much memory grew.
    pub fn param_count(&self) -> usize {
        self.model.num_params()
    }

    /// `ids`: rows*WINDOW phoneme ids · `feats`: rows*2 (f0_norm, pos_frac)
    /// -> mel [rows, 128] flattened.
    pub fn forward(&self, ids: &[i64], feats: &[f32], rows: usize) -> Result<Vec<f32>, String> {
        if rows == 0 {
            return Err("rows must be > 0".into());
        }
        if ids.len() != rows * WINDOW {
            return Err(format!(
                "ids len {} != rows*WINDOW {}",
                ids.len(),
                rows * WINDOW
            ));
        }
        if feats.len() != rows * 2 {
            return Err(format!("feats len {} != rows*2 {}", feats.len(), rows * 2));
        }
        let device = Default::default();
        let t_ids = Tensor::<B, 2, Int>::from_data(
            TensorData::new(ids.to_vec(), vec![rows, WINDOW]),
            &device,
        );
        let t_feats =
            Tensor::<B, 2>::from_data(TensorData::new(feats.to_vec(), vec![rows, 2]), &device);
        let out = self.model.forward(t_ids, t_feats); // [rows, 128]
        out.into_data()
            .convert::<f32>()
            .to_vec::<f32>()
            .map_err(|e| format!("mel to_vec: {e}"))
    }
}

/// Deterministic synthetic input. The browser gets the SAME numbers through the
/// exported `synth_ids` / `synth_feats` below, so no generator is duplicated in JS.
///
/// - ids: `(r*31 + k*7 + 11) % 512` (512 = embedding slots of the checkpoints used here)
/// - f0_norm: `0.25 + (r % 23) * 0.02`  (0.25 .. 0.69)
/// - pos_frac: `(r % 17) / 17`
pub fn synth_ids(rows: usize) -> Vec<i64> {
    let mut v = Vec::with_capacity(rows * WINDOW);
    for r in 0..rows {
        for k in 0..WINDOW {
            v.push(((r * 31 + k * 7 + 11) % 512) as i64);
        }
    }
    v
}

pub fn synth_feats(rows: usize) -> Vec<f32> {
    let mut v = Vec::with_capacity(rows * 2);
    for r in 0..rows {
        v.push(0.25f32 + (r % 23) as f32 * 0.02f32);
        v.push((r % 17) as f32 / 17.0f32);
    }
    v
}

/// Same generator with a different seed -> used to prove the output MOVES with the input.
pub fn synth_ids_alt(rows: usize) -> Vec<i64> {
    let mut v = Vec::with_capacity(rows * WINDOW);
    for r in 0..rows {
        for k in 0..WINDOW {
            v.push(((r * 17 + k * 13 + 5) % 512) as i64);
        }
    }
    v
}

pub fn synth_feats_alt(rows: usize) -> Vec<f32> {
    let mut v = Vec::with_capacity(rows * 2);
    for r in 0..rows {
        v.push(0.10f32 + (r % 11) as f32 * 0.03f32);
        v.push((r % 13) as f32 / 13.0f32);
    }
    v
}

/// min/max/mean/rms/non-finite/distinct-count of a mel buffer.
pub struct Stats {
    pub n: usize,
    pub min: f32,
    pub max: f32,
    pub mean: f64,
    pub rms: f64,
    pub non_finite: usize,
    pub distinct: usize,
    pub checksum: u64,
}

pub fn stats(v: &[f32]) -> Stats {
    let mut s = Stats {
        n: v.len(),
        min: f32::INFINITY,
        max: f32::NEG_INFINITY,
        mean: 0.0,
        rms: 0.0,
        non_finite: 0,
        distinct: 0,
        checksum: 0,
    };
    let mut sum = 0.0f64;
    let mut sq = 0.0f64;
    let mut seen = std::collections::BTreeSet::new();
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in v {
        if !x.is_finite() {
            s.non_finite += 1;
            continue;
        }
        if x < s.min {
            s.min = x;
        }
        if x > s.max {
            s.max = x;
        }
        sum += x as f64;
        sq += (x as f64) * (x as f64);
        if seen.len() < 64 {
            seen.insert(x.to_bits());
        }
        for b in x.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    let ok = v.len() - s.non_finite;
    s.mean = if ok == 0 { 0.0 } else { sum / ok as f64 };
    s.rms = if ok == 0 {
        0.0
    } else {
        (sq / ok as f64).sqrt()
    };
    s.distinct = seen.len();
    s.checksum = h;
    s
}

pub fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max)
}
