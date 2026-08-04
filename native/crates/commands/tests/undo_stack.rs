//! Undo/redo stack behavior: recording, LIFO undo, redo, redo clearing,
//! groups as single undo steps, nesting, cancellation, and error handling.

mod common;

use commands::{AddNoteCommand, Command, Group, UndoStack};
use domain::{UPart, UProject};

/// A trivial command appending to `project.comment`, used to observe
/// execution order.
struct PushCommentCommand {
    text: String,
}

impl PushCommentCommand {
    fn new(text: impl Into<String>) -> Self {
        PushCommentCommand { text: text.into() }
    }
}

impl Command for PushCommentCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        project.comment.push_str(&self.text);
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        if !project.comment.ends_with(&self.text) {
            return Err("comment mismatch".to_string());
        }
        project
            .comment
            .truncate(project.comment.len() - self.text.len());
        Ok(())
    }

    fn name(&self) -> &str {
        "Push comment"
    }
}

fn add_note_cmd(project: &UProject, tone: i32, pos: i32, dur: i32) -> Box<dyn Command> {
    Box::new(AddNoteCommand::new(project, 0, project.create_note_at(tone, pos, dur)).unwrap())
}

#[test]
fn execute_undo_redo_cycle() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut stack = UndoStack::new();

    let cmd = add_note_cmd(&project, 65, 1440, 240);
    stack.execute(&mut project, cmd).unwrap();
    assert!(stack.can_undo());
    assert!(!stack.can_redo());
    assert_eq!(stack.undo_len(), 1);

    let name = stack.undo(&mut project).unwrap();
    assert_eq!(name.as_deref(), Some("Add note"));
    assert_eq!(project, before);
    assert!(!stack.can_undo());
    assert!(stack.can_redo());

    let name = stack.redo(&mut project).unwrap();
    assert_eq!(name.as_deref(), Some("Add note"));
    assert_eq!(stack.undo_len(), 1);
    assert!(!stack.can_redo());
    if let UPart::Voice(vp) = &project.parts[0] {
        assert_eq!(vp.notes.len(), 4);
    }
}

#[test]
fn undo_redo_repeatable() {
    let mut project = common::project_with_notes();
    let before = project.clone();
    let mut stack = UndoStack::new();
    let cmd = add_note_cmd(&project, 65, 1440, 240);
    stack.execute(&mut project, cmd).unwrap();

    for _ in 0..3 {
        stack.undo(&mut project).unwrap();
        assert_eq!(project, before);
        stack.redo(&mut project).unwrap();
        if let UPart::Voice(vp) = &project.parts[0] {
            assert_eq!(vp.notes.len(), 4);
        }
    }
}

#[test]
fn execute_clears_redo() {
    let mut project = common::project_with_notes();
    let mut stack = UndoStack::new();
    let cmd = add_note_cmd(&project, 65, 1440, 240);
    stack.execute(&mut project, cmd).unwrap();
    stack.undo(&mut project).unwrap();
    assert!(stack.can_redo());

    let cmd = add_note_cmd(&project, 66, 1440, 240);
    stack.execute(&mut project, cmd).unwrap();
    assert!(!stack.can_redo());
    assert_eq!(stack.undo_len(), 1);
}

#[test]
fn undo_on_empty_stack_returns_none() {
    let mut project = common::project_with_notes();
    let mut stack = UndoStack::new();
    assert_eq!(stack.undo(&mut project).unwrap(), None);
    assert_eq!(stack.redo(&mut project).unwrap(), None);
}

#[test]
fn group_is_one_undo_step() {
    let mut project = common::voice_project();
    let before = project.clone();
    let mut stack = UndoStack::new();

    stack.start_group("Add notes");
    let cmd = add_note_cmd(&project, 60, 0, 480);
    stack.execute(&mut project, cmd).unwrap();
    let cmd = add_note_cmd(&project, 62, 480, 480);
    stack.execute(&mut project, cmd).unwrap();
    let cmd = add_note_cmd(&project, 64, 960, 480);
    stack.execute(&mut project, cmd).unwrap();
    assert_eq!(stack.undo_len(), 0); // group not yet closed
    assert!(stack.is_group_open());
    stack.end_group().unwrap();

    assert_eq!(stack.undo_len(), 1);
    assert!(!stack.is_group_open());
    if let UPart::Voice(vp) = &project.parts[0] {
        assert_eq!(vp.notes.len(), 3);
    }

    // A single undo reverts all three commands in reverse order.
    let name = stack.undo(&mut project).unwrap();
    assert_eq!(name.as_deref(), Some("Add notes"));
    assert_eq!(project, before);

    // A single redo re-applies all three in order.
    stack.redo(&mut project).unwrap();
    if let UPart::Voice(vp) = &project.parts[0] {
        assert_eq!(vp.notes.len(), 3);
        assert_eq!(vp.notes[2].tone, 64);
    }
}

#[test]
fn group_exposes_commands() {
    let mut project = common::voice_project();
    let mut stack = UndoStack::new();
    stack.start_group("g");
    let cmd = add_note_cmd(&project, 60, 0, 480);
    stack.execute(&mut project, cmd).unwrap();
    let cmd = add_note_cmd(&project, 62, 480, 480);
    stack.execute(&mut project, cmd).unwrap();
    stack.end_group().unwrap();

    let group: &Group = stack.undo_steps().last().expect("undo step");
    assert_eq!(group.name(), "g");
    assert_eq!(group.len(), 2);
    assert_eq!(group.commands()[0].name(), "Add note");
}

#[test]
fn empty_group_is_discarded() {
    let mut stack = UndoStack::new();
    stack.start_group("nothing");
    stack.end_group().unwrap();
    assert_eq!(stack.undo_len(), 0);
    assert!(!stack.can_undo());
}

#[test]
fn end_group_without_open_group_errors() {
    let mut stack = UndoStack::new();
    assert!(stack.end_group().is_err());
}

#[test]
fn nested_groups_flatten_into_outer_step() {
    let mut project = common::voice_project();
    let before = project.clone();
    let mut stack = UndoStack::new();

    stack.start_group("outer");
    let cmd = add_note_cmd(&project, 60, 0, 480);
    stack.execute(&mut project, cmd).unwrap();
    stack.start_group("inner");
    let cmd = add_note_cmd(&project, 62, 480, 480);
    stack.execute(&mut project, cmd).unwrap();
    stack.end_group().unwrap(); // inner closes into outer
    let cmd = add_note_cmd(&project, 64, 960, 480);
    stack.execute(&mut project, cmd).unwrap();
    stack.end_group().unwrap();

    assert_eq!(stack.undo_len(), 1);
    stack.undo(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn undo_closes_open_group_first() {
    let mut project = common::voice_project();
    let before = project.clone();
    let mut stack = UndoStack::new();

    let cmd = add_note_cmd(&project, 60, 0, 480);
    stack.execute(&mut project, cmd).unwrap();
    stack.start_group("late");
    let cmd = add_note_cmd(&project, 62, 480, 480);
    stack.execute(&mut project, cmd).unwrap();

    // undo() first closes the open group, then undoes it.
    stack.undo(&mut project).unwrap();
    assert!(!stack.is_group_open());
    assert_eq!(stack.undo_len(), 1);
    if let UPart::Voice(vp) = &project.parts[0] {
        assert_eq!(vp.notes.len(), 1); // only the grouped note undone
    }

    stack.undo(&mut project).unwrap();
    assert_eq!(project, before);
    assert!(!stack.can_undo());
}

#[test]
fn cancel_group_reverts_its_commands() {
    let mut project = common::voice_project();
    let before = project.clone();
    let mut stack = UndoStack::new();

    stack.start_group("doomed");
    let cmd = add_note_cmd(&project, 60, 0, 480);
    stack.execute(&mut project, cmd).unwrap();
    let cmd = add_note_cmd(&project, 62, 480, 480);
    stack.execute(&mut project, cmd).unwrap();
    stack.cancel_group(&mut project).unwrap();

    assert_eq!(project, before);
    assert_eq!(stack.undo_len(), 0);
    assert!(!stack.is_group_open());

    assert!(stack.cancel_group(&mut project).is_err()); // nothing open
}

#[test]
fn undo_redo_order_is_lifo() {
    let mut project = common::voice_project();
    let mut stack = UndoStack::new();
    stack
        .execute(&mut project, Box::new(PushCommentCommand::new("a")))
        .unwrap();
    stack
        .execute(&mut project, Box::new(PushCommentCommand::new("b")))
        .unwrap();
    assert_eq!(project.comment, "ab");

    stack.undo(&mut project).unwrap(); // undoes "b" first
    assert_eq!(project.comment, "a");
    stack.undo(&mut project).unwrap();
    assert_eq!(project.comment, "");

    stack.redo(&mut project).unwrap(); // redoes "a" first
    assert_eq!(project.comment, "a");
    stack.redo(&mut project).unwrap();
    assert_eq!(project.comment, "ab");
}

#[test]
fn group_undo_unexecutes_in_reverse_order() {
    let mut project = common::voice_project();
    let mut stack = UndoStack::new();
    stack.start_group("g");
    stack
        .execute(&mut project, Box::new(PushCommentCommand::new("a")))
        .unwrap();
    stack
        .execute(&mut project, Box::new(PushCommentCommand::new("b")))
        .unwrap();
    stack.end_group().unwrap();
    assert_eq!(project.comment, "ab");

    stack.undo(&mut project).unwrap();
    assert_eq!(project.comment, ""); // "b" removed before "a"
}

#[test]
fn failed_execute_leaves_stack_and_project_unchanged() {
    let mut project = common::project_with_notes();
    let mut stack = UndoStack::new();
    let cmd = add_note_cmd(&project, 65, 1440, 240);
    stack.execute(&mut project, cmd).unwrap();
    let after_add = project.clone();

    let bad = AddNoteCommand::new(&project, 0, project.create_note_at(60, 0, 0)).unwrap();
    assert!(stack.execute(&mut project, Box::new(bad)).is_err());
    assert_eq!(stack.undo_len(), 1); // the bad command was not recorded
    assert_eq!(project, after_add); // and it did not mutate the project

    // The stack still works normally afterwards.
    stack.undo(&mut project).unwrap();
    assert_eq!(project, common::project_with_notes());
}

#[test]
fn clear_resets_everything() {
    let mut project = common::voice_project();
    let mut stack = UndoStack::new();
    let cmd = add_note_cmd(&project, 60, 0, 480);
    stack.execute(&mut project, cmd).unwrap();
    stack.start_group("open");
    stack.clear();
    assert_eq!(stack.undo_len(), 0);
    assert_eq!(stack.redo_len(), 0);
    assert!(!stack.is_group_open());
}

#[test]
fn single_command_group_after_undo_redo_keeps_order() {
    let mut project = common::voice_project();
    let mut stack = UndoStack::new();
    stack
        .execute(&mut project, Box::new(PushCommentCommand::new("a")))
        .unwrap();
    stack.start_group("g");
    stack
        .execute(&mut project, Box::new(PushCommentCommand::new("b")))
        .unwrap();
    stack
        .execute(&mut project, Box::new(PushCommentCommand::new("c")))
        .unwrap();
    stack.end_group().unwrap();
    assert_eq!(project.comment, "abc");

    stack.undo(&mut project).unwrap();
    assert_eq!(project.comment, "a");
    stack.undo(&mut project).unwrap();
    assert_eq!(project.comment, "");
    stack.redo(&mut project).unwrap();
    assert_eq!(project.comment, "a");
    stack.redo(&mut project).unwrap();
    assert_eq!(project.comment, "abc");
}
