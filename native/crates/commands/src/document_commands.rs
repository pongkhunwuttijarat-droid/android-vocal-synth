//! Document-level command: replace the whole project in one undo step.
//!
//! This is the commit primitive for a session group transaction
//! (`docs/CONTRACT-v1.md` §6.8). A group is edited against a *tentative* copy
//! of the document, so the only thing the real undo stack has to record is the
//! transition "state before the group → state after the group". Keeping both
//! snapshots in the command makes undo/redo exact by construction, no matter
//! how many child commands the group contained.

use domain::UProject;

use crate::Command;

/// Replace the project with `after`; `unexecute` restores `before`.
///
/// Both snapshots are owned clones, so the command is self-contained and
/// idempotent: `execute`/`unexecute` may be called repeatedly across redo/undo
/// cycles and always land on the same two states.
#[derive(Debug, Clone)]
pub struct RestoreDocumentCommand {
    before: UProject,
    after: UProject,
    name: String,
}

impl RestoreDocumentCommand {
    /// Build the command from the two document states.
    pub fn new(before: UProject, after: UProject, name: impl Into<String>) -> Self {
        Self {
            before,
            after,
            name: name.into(),
        }
    }

    /// The state the command installs.
    pub fn after(&self) -> &UProject {
        &self.after
    }
}

impl Command for RestoreDocumentCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        *project = self.after.clone();
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        *project = self.before.clone();
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UndoStack;
    use domain::UProject;

    #[test]
    fn execute_and_unexecute_swap_the_two_states() {
        let before = UProject::create();
        let mut after = before.clone();
        after.name = "After".to_string();
        let mut command = RestoreDocumentCommand::new(before.clone(), after.clone(), "Group");

        let mut project = before.clone();
        command.execute(&mut project).expect("execute");
        assert_eq!(project.name, "After");
        command.unexecute(&mut project).expect("unexecute");
        assert_eq!(project, before);

        // Idempotent across redo/undo cycles.
        command.execute(&mut project).expect("execute again");
        assert_eq!(project.name, "After");
    }

    #[test]
    fn the_undo_stack_sees_one_step() {
        let before = UProject::create();
        let mut after = before.clone();
        after.name = "After".to_string();

        let mut project = before.clone();
        let mut stack = UndoStack::new();
        stack
            .execute(
                &mut project,
                Box::new(RestoreDocumentCommand::new(
                    before.clone(),
                    after.clone(),
                    "Move notes",
                )),
            )
            .expect("execute");

        assert_eq!(project.name, "After");
        assert_eq!(stack.undo_len(), 1, "a group commit is exactly one step");
        assert_eq!(
            stack.undo(&mut project).expect("undo").as_deref(),
            Some("Move notes")
        );
        assert_eq!(project, before);
        assert_eq!(stack.undo_len(), 0);
    }
}
