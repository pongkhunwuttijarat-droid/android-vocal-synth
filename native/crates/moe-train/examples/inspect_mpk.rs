//! Inspect a burn .mpk checkpoint (NamedMpkFileRecorder) and report what it
//! actually contains: which tensor keys exist, their shapes and value stats -
//! without trusting the file name.
//!
//! Usage:
//!   cargo run -p moe-train --example inspect_mpk -- <path/to/file.mpk>
//!
//! Step 1 loads the file into the CURRENT moe_train::baseline::ContextAcoustic
//! layout (WINDOW = 9, EMBED_SLOTS = 4096) and then runs one forward pass:
//! burn 0.21 does NOT validate shapes at load time (Tensor::from_item builds the
//! tensor from the file own data/shape), so "load OK" alone proves nothing - the
//! mismatch only shows up as a forward-time panic.
//! Step 2 loads the file into the LEGACY ContextAcoustic layout (5-wide context,
//! 512 embedding slots) and dumps every key/shape from the loaded tensors, which
//! is the authoritative list (it comes from the file own serialized data).

use burn::module::Module;
use burn::nn::{Dropout, DropoutConfig, Embedding, EmbeddingConfig, Linear, LinearConfig};
use burn::record::{FileRecorder, FullPrecisionSettings, NamedMpkFileRecorder};
use burn::tensor::backend::Backend;
use burn::tensor::{Int, Tensor, TensorData};

type Nb = burn::backend::NdArray;
type Rec = NamedMpkFileRecorder<FullPrecisionSettings>;

/// Pre-WINDOW-9 / pre-EMBED_SLOTS-4096 ContextAcoustic.
/// Field names MUST match the derived Module record in the file
/// (emb / fc_in / hiddens / out / dropout) or deserialization fails.
#[derive(Module, Debug)]
struct LegacyContextAcoustic<B: Backend> {
    emb: Embedding<B>,
    fc_in: Linear<B>,
    hiddens: Vec<Linear<B>>,
    out: Linear<B>,
    dropout: Dropout,
}

impl<B: Backend> LegacyContextAcoustic<B> {
    fn new_legacy(device: &B::Device) -> Self {
        let emb = EmbeddingConfig::new(512, 64).init(device); // 512 slots, dim 64
        let fc_in = LinearConfig::new(5 * 64 + 2, 1024).init(device); // WINDOW=5, +/-2
        let hiddens = vec![LinearConfig::new(1024, 1024).init(device)];
        let out = LinearConfig::new(1024, 128).init(device);
        let dropout = DropoutConfig::new(0.2).init();
        Self {
            emb,
            fc_in,
            hiddens,
            out,
            dropout,
        }
    }
}

fn stat<B: Backend, const D: usize>(name: &str, t: Tensor<B, D>, total: &mut usize) {
    let dims = t.dims();
    let n: usize = dims.iter().product();
    *total += n;
    let data = t.into_data().convert::<f32>();
    let v = data.to_vec::<f32>().unwrap_or_default();
    let (mut mn, mut mx, mut sum, mut sq) = (f32::INFINITY, f32::NEG_INFINITY, 0.0f64, 0.0f64);
    for &x in &v {
        if x < mn {
            mn = x;
        }
        if x > mx {
            mx = x;
        }
        sum += x as f64;
        sq += (x as f64) * (x as f64);
    }
    let mean = if v.is_empty() {
        0.0
    } else {
        sum / v.len() as f64
    };
    let rms = if v.is_empty() {
        0.0
    } else {
        (sq / v.len() as f64).sqrt()
    };
    println!(
        "  {name:<22} shape={dims:?} n={n:<9} first3={:?} min={mn:.5} max={mx:.5} mean={mean:.6} rms={rms:.6}",
        &v[..v.len().min(3)]
    );
}

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: inspect_mpk <path/to/file.mpk>");
        std::process::exit(2);
    };
    println!("file      : {path}");
    if let Ok(m) = std::fs::metadata(&path) {
        println!("size      : {} bytes", m.len());
    }
    println!(
        "recorder  : {} (extension .{})",
        std::any::type_name::<Rec>(),
        <Rec as FileRecorder<Nb>>::file_extension()
    );
    let window = moe_train::baseline::WINDOW;
    println!(
        "current   : WINDOW={window} EMBED_SLOTS={} hidden={} depth={}",
        moe_train::baseline::EMBED_SLOTS,
        moe_train::baseline::DEFAULT_HIDDEN,
        moe_train::baseline::DEFAULT_DEPTH
    );

    let device = Default::default();
    let recorder = Rec::new();

    // ---- (1) CURRENT layout: load + one forward pass ----------------------
    let current = moe_train::baseline::ContextAcoustic::<Nb>::new(
        0,
        moe_train::baseline::DEFAULT_HIDDEN,
        moe_train::baseline::DEFAULT_DEPTH,
        &device,
    );
    match current.load_file(&path, &recorder, &device) {
        Ok(m) => {
            println!("current load        : OK  (burn 0.21 does NOT validate shapes on load)");
            let ids = Tensor::<Nb, 2, Int>::from_data(
                TensorData::new(vec![1i64; window], vec![1, window]),
                &device,
            );
            let feats =
                Tensor::<Nb, 2>::from_data(TensorData::new(vec![0.0f32, 0.0], vec![1, 2]), &device);
            let probed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                m.forward(ids, feats).into_data();
            }));
            match probed {
                Ok(_) => println!("current forward     : OK (weights fit the current layout)"),
                Err(_) => println!(
                    "current forward     : PANIC (shape mismatch - the file is NOT the current {}x architecture)",
                    moe_train::baseline::WINDOW
                ),
            }
        }
        Err(e) => println!("current load        : FAIL -> {e}"),
    }

    // ---- (2) LEGACY layout: authoritative key/shape list ------------------
    let legacy = LegacyContextAcoustic::<Nb>::new_legacy(&device);
    let m = match legacy.load_file(&path, &recorder, &device) {
        Ok(m) => m,
        Err(e) => {
            println!("legacy load         : FAIL -> {e}");
            std::process::exit(1);
        }
    };
    println!("legacy load         : OK (real burn NamedMpk checkpoint)");
    println!("keys + shapes (from the loaded tensors):");
    let mut total = 0usize;
    stat("emb.weight", m.emb.weight.val(), &mut total);
    stat("fc_in.weight", m.fc_in.weight.val(), &mut total);
    if let Some(b) = m.fc_in.bias.as_ref() {
        stat("fc_in.bias", b.val(), &mut total);
    }
    for (i, h) in m.hiddens.iter().enumerate() {
        stat(&format!("hiddens[{i}].weight"), h.weight.val(), &mut total);
        if let Some(b) = h.bias.as_ref() {
            stat(&format!("hiddens[{i}].bias"), b.val(), &mut total);
        }
    }
    stat("out.weight", m.out.weight.val(), &mut total);
    if let Some(b) = m.out.bias.as_ref() {
        stat("out.bias", b.val(), &mut total);
    }
    println!("total parameters: {total}");
    println!("f32 payload      : {} bytes", total * 4);
}
