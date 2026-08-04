//! Undo/redo stack with command groups — mirror of `DocManager`'s
//! `undoQueue`/`redoQueue`/`UCommandGroup` (`OpenUtau.Core/DocManager.cs`).
//!
//! * [`UndoStack::execute`] runs a command immediately and records it.
//!   When an undo group is open the command is recorded into that group
//!   instead of creating its own undo step.
//! * [`UndoStack::start_group`] / [`UndoStack::end_group`] merge consecutive
//!   commands into a single undo step, exactly like
//!   `DocManager.StartUndoGroup` / `EndUndoGroup`. Nested groups flatten:
//!   ending an inner group appends its commands to the outer group.
//! * [`UndoStack::undo`] reverts the most recent step (all commands of the
//!   group, in reverse order); [`UndoStack::redo`] re-applies it.
//! * Executing a new command clears the redo stack.

use domain::UProject;

use crate::command::Command;

/// A named sequence of commands undone/redone as a single step
/// (`UCommandGroup`).
pub struct Group {
    name: String,
    commands: Vec<Box<dyn Command>>,
}

impl Group {
    /// A new empty group. The name is shown by
    /// [`UndoStack::undo`](UndoStack::undo) and
    /// [`UndoStack::redo`](UndoStack::redo) results.
    pub fn new(name: impl Into<String>) -> Self {
        Group {
            name: name.into(),
            commands: Vec::new(),
        }
    }

    /// A group holding a single executed command.
    fn single(cmd: Box<dyn Command>) -> Self {
        let name = cmd.name().to_string();
        Group {
            name,
            commands: vec![cmd],
        }
    }

    /// Group name (`UCommandGroup.ToString` in OpenUtau shows the first
    /// command; here the name is set explicitly by the caller).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of commands in the group.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the group contains no commands.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The commands in execution order.
    pub fn commands(&self) -> &[Box<dyn Command>] {
        &self.commands
    }
}

/// Undo/redo stack driving commands against a [`UProject`].
///
/// The stack does not own the project: every method takes it as a
/// parameter, so a stack instance can be reused across project loads (a
/// fresh project should be paired with [`clear`](Self::clear)).
pub struct UndoStack {
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// Open undo groups; `execute` records into the innermost one.
    groups: Vec<Group>,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoStack {
    /// A new empty undo stack.
    pub fn new() -> Self {
        UndoStack {
            undo: Vec::new(),
            redo: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Execute `cmd` immediately and record it.
    ///
    /// With an open group the command joins that group (one undo step for
    /// the whole group); otherwise it becomes its own undo step. Either
    /// way the redo stack is cleared, like `DocManager.EndUndoGroup`.
    ///
    /// On `Err` the command must not have mutated the project (commands
    /// validate before mutating) and nothing is recorded.
    pub fn execute(
        &mut self,
        project: &mut UProject,
        mut cmd: Box<dyn Command>,
    ) -> Result<(), String> {
        cmd.execute(project)?;
        self.redo.clear();
        match self.groups.last_mut() {
            Some(group) => group.commands.push(cmd),
            None => self.undo.push(Group::single(cmd)),
        }
        Ok(())
    }

    /// Open a new undo group (`DocManager.StartUndoGroup`).
    ///
    /// Commands executed while a group is open are recorded into it. If a
    /// group is already open the new group nests inside it and flattens on
    /// close (ending the inner group appends its commands to the outer
    /// one).
    pub fn start_group(&mut self, name: impl Into<String>) {
        self.groups.push(Group::new(name));
    }

    /// Close the innermost open undo group (`DocManager.EndUndoGroup`).
    ///
    /// When no group is open this is an error. A non-empty group becomes a
    /// single undo step; an empty group is discarded. If an outer group is
    /// still open, the closed group's commands are appended to it instead.
    pub fn end_group(&mut self) -> Result<(), String> {
        let group = self
            .groups
            .pop()
            .ok_or_else(|| "no active undo group to end".to_string())?;
        if let Some(outer) = self.groups.last_mut() {
            outer.commands.extend(group.commands);
        } else if !group.is_empty() {
            self.undo.push(group);
        }
        Ok(())
    }

    /// Abandon the innermost open group: unexecute its commands in reverse
    /// order and drop it, leaving the project as if the group never ran.
    ///
    /// Returns `Err` (and keeps the group open) if any unexecute fails.
    pub fn cancel_group(&mut self, project: &mut UProject) -> Result<(), String> {
        let Some(mut group) = self.groups.pop() else {
            return Err("no active undo group to cancel".to_string());
        };
        for cmd in group.commands.iter_mut().rev() {
            cmd.unexecute(project)?;
        }
        group.commands.clear();
        Ok(())
    }

    /// Undo the most recent step (`DocManager.Undo`).
    ///
    /// An open group is closed first (its commands are already applied, so
    /// they belong to the undo history). Returns the name of the undone
    /// step, or `Ok(None)` when there is nothing to undo.
    ///
    /// On failure the step is restored to the undo stack (commands already
    /// unexecuted are re-executed) and `Err` is returned.
    pub fn undo(&mut self, project: &mut UProject) -> Result<Option<String>, String> {
        if !self.groups.is_empty() {
            self.end_group()?;
        }
        let Some(mut group) = self.undo.pop() else {
            return Ok(None);
        };
        let name = group.name.clone();
        for i in (0..group.commands.len()).rev() {
            if let Err(e) = group.commands[i].unexecute(project) {
                // Roll forward the commands already unexecuted, then put
                // the step back so the stack stays consistent.
                for j in (i + 1)..group.commands.len() {
                    let _ = group.commands[j].execute(project);
                }
                self.undo.push(group);
                return Err(format!("undo failed: {e}"));
            }
        }
        self.redo.push(group);
        Ok(Some(name))
    }

    /// Redo the most recently undone step (`DocManager.Redo`).
    ///
    /// Returns the name of the redone step, or `Ok(None)` when there is
    /// nothing to redo. Mirrors [`undo`](Self::undo) failure handling.
    pub fn redo(&mut self, project: &mut UProject) -> Result<Option<String>, String> {
        let Some(mut group) = self.redo.pop() else {
            return Ok(None);
        };
        let name = group.name.clone();
        for i in 0..group.commands.len() {
            if let Err(e) = group.commands[i].execute(project) {
                for j in (0..i).rev() {
                    let _ = group.commands[j].unexecute(project);
                }
                self.redo.push(group);
                return Err(format!("redo failed: {e}"));
            }
        }
        self.undo.push(group);
        Ok(Some(name))
    }

    /// Whether there is a step to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is a step to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Number of undo steps.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Number of redo steps.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Whether an undo group is currently open.
    pub fn is_group_open(&self) -> bool {
        !self.groups.is_empty()
    }

    /// The undo steps in undo order (most recent last).
    pub fn undo_steps(&self) -> &[Group] {
        &self.undo
    }

    /// The redo steps in redo order (most recent last).
    pub fn redo_steps(&self) -> &[Group] {
        &self.redo
    }

    /// Drop all undo/redo history and any open group (commands already
    /// applied to the project are *not* reverted).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.groups.clear();
    }
}
