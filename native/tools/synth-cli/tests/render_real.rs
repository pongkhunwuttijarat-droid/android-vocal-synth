//! Real-audio integration tests against the built `libworldline.so`
//! (ignored by default — they need the prebuilt library and the golden
//! Teto English voicebank):
//!
//! ```sh
//! WORLDLINE_SO=.../native/build/build-linux/libworldline.so \
//!     cargo test -p synth-cli --test render_real -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use synth_cli::pipeline::{self, PhonemizerKind, SAMPLE_RATE};
use wavwriter::write_wav_16;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn so_path() -> PathBuf {
    if let Ok(path) = std::env::var("WORLDLINE_SO") {
        return PathBuf::from(path);
    }
    let path = repo_root().join("native/build/build-linux/libworldline.so");
    assert!(
        path.exists(),
        "built libworldline.so not found at {path:?}; set WORLDLINE_SO"
    );
    path
}

fn teto_voicebank_path() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

fn mock_voicebank_path() -> PathBuf {
    repo_root().join("native/test-data/mock-voicebank")
}

fn mock_song_path() -> PathBuf {
    repo_root().join("native/test-data/mock-song.ustx")
}

fn demo_song_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/demo-song.ustx")
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()))
}

/// DiffSinger E2E with the FAKE model (no .so needed — the DsEngine
/// renders entirely through ort): dsconfig load → phoneme table → tensor
/// build (tokens/durations/f0) → acoustic.onnx → mel → vocoder.onnx →
/// PCM. The fake model (`test/golden/diffsinger-fake`) is a crude
/// Unsqueeze+Tile graph — its job is to prove the chain, not sound like
/// a singer. The 29-note scale demo should produce ~14.5 s of samples.
#[test]
fn diffsinger_fake_model_renders_e2e() {
    use synth_cli::engine::{DsEngine, Engine as _};

    let project = pipeline::load_project(&repo_root().join("test/golden/scale-demo-app.ustx"))
        .expect("load scale demo");
    let voicebank = pipeline::load_voicebank(&teto_voicebank_path()).expect("load teto bank");
    let model_dir = repo_root().join("test/golden/diffsinger-fake");
    let mut engine = DsEngine::open(&model_dir, false).expect("open fake ds model");

    let report = engine.render_project(&project, &voicebank, 0).expect("render project");
    assert!(
        report.skipped.is_empty(),
        "nothing may be skipped: {:?}",
        report.skipped
    );
    assert!(!report.samples.is_empty(), "synth produced no samples");

    let duration_ms = report.samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    println!(
        "diffsinger fake: {} samples ({duration_ms:.1} ms), peak {:.2}",
        report.samples.len(),
        peak(&report.samples)
    );
    assert!(
        duration_ms > 10000.0,
        "expected ~14.5 s of audio, got {duration_ms:.1} ms"
    );
}

/// THE first-sound test: demo-song (Teto-compatible hints) through the
/// real .so with the full Teto English bank. Non-silent audio, > 1 s, and
/// the wavwriter round-trip must decode to the same sample count.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn renders_demo_song_with_teto_bank_to_audio() {
    let project = pipeline::load_project(&demo_song_path()).expect("load demo-song");
    let voicebank = pipeline::load_voicebank(&teto_voicebank_path()).expect("load teto bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");

    let report = pipeline::render_project(
        &project,
        &voicebank,
        &renderer,
        0,
        PhonemizerKind::English,
        true,
        &mut None,
        None,
        None,
    )
    .expect("render project");
    assert_eq!(
        report.skipped,
        Vec::<String>::new(),
        "nothing may be skipped"
    );
    // demo-song is a single ~2496 ms phrase and the pipeline's chunk target
    // is 4000 ms (`pipeline::CHUNK_TARGET_MS`), so it renders as ONE chunk.
    // (This asserted 2 back when the target was ~1500 ms.)
    assert_eq!(
        report.phrases_rendered, 1,
        "one 2496 ms phrase under a 4000 ms chunk target is one chunk"
    );
    assert!(!report.samples.is_empty(), "synth produced no samples");

    let duration_ms = report.samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    let p = peak(&report.samples);
    println!(
        "demo-song: {} samples ({duration_ms:.1} ms), peak {p:.4}",
        report.samples.len()
    );
    assert!(
        duration_ms > 1000.0,
        "expected >1 s of audio, got {duration_ms:.1} ms"
    );
    assert!(p > 1e-4, "output is silent (peak {p})");

    // The wavwriter round-trip: 16-bit PCM encodes the same length and
    // decodes back (within quantization) via the voicebank reader.
    let wav = write_wav_16(&report.samples, SAMPLE_RATE);
    let parsed = voicebank::parse_wav(&wav).expect("parse written wav");
    assert_eq!(parsed.samples.len(), report.samples.len());
    assert_eq!(parsed.sample_rate, SAMPLE_RATE);
    println!(
        "wav round-trip: {} bytes, {} samples",
        wav.len(),
        parsed.samples.len()
    );
}

/// The `synth-note` quick path with the mock voicebank: one "A" phoneme
/// must produce ~500 ms of non-silent audio.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn synth_note_renders_mock_bank_phoneme() {
    let voicebank = pipeline::load_voicebank(&mock_voicebank_path()).expect("load mock bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");

    let samples = pipeline::synth_note(&voicebank, &renderer, "A", 60, 500.0).expect("synth note");
    assert!(!samples.is_empty());
    let duration_ms = samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    let p = peak(&samples);
    println!(
        "synth-note A: {} samples ({duration_ms:.1} ms), peak {p:.4}",
        samples.len()
    );
    assert!(
        duration_ms > 400.0,
        "expected ~500 ms, got {duration_ms:.1} ms"
    );
    assert!(p > 1e-4, "output is silent (peak {p})");
}

/// Honesty check for the milestone report: mock-song's ARPABET hints do
/// not exist in the Teto bank (nor the mock bank), so `render` must
/// report the skip cleanly — not crash — and produce no audio.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn mock_song_with_teto_bank_is_reported_as_skipped() {
    let project = pipeline::load_project(&mock_song_path()).expect("load mock-song");
    let voicebank = pipeline::load_voicebank(&teto_voicebank_path()).expect("load teto bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");

    let report = pipeline::render_project(
        &project,
        &voicebank,
        &renderer,
        0,
        PhonemizerKind::English,
        false,
        &mut None,
        None,
        None,
    )
    .expect("render project must not crash");
    assert!(report.samples.is_empty());
    assert_eq!(report.skipped.len(), 1, "one phrase, one skip reason");
    println!("skip reason: {}", report.skipped[0]);
    // 3 of mock-song's 9 phonemes (d, d, l) do exist in the Teto bank, so
    // the phrase partially maps and the renderer's 1:1 oto↔phoneme
    // contract rejects it — reported, not crashed.
    assert!(report.skipped[0].contains("render failed"));
}

/// Partial resolution: demo-song hints phonemize to s/i/l/e/d/3/A — only
/// "A" exists in the mock bank, so the renderer's 1:1 oto↔phoneme
/// contract rejects the phrase (reported, not crashed).
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn demo_song_with_mock_bank_partially_resolves_and_is_reported() {
    let project = pipeline::load_project(&demo_song_path()).expect("load demo-song");
    let voicebank = pipeline::load_voicebank(&mock_voicebank_path()).expect("load mock bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");

    let report = pipeline::render_project(
        &project,
        &voicebank,
        &renderer,
        0,
        PhonemizerKind::English,
        false,
        &mut None,
        None,
        None,
    )
    .expect("render project must not crash");
    assert!(report.samples.is_empty());
    assert_eq!(report.skipped.len(), 1);
    println!("skip reason: {}", report.skipped[0]);
    assert!(report.skipped[0].contains("render failed"));
}

/// The track's phonemizer name picks the phonemizer kind.
#[test]
fn phonemizer_kind_from_track_name() {
    let mut track = domain::UTrack::new("Track1");
    // Default is now the Universal (IPA-first) phonemizer.
    assert_eq!(PhonemizerKind::from_track(&track), PhonemizerKind::Universal);
    track.phonemizer = Some("OpenUtau Japanese VCV Phonemizer".into());
    assert_eq!(PhonemizerKind::from_track(&track), PhonemizerKind::Japanese);
    track.phonemizer = Some("EnglishVCCVPhonemizer".into());
    // English-declared tracks now route to Universal too — the legacy
    // English CVVC path's verbatim fallback skips whole phrases on
    // unknown words ("shi" -> no alias -> song renders only its first
    // phrase); Universal letter-splits + NearestPhoneme-maps them.
    assert_eq!(PhonemizerKind::from_track(&track), PhonemizerKind::Universal);
    // An unrecognized name falls through to Universal (was English).
    track.phonemizer = Some("Custom Renderer".into());
    assert_eq!(PhonemizerKind::from_track(&track), PhonemizerKind::Universal);
}

// ---------------------------------------------------------------------------
// Rust-kernel (`r1-rs`) vs C++ `.so` (`r1`) parity
// ---------------------------------------------------------------------------
//
// The pure-Rust Worldline port (`crates/worldline-rs` + `crates/world-rs`)
// must render the SAME audio as the reference `libworldline.so` through the
// exact same pipeline. These tests pin that down at sample level, because
// the length alone is not enough: the frame grid (`512000/44100` ms, the
// C++ `FrameMs()` value — NOT the rounded `11.6` the plugin uses for curve
// sampling) determines the p0..p4 timing and therefore the output length.
//
// They need the built `.so` and the Teto bank, so they are `#[ignore]`d like
// the rest of this file.

/// Compare two renders: identical length, tiny relative RMS difference and
/// a bounded per-sample deviation (float-rounding level).
fn assert_engines_match(label: &str, so_out: &[f32], rs_out: &[f32]) {
    assert_eq!(
        so_out.len(),
        rs_out.len(),
        "{label}: sample count differs (so={}, rs={}) — the frame grid or \
         the p0..p4 timing diverged",
        so_out.len(),
        rs_out.len()
    );
    let rms = |x: &[f32]| -> f64 {
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    };
    let so_rms = rms(so_out);
    let mut sq = 0.0f64;
    let mut max_diff = 0.0f32;
    for (a, b) in so_out.iter().zip(rs_out) {
        let d = (b - a).abs();
        max_diff = max_diff.max(d);
        sq += f64::from(d) * f64::from(d);
    }
    let rms_diff = (sq / so_out.len() as f64).sqrt();
    let rel = if so_rms > 0.0 { rms_diff / so_rms } else { 0.0 };
    println!(
        "{label}: {} samples | rms so={so_rms:.8} diff={rms_diff:.3e} (rel {rel:.3e}) max|diff|={max_diff:.3e}",
        so_out.len()
    );
    assert!(so_rms > 1e-4, "{label}: reference render is silent");
    assert!(
        rel < 1e-3,
        "{label}: relative RMS difference {rel:.3e} exceeds 1e-3"
    );
    assert!(
        max_diff < 5e-3,
        "{label}: max per-sample difference {max_diff:.3e} exceeds 5e-3"
    );
}

/// Render `project` with the `.so` r1 renderer and the pure-Rust `r1-rs`
/// renderer and assert the outputs match.
fn compare_engines(project: &Path, label: &str) -> (Vec<f32>, Vec<f32>) {
    use synth_cli::engine::{Engine as _, WorldlineRsEngine};

    let project = pipeline::load_project(project).expect("load project");
    let voicebank = pipeline::load_voicebank(&teto_voicebank_path()).expect("load teto bank");

    let mut so_engine = synth_cli::engine::WorldlineEngine::open(so_path(), false)
        .expect("open r1 (.so) engine");
    let mut rs_engine = WorldlineRsEngine::open(false).expect("open r1-rs engine");

    let so_report = so_engine
        .render_project(&project, &voicebank, 0)
        .expect("render with .so");
    let rs_report = rs_engine
        .render_project(&project, &voicebank, 0)
        .expect("render with r1-rs");

    assert!(so_report.skipped.is_empty(), "so skipped: {:?}", so_report.skipped);
    assert!(rs_report.skipped.is_empty(), "rs skipped: {:?}", rs_report.skipped);
    assert_engines_match(label, &so_report.samples, &rs_report.samples);
    (so_report.samples, rs_report.samples)
}

/// The demo song (Teto-compatible hints) must render identically through
/// the `.so` and the pure-Rust kernel.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn rust_kernel_matches_so_on_demo_song() {
    compare_engines(&demo_song_path(), "demo-song r1 vs r1-rs");
}

/// The 29-note scale demo (~14.5 s, chunked) must also match — this is the
/// multi-chunk case where a frame-grid divergence shifts chunk boundaries.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn rust_kernel_matches_so_on_scale_demo() {
    compare_engines(&repo_root().join("test/golden/scale-demo-app.ustx"), "scale-demo r1 vs r1-rs");
}


// ---------------------------------------------------------------------------
// Classic resampler (`Resample`) parity
// ---------------------------------------------------------------------------
//
// `worldline_rs::resampler` ports the reference's CLASSIC per-phoneme
// `Resample()` entry point. Two things are easy to get wrong and are pinned
// here: the pitch grid uses the reference's integer truncation (the C++
// declares `int t` and `t -= index`, so the bend is a step function, not an
// interpolated one), and the resampled length must be sample-exact.

/// One phoneme through the reference `Resample` C entry point.
fn so_resample(request: &worldline_plugin::PhonemeRequest) -> Vec<f32> {
    let lib = unsafe { libloading::Library::new(so_path()) }.expect("dlopen libworldline.so");
    type ResampleFn =
        unsafe extern "C" fn(*const worldline_sys::SynthRequest, *mut *mut f32) -> i32;
    let entry: libloading::Symbol<ResampleFn> =
        unsafe { lib.get(b"Resample") }.expect("resolve Resample");

    let wav = voicebank::read_wav(std::path::Path::new(&request.wav_path)).expect("read wav");
    let mono: Vec<f64> = wav.to_mono().into_iter().map(|s| s as f64).collect();
    let frq = read_frq_for(&request.wav_path);
    let (frq_len, frq_ptr) = match frq.as_deref() {
        Some(f) => (
            f.len() as i32,
            f.as_ptr() as *const std::os::raw::c_char as *mut std::os::raw::c_char,
        ),
        None => (0, std::ptr::null_mut()),
    };
    let synth = worldline_sys::SynthRequest {
        sample_fs: wav.sample_rate as i32,
        sample_length: mono.len() as i32,
        sample: mono.as_ptr() as *mut f64,
        frq_length: frq_len,
        frq: frq_ptr,
        tone: request.tone,
        con_vel: request.con_vel,
        offset: request.offset,
        required_length: request.required_length,
        consonant: request.consonant,
        cut_off: request.cut_off,
        volume: request.volume,
        modulation: request.modulation,
        tempo: request.tempo,
        pitch_bend_length: request.pitch_bend.len() as i32,
        pitch_bend: request.pitch_bend.as_ptr() as *mut i32,
        flag_g: request.flag_g,
        flag_O: request.flag_O,
        flag_P: request.flag_P,
        flag_Mt: request.flag_Mt,
        flag_Mb: request.flag_Mb,
        flag_Mv: request.flag_Mv,
    };
    let mut y: *mut f32 = std::ptr::null_mut();
    let n = unsafe { entry(&synth, &mut y) };
    if n <= 0 || y.is_null() {
        return Vec::new();
    }
    let out = unsafe { std::slice::from_raw_parts(y, n as usize) }.to_vec();
    unsafe { worldline_sys::free_buffer(y) };
    out
}

fn read_frq_for(wav_path: &str) -> Option<Vec<u8>> {
    let path = std::path::Path::new(wav_path);
    let stem = path.file_stem()?.to_str()?;
    let data = std::fs::read(path.with_file_name(format!("{stem}_wav.frq"))).ok()?;
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

/// Every phoneme of the demo song through the classic path must match the
/// reference `.so` (same length, sub-1e-4 per-sample deviation).
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn rust_classic_resampler_matches_so() {
    let project = pipeline::load_project(&demo_song_path()).expect("load demo-song");
    let voicebank = pipeline::load_voicebank(&teto_voicebank_path()).expect("load teto bank");
    let track = project.tracks.first().expect("track");
    let part = project
        .parts
        .iter()
        .find_map(|p| match p {
            domain::UPart::Voice(v) if v.track_no == 0 => Some(v),
            _ => None,
        })
        .expect("voice part");
    let phrases = pipeline::build_phrase_inputs(
        &project,
        track,
        part,
        &voicebank,
        PhonemizerKind::Universal,
    )
    .expect("build phrases");
    let requests = worldline_plugin::build_requests(&phrases[0].input).expect("build requests");
    assert_eq!(requests.len(), 7, "demo song phoneme count");

    let mut worst = 0.0f32;
    for request in &requests {
        let wav = voicebank::read_wav(std::path::Path::new(&request.wav_path)).expect("read wav");
        let mono: Vec<f64> = wav.to_mono().into_iter().map(|s| s as f64).collect();
        let so_out = so_resample(request);
        let rs_out = worldline_rs::resampler::resample(
            request,
            mono,
            wav.sample_rate as i32,
            read_frq_for(&request.wav_path).as_deref(),
        )
        .expect("rust resample");
        assert_eq!(
            so_out.len(),
            rs_out.len(),
            "{}: length differs (so={}, rs={})",
            request.phoneme,
            so_out.len(),
            rs_out.len()
        );
        let mut max_diff = 0.0f32;
        for (a, b) in so_out.iter().zip(&rs_out) {
            max_diff = max_diff.max((a - b).abs());
        }
        println!(
            "  classic {:<4} {} samples  max|diff|={max_diff:.3e}",
            request.phoneme,
            rs_out.len()
        );
        worst = worst.max(max_diff);
    }
    println!("  classic worst max|diff| = {worst:.3e}");
    assert!(
        worst < 1e-3,
        "classic resampler diverged: worst max|diff| = {worst:.3e}"
    );
}


// ---------------------------------------------------------------------------
// pYIN F0 parity (the no-`.frq` fallback)
// ---------------------------------------------------------------------------

/// The reference estimates F0 with pYIN (libpyin) for any sample that ships
/// no `.frq`. `worldline_rs::pyin` ports libpyin + the one libgvps Viterbi
/// routine it uses; this test pins the two together through the `.so`'s
/// `F0(..., method = 2)` entry point.
#[test]
#[ignore = "requires libworldline.so; run with WORLDLINE_SO=... cargo test -p synth-cli --test render_real -- --ignored --nocapture"]
fn rust_pyin_matches_libpyin_through_the_so() {
    let bank = teto_voicebank_path();
    let wav_path = bank.join("voice/_a+_ha+_a+_a+_a+-.wav");
    if !wav_path.is_file() {
        eprintln!("skip: {} not present", wav_path.display());
        return;
    }
    let wav = voicebank::read_wav(&wav_path).expect("read wav");
    let mono: Vec<f32> = wav.to_mono();
    let mono64: Vec<f64> = mono.iter().map(|s| f64::from(*s)).collect();
    let fs = wav.sample_rate as i32;
    let frame_ms = 512.0 * 1000.0 / 44100.0;
    let nhop = ((f64::from(fs) * frame_ms / 1000.0).round() as i32).max(1);

    let analysis = worldline_sys::Analysis::open(so_path()).expect("open analysis");
    let so_f0 = analysis.f0(&mono, fs, frame_ms, 2);
    let raw = worldline_rs::pyin::analyze(&worldline_rs::pyin::PyinConfig::new(nhop), &mono64, f64::from(fs));
    // `PyinEstimator::Estimate` drops the first raw frame.
    let rust_f0 = &raw[1..];

    assert_eq!(
        so_f0.len(),
        rust_f0.len(),
        "frame count differs (so={}, rust={})",
        so_f0.len(),
        rust_f0.len()
    );
    let mut voiced_agree = 0usize;
    let mut max_cents = 0.0f64;
    for (a, b) in so_f0.iter().zip(rust_f0) {
        if (*a > 0.0) == (*b > 0.0) {
            voiced_agree += 1;
        }
        if *a > 0.0 && *b > 0.0 {
            max_cents = max_cents.max((1200.0 * (b / a).log2()).abs());
        }
    }
    let agreement = voiced_agree as f64 / so_f0.len().max(1) as f64;
    println!(
        "  pyin: {} frames, voicing agreement {:.1}%, max |cents| {max_cents:.4}",
        so_f0.len(),
        agreement * 100.0
    );
    assert!(
        agreement > 0.99,
        "voicing agreement {agreement:.4} below 0.99"
    );
    assert!(max_cents < 1.0, "max pitch deviation {max_cents:.4} cents");
}
