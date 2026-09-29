//! Backwards-compatible re-export for the Rust-owned project session.
//!
//! Command targets remain index-based for the existing WASM contract. The
//! host should cancel an open group with `group_cancel` after a rejected child
//! command; session-group commands apply incrementally until committed/cancelled.
pub use project_session::ProjectDoc;
