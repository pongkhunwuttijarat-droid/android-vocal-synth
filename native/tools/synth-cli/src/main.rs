//! synth-cli commands: `render` and `synth-note`.
//!
//! ```sh
//! synth-cli render --project song.ustx --voicebank <dir> --so libworldline.so \
//!     --out out.wav [--track 0] [--phonemizer english|japanese] [--verbose]
//! synth-cli synth-note --voicebank <dir> --so libworldline.so \
//!     --phoneme <alias> --tone 60 --out note.wav [--duration-ms 500]
//! ```

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use synth_cli::pipeline::{self, PhonemizerKind, SAMPLE_RATE};
use wavwriter::{write_wav_16, write_wav_file};

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
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PhonemizerArg {
    English,
    Japanese,
}

#[derive(Args)]
struct RenderArgs {
    /// Path to the .ustx project file.
    #[arg(long)]
    project: PathBuf,
    /// Voicebank directory (must contain voice/oto.ini).
    #[arg(long)]
    voicebank: PathBuf,
    /// Path to libworldline.so.
    #[arg(long)]
    so: PathBuf,
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
    /// Print per-phrase progress.
    #[arg(long)]
    verbose: bool,
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
    }
}

fn cmd_render(args: RenderArgs) -> Result<(), String> {
    let project = pipeline::load_project(&args.project)?;
    let voicebank = pipeline::load_voicebank(&args.voicebank)?;
    let renderer = worldline_plugin::WorldlineRenderer::open(&args.so)
        .map_err(|e| format!("open {}: {e}", args.so.display()))?;

    let track = project
        .tracks
        .get(args.track as usize)
        .ok_or_else(|| format!("track {} not found", args.track))?;
    let kind = match args.phonemizer {
        Some(PhonemizerArg::English) => PhonemizerKind::English,
        Some(PhonemizerArg::Japanese) => PhonemizerKind::Japanese,
        None => PhonemizerKind::from_track(track),
    };
    if args.verbose {
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
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/lilt-cache"));
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
            &renderer,
            args.track,
            kind,
            args.verbose,
            &mut cache,
            mixer.as_mut(),
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
