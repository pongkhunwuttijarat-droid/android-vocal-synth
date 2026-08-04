//! The [`Command`] trait — mirror of OpenUtau's `UCommand` base class
//! (`OpenUtau.Core/Commands/UCommand.cs`), adapted to Rust value semantics.

use domain::UProject;

/// A reversible edit on a [`UProject`].
///
/// This is the Rust counterpart of OpenUtau's `UCommand`:
///
/// * [`execute`](Self::execute) applies the edit;
/// * [`unexecute`](Self::unexecute) reverts it exactly;
/// * [`name`](Self::name) is a human-readable description (OpenUtau's
///   `ToString()`).
///
/// # Contract
///
/// Implementations must satisfy:
///
/// 1. **Validation before mutation** — `execute` (and `unexecute`) check
///    their preconditions first and return `Err` *without* touching the
///    project when they cannot apply. This mirrors the validation OpenUtau
///    performs around command execution (`Project.Validate`), adapted to a
///    pure `Result`-based API.
/// 2. **Exact round-trip** — for any command that succeeds, calling
///    `unexecute` immediately afterwards restores the project to a state
///    equal (`PartialEq`) to the one before `execute`.
/// 3. **Idempotent bookkeeping** — the command object itself may be reused:
///    `execute` / `unexecute` may be called repeatedly (redo/undo cycles)
///    and must apply the same edit each time.
///
/// Commands are executed through an [`UndoStack`](crate::UndoStack) in
/// normal use, but can also be driven directly.
pub trait Command {
    /// Apply the edit, validating preconditions first.
    ///
    /// On `Err` the project must be left exactly as it was.
    fn execute(&mut self, project: &mut UProject) -> Result<(), String>;

    /// Revert the edit, restoring the exact prior state.
    ///
    /// On `Err` the project must be left exactly as it was.
    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String>;

    /// Human-readable name of the command (OpenUtau `ToString()`), e.g.
    /// `"Add note"`.
    fn name(&self) -> &str;
}
