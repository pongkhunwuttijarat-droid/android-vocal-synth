//! synth-server: a local HTTP API over the synth-cli render pipeline.
//!
//! ```sh
//! synth-server --so build/build-linux/libworldline.so \
//!     --voicebanks native/test-data --port 8080 [--bind 0.0.0.0]
//! ```
//!
//! `--voicebanks` is a directory of voicebank subdirectories (each one
//! becomes a `/voicebanks` entry) or a single voicebank directory
//! itself. `--so` is optional: without it the server still serves
//! `/health`, `/capabilities`, `/voicebanks` and `/stats`, and reports
//! `so_loaded: false`; `/synth-note` and `/render` then fail with 500.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use synth_server::render_service::RenderService;
use synth_server::server;
use synth_server::state::AppState;
use synth_server::stream_ws::StreamAuth;
use synth_server::voicebanks;

#[derive(Parser)]
#[command(
    name = "synth-server",
    version,
    about = "Local HTTP API over the worldline render pipeline (Sprint 2.4.1)"
)]
struct Cli {
    /// Path to libworldline.so (omit to run without the engine;
    /// /synth-note and /render then return 500).
    #[arg(long)]
    so: Option<PathBuf>,
    /// R2 neural engine: path to mel.onnx (with --vocoder-onnx switches
    /// /render from WORLDLINE-R to WORLDLINE-R2).
    #[arg(long)]
    mel_onnx: Option<PathBuf>,
    /// R2 neural engine: path to pc-nsf-hifigan model.onnx.
    #[arg(long)]
    vocoder_onnx: Option<PathBuf>,
    /// DiffSinger engine: path to the voicebank dir with dsconfig.yaml
    /// (switches /render to the DiffSinger engine).
    #[arg(long)]
    ds_model: Option<PathBuf>,
    /// MoE student engine: checkpoint prefix (the general/experts .mpk
    /// prefix + `{prefix}-phonemes.txt` sidecar). Enables the "moe" engine.
    #[arg(long)]
    moe_prefix: Option<PathBuf>,
    /// MoE engine: pc-nsf-hifigan vocoder path (the same onnx the r2
    /// chain uses, but the checkpoint prefix decides the model).
    #[arg(long)]
    moe_vocoder: Option<PathBuf>,
    /// Path to libmixerfx.so (optional FX chain on the final mix).
    #[arg(long)]
    mixer_so: Option<PathBuf>,
    /// JSON params for the mixer FX chain (e.g.
    /// '{"clip_enabled":0}' for passthrough).
    #[arg(long, default_value = "{}")]
    mixer_params: String,
    /// Directory containing voicebank subdirectories — or a single
    /// voicebank directory itself.
    #[arg(long)]
    voicebanks: PathBuf,
    /// Directory where the server stores project files (ALL project data
    /// lives server-side — the app saves/loads via HTTP).
    #[arg(long, default_value = "projects")]
    projects: PathBuf,
    /// Root that a wave (audio) part's `relative_path` is resolved against —
    /// normally the folder holding the `.ustx`.
    ///
    /// Omit it and a track's audio parts are reported as skipped rather than
    /// silently dropped; the rest of the render is unaffected.
    #[arg(long)]
    media: Option<PathBuf>,
    /// TCP port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Address to bind (loopback by default; use 0.0.0.0 only for explicit LAN hosting).
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let bind: IpAddr = cli
        .bind
        .parse()
        .map_err(|e| format!("invalid --bind '{}': {e}", cli.bind))?;
    let stream_auth = match (
        std::env::var("RUSTED_UTAU_STREAM_CREDENTIAL").ok(),
        std::env::var("RUSTED_UTAU_STREAM_ALLOWED_ORIGINS").ok(),
    ) {
        (Some(credential), Some(origins)) => Some(
            StreamAuth::new(
                credential,
                origins.split(',').map(str::trim).map(str::to_string),
            )
            .map_err(|e| format!("invalid WebSocket auth configuration: {e}"))?,
        ),
        (None, Some(origins)) => {
            if !bind.is_loopback() {
                return Err(
                    "browser credential bootstrap requires a loopback --bind address".into(),
                );
            }
            Some(
                StreamAuth::browser_bootstrap(
                    origins.split(',').map(str::trim).map(str::to_string),
                )
                .map_err(|e| format!("invalid WebSocket origin configuration: {e}"))?,
            )
        }
        (Some(_), None) => {
            return Err("RUSTED_UTAU_STREAM_CREDENTIAL requires RUSTED_UTAU_STREAM_ALLOWED_ORIGINS".into())
        }
        (None, None) => None,
    };
    // A media root that does not exist is NOT harmless: every wave part then
    // fails deep inside a render job with "No such file or directory", which
    // reads like a broken audio track rather than a missing fixture directory
    // (a `cargo clean` or a fresh checkout wipes `target/`, where the demo
    // fixture lives). Say it at boot, with the command that rebuilds it.
    if let Some(media) = &cli.media {
        if !media.is_dir() {
            eprintln!(
                "warning: --media {} does not exist; wave parts will fail to load. \
                 Rebuild the demo fixture with `python3 scripts/make-audio-track-demo.py`.",
                media.display()
            );
        }
    }
    let renderer = match &cli.so {
        Some(path) => {
            let service = RenderService::spawn(
                path.clone(),
                cli.mixer_so.clone(),
                cli.mixer_params.clone(),
                cli.mel_onnx.clone(),
                cli.vocoder_onnx.clone(),
                cli.ds_model.clone(),
                cli.moe_prefix.clone(),
                cli.moe_vocoder.clone(),
                cli.media.clone(),
            )
            .map_err(|e| format!("open {}: {e}", path.display()))?;
            println!("loaded renderer from {}", path.display());
            if cli.mel_onnx.is_some() && cli.vocoder_onnx.is_some() {
                println!("R2 neural engine enabled (mel.onnx + pc-nsf-hifigan)");
            }
            if let Some(ds) = &cli.ds_model {
                println!("DiffSinger engine enabled ({})", ds.display());
            }
            match &cli.mixer_so {
                Some(m) => println!("loaded mixer FX from {}", m.display()),
                None => println!("no mixer FX plugin (--mixer-so not given)"),
            }
            Some(service)
        }
        None => {
            eprintln!(
                "warning: --so not given — /health reports so_loaded=false and \
                 /synth-note + /render return 500"
            );
            None
        }
    };

    let scan = voicebanks::scan_voicebanks(&cli.voicebanks)?;
    for warning in &scan.warnings {
        eprintln!("warning: {warning}");
    }
    if scan.entries.is_empty() {
        eprintln!(
            "warning: no voicebanks found under {}",
            cli.voicebanks.display()
        );
    }
    for entry in &scan.entries {
        println!(
            "voicebank: '{}' (dir {}, {} aliases, {} wavs, {} Hz)",
            entry.info.name,
            entry.info.dir,
            entry.info.aliases_count,
            entry.info.wav_count,
            entry
                .info
                .samples_rate
                .map_or("?".to_string(), |r| r.to_string())
        );
    }

    let mut app_state = AppState::new(
        cli.voicebanks.clone(),
        cli.projects.clone(),
        scan.entries,
        renderer,
    );
    if let Some(auth) = stream_auth {
        app_state = app_state.with_stream_auth(auth);
    }
    let state = Arc::new(app_state);
    // Server-side project storage must exist before any save.
    state.projects.ensure().map_err(|e| e.message())?;
    let addr = SocketAddr::new(bind, cli.port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    println!(
        "synth-server {} listening on http://{addr} (voicebanks root: {})",
        env!("CARGO_PKG_VERSION"),
        cli.voicebanks.display()
    );

    axum::serve(listener, server::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server error: {e}"))?;
    println!("synth-server stopped");
    Ok(())
}

/// Wait for Ctrl+C or SIGTERM, then let in-flight requests drain.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => println!("received Ctrl+C, shutting down"),
        _ = terminate => println!("received SIGTERM, shutting down"),
    }
}
