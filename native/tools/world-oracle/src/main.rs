//! `world-oracle` — stage-by-stage comparison of the pure-Rust WORLD port
//! (`world-rs`) against the C++ reference kernel (`libworldline.so`).
//!
//! The point is attribution: when the Rust port diverges from the C++
//! reference, we need to know WHICH stage drifted (spectral envelope,
//! aperiodicity, synthesis, phrase orchestration) instead of only seeing
//! a difference in the final PCM.
//!
//! Stages compared (identical inputs on both sides):
//!   A. analysis   — CheapTrick sp + D4C ap for the same samples + f0 grid
//!   B. synthesis  — WORLD Synthesis for the same f0/sp/ap features
//!   C. cross      — rs-analysis → so-synthesis and so-analysis → rs-synthesis
//!
//! Usage:
//!   world-oracle analysis  <wav> [--so PATH] [--json]
//!   world-oracle synthesis <wav> [--so PATH] [--json]
//!
//! Exit codes: 0 = ran, 2 = error.

use std::path::{Path, PathBuf};

use domain::UPart;

const SAMPLE_RATE: i32 = 44100;
const HOP_SIZE: i32 = 512;
const FFT_SIZE: i32 = 2048;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: world-oracle <analysis|synthesis> <wav> [--so PATH] [--json]\n       world-oracle phrase <project.ustx> <voicebank-dir> [--so PATH] [--json]\n       world-oracle classic <project.ustx> <voicebank-dir> [--so PATH] [--json]\n       world-oracle pyin <wav> [--so PATH] [--json]\n       world-oracle bench <project.ustx> <voicebank-dir> [--so PATH]"
        );
        return std::process::ExitCode::from(2);
    }
    let mode = args[1].clone();
    let wav = PathBuf::from(&args[2]);
    let mut so_path = default_so();
    let mut json = false;
    let mut extra_positional: Option<String> = None;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--so" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--so needs a path");
                    return std::process::ExitCode::from(2);
                }
                so_path = PathBuf::from(&args[i]);
            }
            "--json" => json = true,
            "--no-bend" => {
                std::env::set_var("WORLD_ORACLE_NO_BEND", "1");
            }
            other if extra_positional.is_none() => extra_positional = Some(other.to_string()),
            other => {
                eprintln!("unknown argument: {other}");
                return std::process::ExitCode::from(2);
            }
        }
        i += 1;
    }
    let extra = extra_positional.unwrap_or_default();
    match run(&mode, &wav, &so_path, json, &extra) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("world-oracle: error: {e}");
            std::process::ExitCode::from(2)
        }
    }
}

fn default_so() -> PathBuf {
    // tools/world-oracle → native/
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../build/build-linux/libworldline.so")
}

fn run(mode: &str, wav: &Path, so_path: &Path, json: bool, extra: &str) -> Result<(), String> {
    if mode == "bench" {
        if extra.is_empty() {
            return Err("bench mode needs: world-oracle bench <project.ustx> <voicebank-dir>".into());
        }
        return run_bench(wav, Path::new(extra));
    }
    if mode == "pyin" {
        return run_pyin(wav, so_path, json);
    }
    if mode == "classic" {
        if extra.is_empty() {
            return Err(
                "classic mode needs: world-oracle classic <project.ustx> <voicebank-dir>".into(),
            );
        }
        return run_classic(wav, Path::new(extra), so_path, json);
    }
    if mode == "phrase" {
        if extra.is_empty() {
            return Err("phrase mode needs: world-oracle phrase <project.ustx> <voicebank-dir>".into());
        }
        return run_phrase(wav, Path::new(extra), so_path, json);
    }
    let data = voicebank::read_wav(wav).map_err(|e| format!("read {}: {e}", wav.display()))?;
    let mono: Vec<f32> = if data.channels > 1 {
        data.to_mono()
    } else {
        data.samples.clone()
    };
    if mono.is_empty() {
        return Err(format!("{} has no samples", wav.display()));
    }
    println!(
        "input: {} ({} samples, {} Hz, {} ch, {:.1} ms)",
        wav.display(),
        mono.len(),
        data.sample_rate,
        data.channels,
        mono.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE)
    );
    println!("oracle: {} (fft={FFT_SIZE} hop={HOP_SIZE} fs={SAMPLE_RATE})", so_path.display());

    let frame_ms = f64::from(HOP_SIZE) * 1000.0 / f64::from(SAMPLE_RATE);
    let f0 = f0_for(&mono, wav, frame_ms);
    println!(
        "f0: {} frames, voiced {}/{} ({:.1}%)",
        f0.len(),
        f0.iter().filter(|v| **v > 0.0).count(),
        f0.len(),
        100.0 * f0.iter().filter(|v| **v > 0.0).count() as f64 / f0.len().max(1) as f64
    );

    let analysis = worldline_sys::Analysis::open(so_path)
        .map_err(|e| format!("open {}: {e}", so_path.display()))?;
    let config = analysis.init_config(SAMPLE_RATE, HOP_SIZE, FFT_SIZE);
    let sp_size = (FFT_SIZE / 2 + 1) as usize;

    // ---- Stage A: analysis ------------------------------------------------
    let (sp_so, ap_so) = analysis.analysis_f0_in(&config, &mono, &f0, sp_size);

    let rs_config = world_rs::analysis::AnalysisConfig {
        sample_rate: SAMPLE_RATE as u32,
        frame_period_ms: frame_ms,
        fft_size: FFT_SIZE as usize,
        min_f0_hz: f64::from(config.f0_floor),
        max_f0_hz: 1100.0,
    };
    let features = world_rs::analysis::analyze_world_audio_with_f0(&mono, rs_config, Some(&f0), &())
        .map_err(|e| format!("world-rs analysis: {e:?}"))?;

    let sp_stats = log_spectrum_stats(&sp_so, &features.spectral, features.bin_count);
    let ap_stats = ratio_stats(&ap_so, &features.aperiodicity, features.bin_count);
    println!();
    println!("== stage A: analysis ==");
    println!(
        "  frames: so={} rs={}   bins: so={} rs={}",
        sp_so.len() / sp_size,
        features.frame_count,
        sp_size,
        features.bin_count
    );
    print_log_stats("  spectral envelope (dB)", &sp_stats);
    print_ratio_stats("  aperiodicity (0..1) ", &ap_stats);

    if mode == "analysis" {
        if json {
            println!(
                "{{\"stage\":\"analysis\",\"frames_so\":{},\"frames_rs\":{},\"sp_corr\":{:.9},\"sp_db_mae\":{:.6},\"sp_db_max\":{:.6},\"ap_corr\":{:.9},\"ap_mae\":{:.6},\"ap_max\":{:.6}}}",
                sp_so.len() / sp_size,
                features.frame_count,
                sp_stats.corr,
                sp_stats.db_mae,
                sp_stats.db_max,
                ap_stats.corr,
                ap_stats.mae,
                ap_stats.max
            );
        }
        return Ok(());
    }

    // ---- Stage B: synthesis on IDENTICAL features -------------------------
    let frames = features.frame_count.min(sp_so.len() / sp_size);
    let neutral = NeutralCurves::new(frames);

    let y_len = 1 + ((frames as f64 - 1.0) * frame_ms / 1000.0 * f64::from(SAMPLE_RATE)) as usize;

    let so_out = so_synthesis(&analysis, &f0[..frames], &sp_so, &ap_so, sp_size, frame_ms, &neutral, y_len);
    println!();
    println!("== stage B: synthesis (identical so-derived features) ==");
    println!("  so output: {} samples", so_out.len());

    // so features → rs synthesis
    let mut f_so = world_rs::WorldFeatures::try_new(frames, sp_size)
        .map_err(|e| format!("features: {e:?}"))?;
    f_so.f0.copy_from_slice(&f0[..frames]);
    f_so.spectral.copy_from_slice(&sp_so[..frames * sp_size]);
    f_so.aperiodicity.copy_from_slice(&ap_so[..frames * sp_size]);
    let rs_from_so = rs_synthesis(&f_so, frame_ms, y_len)?;
    println!("  rs output: {} samples", rs_from_so.len());
    print_pcm_stats("  so-features: so vs rs", &so_out, &rs_from_so);

    // rs features → so synthesis (isolates analysis error)
    let rs_from_rs = rs_synthesis(&features, frame_ms, y_len)?;
    let so_from_rs = so_synthesis(
        &analysis,
        &features.f0[..frames],
        &features.spectral[..frames * sp_size],
        &features.aperiodicity[..frames * sp_size],
        sp_size,
        frame_ms,
        &neutral,
        y_len,
    );
    print_pcm_stats("  rs-features: so vs rs", &so_from_rs, &rs_from_rs);
    print_pcm_stats("  same-features so vs rs-features so", &so_out, &so_from_rs);
    print_pcm_stats("  same-features rs vs rs-features rs", &so_out, &rs_from_rs);
    print_pcm_stats("  end-to-end: so(so feat) vs rs(rs feat)", &so_out, &rs_from_rs);

    if json {
        let a = pcm_stats(&so_out, &rs_from_so);
        let b = pcm_stats(&so_from_rs, &rs_from_rs);
        println!(
            "{{\"stage\":\"synthesis\",\"frames\":{frames},\"so_samples\":{},\"rs_samples\":{},\"sp_corr\":{:.9},\"sp_db_mae\":{:.6},\"ap_corr\":{:.9},\"ap_mae\":{:.6},\"iso_corr\":{:.9},\"iso_rms_ratio\":{:.6},\"e2e_corr\":{:.9},\"e2e_rms_ratio\":{:.6}}}",
            so_out.len(),
            rs_from_rs.len(),
            sp_stats.corr,
            sp_stats.db_mae,
            ap_stats.corr,
            ap_stats.mae,
            a.corr,
            a.rms_ratio,
            b.corr,
            b.rms_ratio
        );
    }
    Ok(())
}





/// Time the full render pipeline (chunking + mixing, exactly like
/// `synth-cli render` / the server) for the pure-Rust engine.
///
/// Run it twice with `RAYON_NUM_THREADS=1` and unset to separate the DSP cost
/// from the chunk-parallelism speedup — that is what a wasm build forgoes.
fn run_bench(project_path: &Path, voicebank_path: &Path) -> Result<(), String> {
    use std::sync::atomic::AtomicBool;

    let project = synth_cli::pipeline::load_project(project_path)
        .map_err(|e| format!("load project: {e}"))?;
    let voicebank = synth_cli::pipeline::load_voicebank(voicebank_path)
        .map_err(|e| format!("load voicebank: {e}"))?;
    let track = project.tracks.first().ok_or("project has no tracks")?;
    let kind = synth_cli::pipeline::PhonemizerKind::from_track(track);

    // How many chunks does this project split into? (Chunk-level parallelism
    // is the only parallelism the pipeline has.)
    let mut chunks = 0usize;
    for part in project.parts.iter() {
        if let UPart::Voice(v) = part {
            if v.track_no != 0 {
                continue;
            }
            let phrases = synth_cli::pipeline::build_phrase_inputs(
                &project,
                track,
                v,
                &voicebank,
                kind,
            )?;
            for phrase in &phrases {
                chunks += synth_cli::chunking::plan_chunks(
                    &phrase.input.phonemes,
                    &phrase.input.notes,
                    synth_cli::pipeline::CHUNK_TARGET_MS,
                    1,
                )
                .len();
            }
        }
    }

    let engine = synth_cli::engine::WorldlineRsEngine::open(false)?;
    let mut cache = None;
    let cancel = AtomicBool::new(false);

    // Warm-up (page-in, any lazy init) — not timed.
    let _ = synth_cli::pipeline::render_project(
        &project,
        &voicebank,
        &engine,
        0,
        kind,
        false,
        &mut cache,
        None,
        Some(&cancel),
    )?;

    let mut times = Vec::new();
    let mut samples = 0usize;
    for _ in 0..3 {
        let t0 = std::time::Instant::now();
        let report = synth_cli::pipeline::render_project(
            &project,
            &voicebank,
            &engine,
            0,
            kind,
            false,
            &mut cache,
            None,
            Some(&cancel),
        )?;
        times.push(t0.elapsed().as_secs_f64() * 1000.0);
        samples = report.samples.len();
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let audio_s = samples as f64 / 44100.0;
    let best = times[0];
    println!(
        "bench: {} | {} chunks | {:.2}s audio | best {:.1} ms | {:.2}x realtime | runs {}",
        project.name,
        chunks,
        audio_s,
        best,
        audio_s / (best / 1000.0),
        times.iter().map(|t| format!("{t:.0}")).collect::<Vec<_>>().join("/")
    );
    Ok(())
}

/// pYIN comparison: the reference's `F0(samples, fs, frame_ms, method=2)`
/// (libpyin through the `.so`) vs the pure-Rust port (`worldline_rs::pyin`).
///
/// This is the F0 path the reference uses for any sample that ships no
/// `.frq`; a mismatch here means an imported bank (no `.frq`) would render
/// differently on the two engines.
fn run_pyin(wav: &Path, so_path: &Path, json: bool) -> Result<(), String> {
    let data = voicebank::read_wav(wav).map_err(|e| format!("read {}: {e}", wav.display()))?;
    let mono: Vec<f32> = if data.channels > 1 { data.to_mono() } else { data.samples.clone() };
    let mono64: Vec<f64> = mono.iter().map(|s| f64::from(*s)).collect();
    let fs = data.sample_rate as i32;
    let frame_ms = 512.0 * 1000.0 / 44100.0;
    let nhop = (f64::from(fs) * frame_ms / 1000.0).round() as i32;

    // ---- reference: F0(..., method = 2) = PyinEstimator ----
    let analysis = worldline_sys::Analysis::open(so_path)
        .map_err(|e| format!("open {}: {e}", so_path.display()))?;
    let so_f0 = analysis.f0(&mono, fs, frame_ms, 2);

    // ---- Rust port ----
    let config = worldline_rs::pyin::PyinConfig::new(nhop);
    let raw = worldline_rs::pyin::analyze(&config, &mono64, f64::from(fs));

    // `PyinEstimator::Estimate` drops the FIRST raw frame (`copy(raw + 1,
    // raw + f0_len)`), so the `.so` curve is `raw[1..]`. Comparing `so[i]`
    // against `raw[i]` shifts everything by one frame.
    let rust_f0: Vec<f64> = if raw.len() > 1 { raw[1..].to_vec() } else { Vec::new() };

    println!("input: {} ({} samples, {fs} Hz)", wav.display(), mono.len());
    println!(
        "f0: so={} frames, rust={} frames (raw={}, hop={nhop})",
        so_f0.len(),
        rust_f0.len(),
        raw.len()
    );

    let n = so_f0.len().min(rust_f0.len());
    if n == 0 {
        return Err("no frames to compare".into());
    }
    let mut voiced_so = 0usize;
    let mut voiced_rs = 0usize;
    let mut agree = 0usize;
    let mut cents_sum = 0.0f64;
    let mut cents_n = 0usize;
    let mut max_cents = 0.0f64;
    for i in 0..n {
        let a = so_f0[i];
        let b = rust_f0[i];
        if a > 0.0 { voiced_so += 1; }
        if b > 0.0 { voiced_rs += 1; }
        if (a > 0.0) == (b > 0.0) {
            agree += 1;
        }
        if a > 0.0 && b > 0.0 {
            let cents = (1200.0 * (b / a).log2()).abs();
            cents_sum += cents;
            cents_n += 1;
            max_cents = max_cents.max(cents);
        }
    }
    let mean_cents = if cents_n > 0 { cents_sum / cents_n as f64 } else { 0.0 };
    println!(
        "  voiced so={voiced_so} rust={voiced_rs}  voicing agreement={:.1}%  mean |cents|={mean_cents:.2}  max={max_cents:.2}",
        100.0 * agree as f64 / n as f64
    );
    if json {
        println!(
            "{{\"stage\":\"pyin\",\"frames_so\":{},\"frames_rs\":{},\"compared\":{n},\"voiced_so\":{voiced_so},\"voiced_rs\":{voiced_rs},\"voicing_agreement\":{:.6},\"mean_cents\":{mean_cents:.6},\"max_cents\":{max_cents:.6}}}",
            so_f0.len(),
            rust_f0.len(),
            100.0 * agree as f64 / n as f64
        );
    }
    Ok(())
}

/// Classic (UTAU `Resample`) comparison: the C++ `Resample` entry point vs
/// the pure-Rust `worldline_rs::ResamplerRs`, on the same per-phoneme
/// requests (same oto region, tone, flags).
fn run_classic(
    project_path: &Path,
    voicebank_path: &Path,
    so_path: &Path,
    json: bool,
) -> Result<(), String> {
    use worldline_plugin::PhonemeRequest;

    let project = synth_cli::pipeline::load_project(project_path)
        .map_err(|e| format!("load project: {e}"))?;
    let voicebank = synth_cli::pipeline::load_voicebank(voicebank_path)
        .map_err(|e| format!("load voicebank: {e}"))?;
    let track = project.tracks.first().ok_or("project has no tracks")?;
    let part = project
        .parts
        .iter()
        .find_map(|p| match p {
            UPart::Voice(v) if v.track_no == 0 => Some(v),
            _ => None,
        })
        .ok_or("track 0 has no voice part")?;

    let phrases = synth_cli::pipeline::build_phrase_inputs(
        &project,
        track,
        part,
        &voicebank,
        synth_cli::pipeline::PhonemizerKind::Universal,
    )?;
    let first = phrases.first().ok_or("project produced no phrases")?;
    let requests = worldline_plugin::build_requests(&first.input)
        .map_err(|e| format!("build_requests: {e}"))?;
    if requests.is_empty() {
        return Err("no per-phoneme requests".into());
    }

    // dlopen the reference `Resample` entry point.
    let lib = unsafe { libloading::Library::new(so_path) }
        .map_err(|e| format!("dlopen {}: {e}", so_path.display()))?;
    type ResampleFn =
        unsafe extern "C" fn(*const worldline_sys::SynthRequest, *mut *mut f32) -> i32;
    let so_resample: libloading::Symbol<ResampleFn> = unsafe {
        lib.get(b"Resample")
            .map_err(|e| format!("resolve Resample: {e}"))?
    };

    println!(
        "classic: {} | {} request(s) from the first phrase",
        project.name,
        requests.len()
    );

    let mut worst = 0.0f32;
    let mut compared = 0usize;
    let mut total_so = 0usize;
    let mut total_rs = 0usize;
    for (index, request) in requests.iter().enumerate() {
        let wav = voicebank::read_wav(Path::new(&request.wav_path))
            .map_err(|e| format!("read {}: {e}", request.wav_path))?;
        let mono_f64: Vec<f64> = wav.to_mono().into_iter().map(|s| s as f64).collect();
        let frq = read_frq_next_to(&request.wav_path);

        // `--no-bend` investigation: neutralise the pitch-bend curve on BOTH
        // sides. If the outputs then match, the divergence is entirely in the
        // bend handling (the reference reads one past the array's end at the
        // final grid point).
        let request = if std::env::var_os("WORLD_ORACLE_NO_BEND").is_some() {
            let mut r = request.clone();
            r.pitch_bend.clear();
            r
        } else {
            request.clone()
        };
        let request = &request;

        // ---- C++ Resample ----
        let so_out = call_so_resample(&so_resample, request, &mono_f64, wav.sample_rate, frq.as_deref());

        // ---- Rust resampler ----
        let rs_out = worldline_rs::resampler::resample(
            request,
            mono_f64.clone(),
            wav.sample_rate as i32,
            frq.as_deref(),
        )?;

        total_so += so_out.len();
        total_rs += rs_out.len();
        let n = so_out.len().min(rs_out.len());
        let mut max_diff = 0.0f32;
        for i in 0..n {
            max_diff = max_diff.max((so_out[i] - rs_out[i]).abs());
        }
        worst = worst.max(max_diff);
        compared += n;
        println!(
            "  [{index}] {:<4} so={:>7} rs={:>7} samples  max|diff|={max_diff:.3e}  \
             [vol={:.1} off={:.2} req={:.1} cut={:.2} con={:.1} cv={:.1} tone={} bend={} \
              g={} P={} Mt={} Mb={} Mv={} tempo={:.1}]",
            request.phoneme,
            so_out.len(),
            rs_out.len(),
            request.volume,
            request.offset,
            request.required_length,
            request.cut_off,
            request.consonant,
            request.con_vel,
            request.tone,
            request.pitch_bend.len(),
            request.flag_g,
            request.flag_P,
            request.flag_Mt,
            request.flag_Mb,
            request.flag_Mv,
            request.tempo
        );
    }
    println!(
        "  total: so={total_so} rs={total_rs} samples compared={compared}  worst max|diff|={worst:.3e}"
    );
    if json {
        println!(
            "{{\"stage\":\"classic\",\"requests\":{},\"so_samples\":{total_so},\"rs_samples\":{total_rs},\"compared\":{compared},\"max_abs_diff\":{worst:.9}}}",
            requests.len()
        );
    }
    Ok(())
}

/// Fill the C `SynthRequest` and call the reference `Resample`.
fn call_so_resample(
    resample: &libloading::Symbol<
        unsafe extern "C" fn(*const worldline_sys::SynthRequest, *mut *mut f32) -> i32,
    >,
    request: &worldline_plugin::PhonemeRequest,
    samples: &[f64],
    sample_rate: u32,
    frq: Option<&[u8]>,
) -> Vec<f32> {
    let (frq_len, frq_ptr) = match frq {
        Some(f) => (
            f.len() as i32,
            f.as_ptr() as *const std::os::raw::c_char as *mut std::os::raw::c_char,
        ),
        None => (0, std::ptr::null_mut()),
    };
    let synth = worldline_sys::SynthRequest {
        sample_fs: sample_rate as i32,
        sample_length: samples.len() as i32,
        sample: samples.as_ptr() as *mut f64,
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
    let n = unsafe { resample(&synth, &mut y) };
    if n <= 0 || y.is_null() {
        return Vec::new();
    }
    let out = unsafe { std::slice::from_raw_parts(y, n as usize) }.to_vec();
    unsafe { worldline_sys::free_buffer(y) };
    out
}

/// `{stem}_wav.frq` next to a wav path, when present.
fn read_frq_next_to(wav_path: &str) -> Option<Vec<u8>> {
    let path = Path::new(wav_path);
    let stem = path.file_stem()?.to_str()?;
    let frq = path.with_file_name(format!("{stem}_wav.frq"));
    let data = std::fs::read(frq).ok()?;
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

/// Full-phrase comparison: build the phrase inputs from a real project and
/// render each phrase through both the C++ `.so` (`PhraseSynth`) and the
/// pure-Rust `worldline-rs` phrase path.
fn run_phrase(
    project_path: &Path,
    voicebank_path: &Path,
    so_path: &Path,
    json: bool,
) -> Result<(), String> {
    let project = synth_cli::pipeline::load_project(project_path)
        .map_err(|e| format!("load project: {e}"))?;
    let voicebank = synth_cli::pipeline::load_voicebank(voicebank_path)
        .map_err(|e| format!("load voicebank: {e}"))?;
    let track = project.tracks.first().ok_or("project has no tracks")?;
    let part = project
        .parts
        .iter()
        .find_map(|p| match p {
            UPart::Voice(v) if v.track_no == 0 => Some(v),
            _ => None,
        })
        .ok_or("track 0 has no voice part")?;

    let phrases = synth_cli::pipeline::build_phrase_inputs(
        &project,
        track,
        part,
        &voicebank,
        synth_cli::pipeline::PhonemizerKind::Universal,
    )?;
    println!(
        "phrase: {} | {} phrase(s)",
        project.name,
        phrases.len()
    );

    let so_renderer = worldline_plugin::WorldlineRenderer::open(so_path)
        .map_err(|e| format!("open {}: {e}", so_path.display()))?;
    // The plugin's curve-sampling density (what the `.so` path passes to
    // SetCurves); the model grid comes from the C++ FrameMs() default
    // inside worldline-rs.
    let curve_frame_ms = std::env::var("WORLD_FRAME_MS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(worldline_plugin::DEFAULT_FRAME_MS);

    let mut all_so: Vec<f32> = Vec::new();
    let mut all_rs: Vec<f32> = Vec::new();
    for (index, phrase) in phrases.iter().enumerate() {
        let so_out = worldline_plugin::WorldlineRenderer::render_phrase(&so_renderer, &phrase.input)
            .map_err(|e| format!("so render: {e}"))?;
        let rs_out = worldline_rs::render_phrase(&phrase.input, curve_frame_ms)?;
        println!(
            "  phrase {index} @{:.0} ms: so={} samples ({:.1} ms)  rs={} samples ({:.1} ms)  diff={} samples",
            phrase.group.position_ms,
            so_out.len(),
            so_out.len() as f64 * 1000.0 / 44100.0,
            rs_out.len(),
            rs_out.len() as f64 * 1000.0 / 44100.0,
            rs_out.len() as isize - so_out.len() as isize
        );
        if index == 0 {
            print_pcm_stats("  phrase 0: so vs rs", &so_out, &rs_out);
        }
        all_so.extend_from_slice(&so_out);
        all_rs.extend_from_slice(&rs_out);
    }
    if phrases.len() > 1 {
        print_pcm_stats("  concat: so vs rs", &all_so, &all_rs);
    }

    if json {
        let s = pcm_stats(&all_so, &all_rs);
        println!(
            "{{\"stage\":\"phrase\",\"phrases\":{},\"so_samples\":{},\"rs_samples\":{},\"len_diff\":{},\"max_abs_diff\":{:.9},\"rms_diff\":{:.9}}}",
            phrases.len(),
            all_so.len(),
            all_rs.len(),
            all_rs.len() as isize - all_so.len() as isize,
            s.max_abs_diff,
            s.rms_diff
        );
    }
    Ok(())
}

/// Neutral expression curves: the C++ `WorldSynthesis` semantics with
/// gender=0.5 (no shift), tension=0.5 (coefficients 1.0), breathiness=0.5
/// (`0.5 > 0.5` is false → ×2 → 1.0), voicing=1.0 (unchanged).
struct NeutralCurves {
    gender: Vec<f64>,
    tension: Vec<f64>,
    breathiness: Vec<f64>,
    voicing: Vec<f64>,
}

impl NeutralCurves {
    fn new(frames: usize) -> Self {
        Self {
            gender: vec![0.5; frames],
            tension: vec![0.5; frames],
            breathiness: vec![0.5; frames],
            voicing: vec![1.0; frames],
        }
    }
}

fn so_synthesis(
    analysis: &worldline_sys::Analysis,
    f0: &[f64],
    sp: &[f64],
    ap: &[f64],
    sp_size: usize,
    frame_ms: f64,
    curves: &NeutralCurves,
    y_len: usize,
) -> Vec<f32> {
    let mut f0 = f0.to_vec();
    let mut sp = sp.to_vec();
    let mut ap = ap.to_vec();
    let mut g = curves.gender.clone();
    let mut t = curves.tension.clone();
    let mut b = curves.breathiness.clone();
    let mut v = curves.voicing.clone();
    let mut y: *mut f64 = std::ptr::null_mut();
    let n = unsafe {
        analysis.world_synthesis(
            &mut f0,
            &mut sp,
            sp_size as i32,
            &mut ap,
            FFT_SIZE,
            frame_ms,
            SAMPLE_RATE,
            &mut y,
            &mut g,
            &mut t,
            &mut b,
            &mut v,
        )
    };
    if n <= 0 || y.is_null() {
        return Vec::new();
    }
    let out: Vec<f32> = unsafe { std::slice::from_raw_parts(y, n as usize) }
        .iter()
        .map(|&s| s as f32)
        .collect();
    debug_assert!(n as usize == y_len);
    unsafe { worldline_sys::libc_free(y as *mut std::ffi::c_void) };
    out
}

fn rs_synthesis(
    features: &world_rs::WorldFeatures,
    frame_ms: f64,
    y_len: usize,
) -> Result<Vec<f32>, String> {
    let config = world_rs::synthesis::SynthesisConfig {
        sample_rate: SAMPLE_RATE as u32,
        frame_period_ms: frame_ms,
        fft_size: FFT_SIZE as usize,
        tension: 1.0,
        breathiness: 1.0,
        voicing: 1.0,
        output_length: Some(y_len),
        ..Default::default()
    };
    world_rs::synthesis::synthesize(features, config, &()).map_err(|e| format!("world-rs synth: {e:?}"))
}

/// F0 for the analysis: the voicebank's own `.frq` when present (the
/// reference always prefers it), resampled onto the DIO grid exactly like
/// the runtime path; otherwise the `.so` DIO estimator.
fn f0_for(mono: &[f32], wav: &Path, frame_ms: f64) -> Vec<f64> {
    let frames = (1000.0 * mono.len() as f64 / f64::from(SAMPLE_RATE) / frame_ms) as usize + 1;
    let frq_path = voicebank::frq_path_for_wav(wav);
    if let Ok(bytes) = std::fs::read(&frq_path) {
        if let Ok(frq) = voicebank::parse_frq(&bytes) {
            let mut out = vec![0.0f64; frames];
            if !frq.f0.is_empty() && frq.hop_size > 0 {
                let ratio = f64::from(HOP_SIZE) / f64::from(frq.hop_size);
                let floor = 3.0 * f64::from(SAMPLE_RATE) / (f64::from(FFT_SIZE) - 3.0);
                for (i, value) in out.iter_mut().enumerate() {
                    let i_f = i as f64;
                    let low = ((i_f * ratio).floor() as usize).min(frq.f0.len() - 1);
                    let high = (((i_f + 1.0) * ratio).ceil() as usize).min(frq.f0.len() - 1);
                    let mut sum = 0.0;
                    let mut count = 0;
                    for j in low..=high {
                        if frq.f0[j] > floor {
                            sum += frq.f0[j];
                            count += 1;
                        }
                    }
                    *value = if count > 0 { sum / f64::from(count) } else { 0.0 };
                }
            }
            return out;
        }
    }
    // No usable .frq → DIO through the reference .so (method 0).
    let so = std::env::var("WORLDLINE_SO").unwrap_or_else(|_| {
        default_so().to_string_lossy().into_owned()
    });
    match worldline_sys::Analysis::open(&so) {
        Ok(a) => a.f0(mono, SAMPLE_RATE, frame_ms, 0),
        Err(_) => vec![0.0; frames],
    }
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

struct LogStats {
    corr: f64,
    db_mae: f64,
    db_max: f64,
    mean_so_db: f64,
    mean_rs_db: f64,
}

fn log_spectrum_stats(so: &[f64], rs: &[f64], bins: usize) -> LogStats {
    let n = so.len().min(rs.len()) / bins * bins;
    let mut a = Vec::with_capacity(n);
    let mut b = Vec::with_capacity(n);
    let mut db_sum = 0.0;
    let mut db_max = 0.0f64;
    for i in 0..n {
        let x = so[i].max(1e-30);
        let y = rs[i].max(1e-30);
        a.push(x.ln());
        b.push(y.ln());
        let db = 20.0 * (y / x).log10();
        db_sum += db.abs();
        db_max = db_max.max(db.abs());
    }
    let mean_so = a.iter().sum::<f64>() / n.max(1) as f64 * 20.0 / std::f64::consts::LN_10;
    let mean_rs = b.iter().sum::<f64>() / n.max(1) as f64 * 20.0 / std::f64::consts::LN_10;
    LogStats {
        corr: pearson(&a, &b),
        db_mae: db_sum / n.max(1) as f64,
        db_max,
        mean_so_db: mean_so,
        mean_rs_db: mean_rs,
    }
}

struct RatioStats {
    corr: f64,
    mae: f64,
    max: f64,
}

fn ratio_stats(so: &[f64], rs: &[f64], bins: usize) -> RatioStats {
    let n = so.len().min(rs.len()) / bins * bins;
    let mut mae = 0.0;
    let mut max = 0.0f64;
    for i in 0..n {
        let d = (so[i] - rs[i]).abs();
        mae += d;
        max = max.max(d);
    }
    RatioStats {
        corr: pearson(&so[..n], &rs[..n]),
        mae: mae / n.max(1) as f64,
        max,
    }
}

fn print_log_stats(label: &str, s: &LogStats) {
    println!(
        "{label}: corr={:.12}  dB MAE={:.6}  dB max={:.6}  mean so={:.3} dB rs={:.3} dB",
        s.corr, s.db_mae, s.db_max, s.mean_so_db, s.mean_rs_db
    );
}

fn print_ratio_stats(label: &str, s: &RatioStats) {
    println!(
        "{label}: corr={:.12}  MAE={:.9}  max={:.9}",
        s.corr, s.mae, s.max
    );
}

struct PcmStats {
    corr: f64,
    best_lag: isize,
    best_corr: f64,
    rms_ratio: f64,
    rms_so: f64,
    rms_rs: f64,
    duration_diff_ms: f64,
    max_abs_diff: f32,
    rms_diff: f64,
    rms_diff_dbfs: f64,
}

fn pcm_stats(so: &[f32], rs: &[f32]) -> PcmStats {
    let rms = |x: &[f32]| -> f64 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    };
    let n = so.len().min(rs.len());
    let corr = pearson_f32(&so[..n], &rs[..n]);
    // Coarse alignment search (±4 ms) — the two paths may place the first
    // pulse one sample apart when the frame grid rounds differently.
    let mut best_lag = 0isize;
    let mut best_corr = corr;
    let max_lag = (SAMPLE_RATE as usize / 250).min(n / 4);
    for lag in -(max_lag as isize)..=(max_lag as isize) {
        let c = if lag >= 0 {
            let l = lag as usize;
            if n <= l {
                continue;
            }
            pearson_f32(&so[..n - l], &rs[l..n])
        } else {
            let l = (-lag) as usize;
            if n <= l {
                continue;
            }
            pearson_f32(&so[l..n], &rs[..n - l])
        };
        if c > best_corr {
            best_corr = c;
            best_lag = lag;
        }
    }
    let rms_so = rms(so);
    let rms_rs = rms(rs);
    let mut max_abs_diff = 0.0f32;
    let mut sq = 0.0f64;
    for i in 0..n {
        let d = (rs[i] - so[i]).abs();
        max_abs_diff = max_abs_diff.max(d);
        sq += f64::from(d) * f64::from(d);
    }
    let rms_diff = (sq / n.max(1) as f64).sqrt();
    PcmStats {
        corr,
        best_lag,
        best_corr,
        rms_ratio: if rms_so > 0.0 { rms_rs / rms_so } else { 0.0 },
        rms_so,
        rms_rs,
        duration_diff_ms: (rs.len() as f64 - so.len() as f64) * 1000.0 / f64::from(SAMPLE_RATE),
        max_abs_diff,
        rms_diff,
        rms_diff_dbfs: if rms_diff > 0.0 { 20.0 * rms_diff.log10() } else { f64::NEG_INFINITY },
    }
}

fn print_pcm_stats(label: &str, so: &[f32], rs: &[f32]) {
    let s = pcm_stats(so, rs);
    println!(
        "{label}:\n    corr={:.12}  best-lag corr={:.12} @ {}  rms so={:.8} rs={:.8} (x{:.8})\n    max|diff|={:.3e}  rms(diff)={:.3e} ({:.1} dBFS)  len diff={:.2} ms",
        s.corr, s.best_corr, s.best_lag, s.rms_so, s.rms_rs, s.rms_ratio,
        s.max_abs_diff, s.rms_diff, s.rms_diff_dbfs, s.duration_diff_ms
    );
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let ma = a[..n].iter().sum::<f64>() / n as f64;
    let mb = b[..n].iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for i in 0..n {
        let x = a[i] - ma;
        let y = b[i] - mb;
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        return 0.0;
    }
    num / (da.sqrt() * db.sqrt())
}

fn pearson_f32(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let af: Vec<f64> = a[..n].iter().map(|v| f64::from(*v)).collect();
    let bf: Vec<f64> = b[..n].iter().map(|v| f64::from(*v)).collect();
    pearson(&af, &bf)
}
