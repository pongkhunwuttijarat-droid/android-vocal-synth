//! moe-train — burn training for the MoE singing acoustic (plan
//! Task 3+). POC v0: a frame-wise baseline acoustic — per-frame
//! (phoneme_id, normalized f0, in-phoneme position) → mel frame.
//! Trained on the train-prep distillation set (R2 teacher mel).

// burn-wgpu's wgpu dep chain nests associated types past the default
// recursion limit (E0275) — the documented fix (burn-wgpu README).
#![recursion_limit = "256"]

pub mod baseline;
pub mod data;
pub mod phoneme_groups;
pub mod render;

/// Re-export the train-prep entry shape so the trainer can consume
/// `train-set.jsonl` produced by train-prep without its full dep tree.
pub use data::load_train_set;
