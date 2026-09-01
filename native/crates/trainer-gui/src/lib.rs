//! trainer-gui — desktop GUI + MCP-like HTTP API for the burn trainer.
//!
//! A small axum server that runs moe-train's training loop in a
//! background thread and exposes:
//!   GET  /            → embedded progress GUI (single HTML page)
//!   GET  /api/status  → {state, backend, epoch, losses, history, ...}
//!   POST /api/start   → start training {epochs, batch, lr, eval_frac, backend}
//!   POST /api/stop    → cooperative stop between epochs
//!
//! The JSON API is agent-friendly (curl-able) — "MCP-like": an agent
//! can watch progress, start/stop runs, and read the loss history.

// burn-wgpu's wgpu dep chain nests associated types past the default
// recursion limit (E0275) — the documented fix (burn-wgpu README).
#![recursion_limit = "256"]

pub mod server;
