//! synth-server: a LAN HTTP API over the synth-cli render pipeline.
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
use synth_server::voicebanks;

#[derive(Parser)]
#[command(
    name = "synth-server",
    version,
    about = "LAN HTTP API over the worldline render pipeline (Sprint 2.4.1)"
)]
struct Cli {
    /// Path to libworldline.so (omit to run without the engine;
    /// /synth-note and /render then return 500).
    #[arg(long)]
    so: Option<PathBuf>,
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
    /// TCP port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Address to bind (0.0.0.0 exposes the API on the LAN).
    #[arg(long, default_value = "0.0.0.0")]
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
    let renderer = match &cli.so {
        Some(path) => {
            let service = RenderService::spawn(path.clone(), cli.mixer_so.clone(), cli.mixer_params.clone())
                .map_err(|e| format!("open {}: {e}", path.display()))?;
            println!("loaded renderer from {}", path.display());
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

    let state = Arc::new(AppState::new(cli.voicebanks.clone(), scan.entries, renderer));
    let bind: IpAddr = cli
        .bind
        .parse()
        .map_err(|e| format!("invalid --bind '{}': {e}", cli.bind))?;
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
