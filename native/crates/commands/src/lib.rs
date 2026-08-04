//! Command system for the Android voice synth editor (Sprint 1.2).
//!
//! Implements the OpenUtau `UCommand` pattern (`OpenUtau.Core/Commands/*`):
//! reversible commands that operate on a [`UProject`] via the [`Command`]
//! trait, plus an [`UndoStack`] that records executed commands and merges
//! groups of commands into single undo steps (like `DocManager`).
//!
//! Commands are pure in-memory edits: no I/O, no FFI. Every command
//! validates its preconditions inside [`Command::execute`] *before*
//! mutating anything, so a failed execute leaves the project untouched and
//! the undo stack unmodified.
//!
//! # Identity model
//!
//! OpenUtau commands hold C# object references; Rust values cannot. Commands
//! therefore snapshot the domain structs they touch (notes, parts, tracks,
//! curves) and locate them again on execute/unexecute by value equality,
//! with the construction-time index as a fast-path hint. This keeps
//! `execute` + `unexecute` round-trips exact (verified by `PartialEq`
//! comparisons in the tests) even when earlier commands in the same undo
//! group shift list indices.
//!
//! # Example
//!
//! ```no_run
//! use commands::{AddNoteCommand, UndoStack};
//! use domain::UProject;
//!
//! let mut project = UProject::create();
//! let mut stack = UndoStack::new();
//! let note = project.create_note_at(60, 480, 480);
//! let cmd = AddNoteCommand::new(&project, 0, note).unwrap();
//! stack.execute(&mut project, Box::new(cmd)).unwrap();
//! stack.undo(&mut project).unwrap();
//! ```

pub mod command;
pub mod exp_commands;
pub mod note_commands;
pub mod part_commands;
pub mod track_commands;
pub mod undo_stack;

mod locate;

pub use command::Command;
pub use exp_commands::{
    SetCurveCommand, SetCurvePointsCommand, SetExpDescriptorCommand, SetExpressionCommand,
};
pub use note_commands::{
    AddNoteCommand, ChangeNoteLyricCommand, ChangeNoteToneCommand, ChangeNoteTuningCommand,
    MoveNoteCommand, RemoveNoteCommand, ResizeNoteCommand,
};
pub use part_commands::{
    AddPartCommand, MovePartCommand, RemovePartCommand, RenamePartCommand, ResizeVoicePartCommand,
    ResizeWavePartCommand,
};
pub use track_commands::{
    AddTrackCommand, RemoveTrackCommand, RenameTrackCommand, SetTrackMuteCommand,
    SetTrackPanCommand, SetTrackSoloCommand, SetTrackVolumeCommand,
};
pub use undo_stack::{Group, UndoStack};
