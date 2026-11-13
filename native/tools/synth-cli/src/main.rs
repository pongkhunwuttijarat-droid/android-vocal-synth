//! synth-cli commands: `render` and `synth-note`.
//!
//! ```sh
//! synth-cli render --project song.ustx --voicebank <dir> --so libworldline.so \
//!     --out out.wav [--track 0] [--phonemizer english|japanese|universal|default] [--verbose]
//! synth-cli synth-note --voicebank <dir> --so libworldline.so \
//!     --phoneme <alias> --tone 60 --out note.wav [--duration-ms 500]
//! synth-cli mixdown --project song.ustx --voicebank <dir> --so libworldline.so \
//!     --out mix.wav [--media-root <dir>]
//! ```
//!
//! `render` is the per-track export (OpenUtau's "Export each track");
//! `mixdown` is the whole-project one (OpenUtau's "Export mixdown"): every
//! track through its fader plus every wave (audio) part, summed into a single
//! mono 44.1 kHz stream.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use synth_cli::diffusion::DiffusionEngine;
use synth_cli::engine::{DsEngine, WorldlineClassicEngine, WorldlineR2Engine, WorldlineRsEngine};
use synth_cli::moe::MoeEngine;
use synth_cli::pipeline::{self, PhonemizerKind, SAMPLE_RATE};
use wavwriter::{write_wav_16, write_wav_16_channels, write_wav_file};

#[derive(Parser)]
#[command(
    name = "synth-cli",
    version,
    about = "Render .ustx projects and single notes through libworldline.so to WAV (Sprint 2.2 first-sound milestone)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a .ustx project with a voicebank through libworldline.so.
    Render(RenderArgs),
    /// Render one phoneme/note as a quick first-sound test.
    SynthNote(SynthNoteArgs),
    /// Render every track plus every wave part into one mixed WAV.
    Mixdown(MixdownArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PhonemizerArg {
    English,
    Japanese,
    Universal,
    /// OpenUtau DefaultPhonemizer parity: one note -> one phoneme = the lyric
    /// verbatim (the oto alias the bank reports). A note whose lyric has no oto
    /// alias emits no phoneme, mirroring OpenUtau's Error + FromPart filter.
    Default,
}

/// Engine selection options, shared by `render` and `mixdown`.
///
/// One definition so the two subcommands cannot drift apart on which engines
/// exist or what files an engine needs.
#[derive(Args, Clone)]
struct EngineFiles {
    /// Path to libworldline.so.
    #[arg(long)]
    so: PathBuf,
    /// Render engine: r1 = WORLDLINE-R (WORLD resynthesis, default),
    /// r2 = WORLDLINE-R2 (neural mel→HiFiGAN; needs --mel-onnx and
    /// --vocoder-onnx), classic = WORLDLINE-R Classic (SynthFeatures →
    /// C++ WorldSynthesis — the reference v1 feature path),
    /// r1-rs = WORLDLINE-R on the pure-Rust kernel (worldline-rs, no .so),
    /// diffsinger = diffusion acoustic model (needs --ds-model).
    #[arg(long, value_enum, default_value_t = EngineArg::R1)]
    engine: EngineArg,
    /// R2 mel.onnx model path (only with --engine r2).
    #[arg(long)]
    mel_onnx: Option<PathBuf>,
    /// R2 pc-nsf-hifigan onnx model path (only with --engine r2).
    #[arg(long)]
    vocoder_onnx: Option<PathBuf>,
    /// DiffSinger voicebank dir with dsconfig.yaml (only with
    /// --engine diffsinger).
    #[arg(long)]
    ds_model: Option<PathBuf>,
    /// MoE engine: checkpoint prefix (the general/experts .mpk prefix +
    /// `{prefix}-phonemes.txt` sidecar) — only with --engine moe.
    #[arg(long)]
    moe_prefix: Option<PathBuf>,
    /// MoE engine: pc-nsf-hifigan onnx (only with --engine moe).
    #[arg(long)]
    moe_vocoder: Option<PathBuf>,
    /// MoE engine: general-expert hidden width (default 1024; match the
    /// checkpoint's training --hidden or load fails).
    #[arg(long, default_value_t = moe_train::baseline::DEFAULT_HIDDEN)]
    moe_hidden: usize,
    /// MoE engine: general-expert depth (default 1; match training).
    #[arg(long, default_value_t = moe_train::baseline::DEFAULT_DEPTH)]
    moe_depth: usize,
    /// Diffusion engine: fresh frozen base checkpoint prefix.
    #[arg(long)]
    diffusion_base: Option<PathBuf>,
    /// Diffusion engine: denoiser checkpoint prefix and vocab sidecar.
    #[arg(long)]
    diffusion_prefix: Option<PathBuf>,
    /// Diffusion engine: pc-nsf-hifigan ONNX model.
    #[arg(long)]
    diffusion_vocoder: Option<PathBuf>,
    /// Diffusion engine base width (match the base training --hidden).
    #[arg(long, default_value_t = moe_train::baseline::SINGLE_SINGER_BASE_HIDDEN)]
    diffusion_base_hidden: usize,
    /// Diffusion engine base depth (match the base training --depth).
    #[arg(long, default_value_t = moe_train::baseline::SINGLE_SINGER_BASE_DEPTH)]
    diffusion_base_depth: usize,
    /// DDIM inference steps (2..=64; 64 matches heldout evaluation).
    #[arg(long, default_value_t = 64)]
    diffusion_steps: usize,
    /// Print per-phrase progress.
    #[arg(long)]
    verbose: bool,
}

#[derive(Args)]
struct RenderArgs {
    #[command(flatten)]
    engine_files: EngineFiles,
    /// Path to the .ustx project file.
    #[arg(long)]
    project: PathBuf,
    /// Voicebank directory (must contain voice/oto.ini).
    #[arg(long)]
    voicebank: PathBuf,
    /// Optional mixer FX plugin (libmixerfx.so) — processed after mixing.
    #[arg(long)]
    mixer: Option<PathBuf>,
    /// Mixer FX params JSON (e.g. {"gain":1.0,"clip_enabled":1}).
    #[arg(long, default_value = "{}")]
    mixer_params: String,
    /// Output .wav path.
    #[arg(long)]
    out: PathBuf,
    /// Track index to render.
    #[arg(long, default_value_t = 0)]
    track: i32,
    /// Phonemizer override (default: picked from the track's setting).
    #[arg(long, value_enum)]
    phonemizer: Option<PhonemizerArg>,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum EngineArg {
    R1,
    #[value(name = "r1-rs")]
    R1Rs,
    R2,
    Classic,
    #[value(name = "diffsinger")]
    DiffSinger,
    #[value(name = "moe")]
    Moe,
    #[value(name = "diffusion")]
    Diffusion,
}

#[derive(Args)]
struct SynthNoteArgs {
    /// Voicebank directory (must contain voice/oto.ini).
    #[arg(long)]
    voicebank: PathBuf,
    /// Path to libworldline.so.
    #[arg(long)]
    so: PathBuf,
    /// Oto alias to synthesize (e.g. "A" for the mock bank).
    #[arg(long)]
    phoneme: String,
    /// MIDI note number (60 = C4).
    #[arg(long)]
    tone: i32,
    /// Output .wav path.
    #[arg(long)]
    out: PathBuf,
    /// Requested note duration in ms.
    #[arg(long, default_value_t = 500.0)]
    duration_ms: f64,
}

#[derive(Args)]
struct MixdownArgs {
    /// Path to the .ustx project file.
    #[arg(long)]
    project: PathBuf,
    /// Voicebank directory (must contain voice/oto.ini).
    #[arg(long)]
    voicebank: PathBuf,
    /// Output .wav path.
    #[arg(long)]
    out: PathBuf,
    /// Where wave parts resolve their `relative_path`.
    /// Defaults to the folder holding the .ustx, which is what OpenUtau does.
    #[arg(long)]
    media_root: Option<PathBuf>,
    /// Optional mixer FX plugin (libmixerfx.so) — processed after mixing.
    #[arg(long)]
    mixer: Option<PathBuf>,
    /// Mixer FX params JSON (e.g. {"gain":1.0,"clip_enabled":1}).
    #[arg(long, default_value = "{}")]
    mixer_params: String,
    /// Phonemizer override (default: each track's own setting).
    #[arg(long, value_enum)]
    phonemizer: Option<PhonemizerArg>,
    #[command(flatten)]
    engine_files: EngineFiles,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Render(args) => cmd_render(args),
        Command::SynthNote(args) => cmd_synth_note(args),
        Command::Mixdown(args) => cmd_mixdown(args),
    }
}

/// Build the phrase renderer the CLI was asked for.
fn open_engine(files: EngineFiles) -> Result<Box<dyn pipeline::PhraseRenderer>, String> {
    // Engine selection: r1 = WORLDLINE-R (WORLD resynthesis via
    // libworldline.so), r2 = WORLDLINE-R2 (neural mel→HiFiGAN, needs
    // the ONNX models). Both render the SAME voicebank samples.
    let engine: Box<dyn pipeline::PhraseRenderer> = match files.engine {
        EngineArg::R1 => Box::new(
            worldline_plugin::WorldlineRenderer::open(&files.so)
                .map_err(|e| format!("open {}: {e}", files.so.display()))?,
        ),
        EngineArg::R1Rs => Box::new(
            WorldlineRsEngine::open(files.verbose)
                .map_err(|e| format!("open r1-rs engine: {e}"))?,
        ),
        EngineArg::R2 => {
            let mel = files.mel_onnx.ok_or("--engine r2 requires --mel-onnx")?;
            let vocoder = files
                .vocoder_onnx
                .ok_or("--engine r2 requires --vocoder-onnx")?;
            Box::new(
                WorldlineR2Engine::open(&files.so, mel, vocoder, files.verbose)
                    .map_err(|e| format!("open r2 engine: {e}"))?,
            )
        }
        EngineArg::Classic => Box::new(
            WorldlineClassicEngine::open(&files.so, files.verbose)
                .map_err(|e| format!("open classic engine: {e}"))?,
        ),
        EngineArg::DiffSinger => {
            let model = files
                .ds_model
                .ok_or("--engine diffsinger requires --ds-model <dir>")?;
            Box::new(
                DsEngine::open(model, files.verbose)
                    .map_err(|e| format!("open diffsinger engine: {e}"))?,
            )
        }
        EngineArg::Moe => {
            let prefix = files
                .moe_prefix
                .ok_or("--engine moe requires --moe-prefix <checkpoint-prefix>")?;
            let vocoder = files
                .moe_vocoder
                .ok_or("--engine moe requires --moe-vocoder <pc-nsf-hifigan.onnx>")?;
            Box::new(
                MoeEngine::open(
                    prefix,
                    vocoder,
                    files.moe_hidden,
                    files.moe_depth,
                    files.verbose,
                )
                .map_err(|e| format!("open moe engine: {e}"))?,
            )
        }
        EngineArg::Diffusion => {
            let base = files
                .diffusion_base
                .ok_or("--engine diffusion requires --diffusion-base <checkpoint-prefix>")?;
            let prefix = files
                .diffusion_prefix
                .ok_or("--engine diffusion requires --diffusion-prefix <checkpoint-prefix>")?;
            let vocoder = files
                .diffusion_vocoder
                .ok_or("--engine diffusion requires --diffusion-vocoder <pc-nsf-hifigan.onnx>")?;
            Box::new(
                DiffusionEngine::open(
                    base,
                    prefix,
                    vocoder,
                    files.diffusion_base_hidden,
                    files.diffusion_base_depth,
                    files.diffusion_steps,
                    files.verbose,
                )
                .map_err(|e| format!("open diffusion engine: {e}"))?,
            )
        }
    };
    Ok(engine)
}

fn cmd_render(args: RenderArgs) -> Result<(), String> {
    let project = pipeline::load_project(&args.project)?;
    let voicebank = pipeline::load_voicebank(&args.voicebank)?;

    let engine = open_engine(args.engine_files.clone())?;

    let track = project
        .tracks
        .get(args.track as usize)
        .ok_or_else(|| format!("track {} not found", args.track))?;
    let kind = match args.phonemizer {
        Some(PhonemizerArg::English) => PhonemizerKind::English,
        Some(PhonemizerArg::Japanese) => PhonemizerKind::Japanese,
        Some(PhonemizerArg::Universal) => PhonemizerKind::Universal,
        Some(PhonemizerArg::Default) => PhonemizerKind::Default,
        None => PhonemizerKind::from_track(track),
    };
    if args.engine_files.verbose {
        println!(
            "project: {} | track {} ('{}') | phonemizer: {:?}",
            project.name, args.track, track.track_name, kind
        );
    }

    let report = {
        // Render cache: `<voicebank>/../cache` (OpenUtau-style res-{hash}
        // files). 64 MB in-memory budget; disk persists across runs so an
        // unchanged project renders near-instantly on Play/Export.
        let cache_dir = args
            .voicebank
            .parent()
            .map(|p| p.join("cache"))
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/rusted-utau-cache"));
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("cache dir {}: {e}", cache_dir.display()))?;
        let cache = runtime::RenderCache::new_in_dir(cache_dir, 64 << 20);
        let mut cache = Some(cache);
        // Optional mixer FX plugin — processed on the final mixed samples.
        let mut mixer = match &args.mixer {
            Some(so) => Some(
                mixer_fx::MixerFx::open(so, &args.mixer_params)
                    .map_err(|e| format!("mixer plugin: {e}"))?,
            ),
            None => None,
        };
        pipeline::render_project(
            &project,
            &voicebank,
            engine.as_ref(),
            args.track,
            kind,
            args.engine_files.verbose,
            &mut cache,
            mixer.as_mut().map(|m| m as &mut dyn pipeline::PostMixFx),
            None,
        )?
    };
    for reason in &report.skipped {
        println!("skipped: {reason}");
    }
    if report.samples.is_empty() {
        return Err(format!(
            "no audio produced: all {} phrase(s) were skipped (see above)",
            report.skipped.len()
        ));
    }

    let wav = write_wav_16(&report.samples, SAMPLE_RATE);
    write_wav_file(&args.out, &wav).map_err(|e| format!("write {}: {e}", args.out.display()))?;
    let peak = report
        .samples
        .iter()
        .fold(0.0f32, |peak, &s| peak.max(s.abs()));
    let duration_ms = report.samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    println!(
        "rendered {} phrase(s), {} skipped",
        report.phrases_rendered,
        report.skipped.len()
    );
    println!(
        "samples: {} ({:.1} ms @ {} Hz), peak: {:.4}",
        report.samples.len(),
        duration_ms,
        SAMPLE_RATE,
        peak
    );
    println!("wrote {} ({} bytes)", args.out.display(), wav.len());
    Ok(())
}

fn cmd_mixdown(args: MixdownArgs) -> Result<(), String> {
    let project = pipeline::load_project(&args.project)?;
    let voicebank = pipeline::load_voicebank(&args.voicebank)?;
    let engine = open_engine(args.engine_files.clone())?;

    // Wave-part media resolves against the project folder by default, exactly
    // like `UWavePart.AfterLoad`. `restricted` is deliberately NOT the default:
    // real `.ustx` files carry paths written on another machine that leave the
    // project folder (the attached fixture's wave part points at
    // `../../../../../home/<user>/...`). Hosts that render untrusted projects
    // should use `FileMediaSource::restricted` instead.
    let media_root = args
        .media_root
        .clone()
        .or_else(|| args.project.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let media = media::FileMediaSource::new(&media_root);

    let report = {
        let cache_dir = args
            .voicebank
            .parent()
            .map(|p| p.join("cache"))
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/rusted-utau-cache"));
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("cache dir {}: {e}", cache_dir.display()))?;
        let cache = runtime::RenderCache::new_in_dir(cache_dir, 64 << 20);
        let mut cache = Some(cache);
        let mut mixer = match &args.mixer {
            Some(so) => Some(
                mixer_fx::MixerFx::open(so, &args.mixer_params)
                    .map_err(|e| format!("mixer plugin: {e}"))?,
            ),
            None => None,
        };
        pipeline::render_mixdown(
            &project,
            &voicebank,
            engine.as_ref(),
            // A mixdown spans tracks, so there is no single track to read the
            // phonemizer from; each track's own setting is used unless the user
            // overrides it with --phonemizer.
            phonemizer_kind(args.phonemizer),
            args.engine_files.verbose,
            &mut cache,
            &media,
            mixer.as_mut().map(|m| m as &mut dyn pipeline::PostMixFx),
            None,
            None,
        )?
    };

    for track in &report.tracks {
        if !track.skipped.is_empty() {
            for reason in &track.skipped {
                println!("skipped: {reason}");
            }
        }
    }
    if report.samples.is_empty() {
        return Err("mixdown produced no audio".to_string());
    }
    let wav = write_wav_16_channels(&report.samples, report.sample_rate, report.channels);
    write_wav_file(&args.out, &wav).map_err(|e| format!("write {}: {e}", args.out.display()))?;
    let peak = report
        .samples
        .iter()
        .fold(0.0f32, |peak, &s| peak.max(s.abs()));
    let frames = report.samples.len() / report.channels.max(1) as usize;
    let duration_ms = frames as f64 * 1000.0 / f64::from(report.sample_rate);
    for track in &report.tracks {
        println!(
            "track {}: {} phrase(s), {} wave part(s)",
            track.track_no, track.phrases_rendered, track.wave_parts_mixed
        );
    }
    println!(
        "samples: {} ({} channels; {duration_ms:.1} ms @ {} Hz), peak: {peak:.4}",
        report.samples.len(),
        report.channels,
        report.sample_rate
    );
    println!(
        "wrote {} (media root: {})",
        args.out.display(),
        media_root.display()
    );
    Ok(())
}

/// The `--phonemizer` override, or `None` to let each track decide for itself.
fn phonemizer_kind(requested: Option<PhonemizerArg>) -> Option<pipeline::PhonemizerKind> {
    Some(match requested? {
        PhonemizerArg::English => pipeline::PhonemizerKind::English,
        PhonemizerArg::Japanese => pipeline::PhonemizerKind::Japanese,
        PhonemizerArg::Universal => pipeline::PhonemizerKind::Universal,
        PhonemizerArg::Default => pipeline::PhonemizerKind::Default,
    })
}

fn cmd_synth_note(args: SynthNoteArgs) -> Result<(), String> {
    let voicebank = pipeline::load_voicebank(&args.voicebank)?;
    let renderer = worldline_plugin::WorldlineRenderer::open(&args.so)
        .map_err(|e| format!("open {}: {e}", args.so.display()))?;

    let samples = pipeline::synth_note(
        &voicebank,
        &renderer,
        &args.phoneme,
        args.tone,
        args.duration_ms,
    )?;
    if samples.is_empty() {
        return Err("renderer produced no samples".into());
    }

    let wav = write_wav_16(&samples, SAMPLE_RATE);
    write_wav_file(&args.out, &wav).map_err(|e| format!("write {}: {e}", args.out.display()))?;
    let peak = samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()));
    let duration_ms = samples.len() as f64 * 1000.0 / f64::from(SAMPLE_RATE);
    println!(
        "phoneme '{}' @ tone {}, requested {:.0} ms",
        args.phoneme, args.tone, args.duration_ms
    );
    println!(
        "samples: {} ({:.1} ms @ {} Hz), peak: {:.4}",
        samples.len(),
        duration_ms,
        SAMPLE_RATE,
        peak
    );
    println!("wrote {} ({} bytes)", args.out.display(), wav.len());
    Ok(())
}
