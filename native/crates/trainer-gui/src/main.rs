//! trainer-gui binary — serve the GUI + API on a port.
//!
//! Usage:
//!   trainer-gui <train-set.jsonl> [--port N] [--host H]

#![recursion_limit = "256"]

use std::sync::{Arc, RwLock, atomic::AtomicBool};

use axum::Router;
use trainer_gui::server::{AppState, Status, router};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: {} <train-set.jsonl> <vocoder.onnx> [--port N] [--host H]",
            args[0]
        );
        std::process::exit(2);
    }
    let train_set = args[1].clone();
    let vocoder = args[2].clone();
    let mut port = 18230u16;
    let mut host = "127.0.0.1".to_string();
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                port = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(port);
            }
            "--host" => {
                i += 1;
                host = args.get(i).cloned().unwrap_or(host);
            }
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let state = AppState {
        status: Arc::new(RwLock::new(Status::default())),
        stop: Arc::new(AtomicBool::new(false)),
        train_set,
        vocoder,
        train_cache: Arc::new(RwLock::new(None)),
        logs: Arc::new(RwLock::new(Vec::new())),
        last_cpu: Arc::new(RwLock::new((0, 0))),
        last_proc: Arc::new(RwLock::new(std::collections::HashMap::new())),
    };

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async move {
        let app: Router = router(state);
        let addr = format!("{host}:{port}");
        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| {
            eprintln!("bind {addr}: {e}");
            std::process::exit(1);
        });
        println!("trainer-gui on http://{addr}  (GET / = GUI, /api/status = JSON)");
        axum::serve(listener, app).await.expect("server error");
    });
}
