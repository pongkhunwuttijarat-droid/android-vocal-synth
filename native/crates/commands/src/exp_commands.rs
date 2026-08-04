//! Expression commands — mirror of `OpenUtau.Core/Commands/ExpCommands.cs`
//! (`SetCurveCommand`, `SetNoteExpressionCommand`) plus the
//! `ConfigureExpressionsCommand`-style descriptor edit.
//!
//! Curve and note expressions are stored in the part/note values, so the
//! commands snapshot the affected values and restore them exactly on
//! unexecute. Like OpenUtau, curve edits that reference an unregistered
//! expression abbreviation are a silent no-op.

use domain::{UCurve, UExpression, UExpressionDescriptor, UNote, UPart, UProject, UVoicePart};

use crate::command::Command;
use crate::locate::{locate_part_mut, PartFingerprint};

/// Voice part snapshot used to locate the part on execute/unexecute.
struct PartTarget {
    part_index: usize,
    part_fp: PartFingerprint,
}

impl PartTarget {
    fn from_project(project: &UProject, part_index: usize) -> Result<Self, String> {
        let part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp,
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        Ok(PartTarget {
            part_index,
            part_fp: PartFingerprint::of(part),
        })
    }
}

/// Curve descriptor bounds of `abbr` in the project, if registered.
fn descriptor_bounds(project: &UProject, abbr: &str) -> Option<(i32, i32)> {
    project
        .expressions
        .get(abbr)
        .map(|d| (d.min as i32, d.max as i32))
}

/// Edit a single curve point (`SetCurveCommand`).
///
/// Mirrors OpenUtau: `y` and `last_y` are clamped to the expression
/// descriptor's `[min, max]` range and the point is written through
/// [`UCurve::set`], which snaps to the 5-tick interval and keeps the curve
/// continuous. When the curve does not exist yet it is created (only for
/// registered expressions), and unexecute removes it again so the state is
/// restored exactly.
pub struct SetCurveCommand {
    name: &'static str,
    target: PartTarget,
    abbr: String,
    x: i32,
    y: i32,
    last_x: i32,
    last_y: i32,
    old_xs: Option<Vec<i32>>,
    old_ys: Option<Vec<i32>>,
    curve_existed: bool,
}

impl SetCurveCommand {
    /// `SetCurveCommand(project, part, abbr, x, y, lastX, lastY)`.
    pub fn new(
        project: &UProject,
        part_index: usize,
        abbr: impl Into<String>,
        x: i32,
        y: i32,
        last_x: i32,
        last_y: i32,
    ) -> Result<Self, String> {
        let target = PartTarget::from_project(project, part_index)?;
        let abbr = abbr.into();
        let part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp,
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        let curve = part.curves.iter().find(|c| c.abbr == abbr);
        Ok(SetCurveCommand {
            name: "Set curve",
            target,
            abbr,
            x,
            y,
            last_x,
            last_y,
            old_xs: curve.map(|c| c.xs.clone()),
            old_ys: curve.map(|c| c.ys.clone()),
            curve_existed: curve.is_some(),
        })
    }
}

impl Command for SetCurveCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        let Some((min, max)) = descriptor_bounds(project, &self.abbr) else {
            return Ok(()); // unregistered expression: no-op, like OpenUtau
        };
        let part = locate_part_mut(project, self.target.part_index, &self.target.part_fp)?;
        let curve = match part.curves.iter_mut().find(|c| c.abbr == self.abbr) {
            Some(c) => c,
            None => {
                part.curves.push(UCurve::new(self.abbr.clone()));
                part.curves.last_mut().expect("just pushed")
            }
        };
        let y = self.y.clamp(min, max);
        let last_y = self.last_y.clamp(min, max);
        curve.set(self.x, y, self.last_x, last_y);
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let part = locate_part_mut(project, self.target.part_index, &self.target.part_fp)?;
        if !self.curve_existed {
            part.curves.retain(|c| c.abbr != self.abbr);
            return Ok(());
        }
        let curve = part
            .curves
            .iter_mut()
            .find(|c| c.abbr == self.abbr)
            .ok_or_else(|| "curve not found in part".to_string())?;
        curve.xs = self.old_xs.clone().unwrap_or_default();
        curve.ys = self.old_ys.clone().unwrap_or_default();
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Replace all points of a curve (`SetCurvePointsCommand`).
///
/// The `xs`/`ys` arrays replace the curve's points wholesale (for a
/// registered expression only). The curve is created when missing and
/// removed again by unexecute, restoring the exact prior state.
pub struct SetCurvePointsCommand {
    name: &'static str,
    target: PartTarget,
    abbr: String,
    xs: Vec<i32>,
    ys: Vec<i32>,
    old_xs: Option<Vec<i32>>,
    old_ys: Option<Vec<i32>>,
    curve_existed: bool,
}

impl SetCurvePointsCommand {
    pub fn new(
        project: &UProject,
        part_index: usize,
        abbr: impl Into<String>,
        xs: Vec<i32>,
        ys: Vec<i32>,
    ) -> Result<Self, String> {
        if xs.len() != ys.len() {
            return Err("curve xs and ys must have the same length".to_string());
        }
        let target = PartTarget::from_project(project, part_index)?;
        let abbr = abbr.into();
        let part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp,
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        let curve = part.curves.iter().find(|c| c.abbr == abbr);
        Ok(SetCurvePointsCommand {
            name: "Set curve points",
            target,
            abbr,
            xs,
            ys,
            old_xs: curve.map(|c| c.xs.clone()),
            old_ys: curve.map(|c| c.ys.clone()),
            curve_existed: curve.is_some(),
        })
    }
}

impl Command for SetCurvePointsCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        if !project.expressions.contains_key(&self.abbr) {
            return Ok(()); // unregistered expression: no-op, like OpenUtau
        }
        let part = locate_part_mut(project, self.target.part_index, &self.target.part_fp)?;
        let curve = match part.curves.iter_mut().find(|c| c.abbr == self.abbr) {
            Some(c) => c,
            None => {
                part.curves.push(UCurve::new(self.abbr.clone()));
                part.curves.last_mut().expect("just pushed")
            }
        };
        curve.xs = self.xs.clone();
        curve.ys = self.ys.clone();
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let part = locate_part_mut(project, self.target.part_index, &self.target.part_fp)?;
        if !self.curve_existed {
            part.curves.retain(|c| c.abbr != self.abbr);
            return Ok(());
        }
        let curve = part
            .curves
            .iter_mut()
            .find(|c| c.abbr == self.abbr)
            .ok_or_else(|| "curve not found in part".to_string())?;
        curve.xs = self.old_xs.clone().unwrap_or_default();
        curve.ys = self.old_ys.clone().unwrap_or_default();
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Set (or clear) a note's phoneme expression value
/// (`SetNoteExpressionCommand`).
///
/// The value is written into `note.phoneme_expressions` for `abbr`, clamped
/// to the descriptor's `[min, max]` range (`clr` is never clamped, like
/// OpenUtau). `value = None` clears the expression. Edits for unregistered
/// abbreviations are a silent no-op. Only the first phoneme (`index = 0`)
/// is addressed — the domain model does not carry per-phoneme index lists.
pub struct SetExpressionCommand {
    name: &'static str,
    part_index: usize,
    part_fp: PartFingerprint,
    note_index: usize,
    old_note: UNote,
    new_note: UNote,
}

impl SetExpressionCommand {
    /// `SetNoteExpressionCommand(project, track, part, note, abbr, values)`
    /// reduced to a single value.
    pub fn new(
        project: &UProject,
        part_index: usize,
        note_index: usize,
        abbr: &str,
        value: Option<f32>,
    ) -> Result<Self, String> {
        let part = match project.parts.get(part_index) {
            Some(UPart::Voice(vp)) => vp,
            _ => return Err("part index does not refer to a voice part".to_string()),
        };
        let old_note = part
            .notes
            .get(note_index)
            .cloned()
            .ok_or_else(|| "note index out of bounds".to_string())?;
        let new_note = set_note_expression(project, part, &old_note, abbr, value);
        Ok(SetExpressionCommand {
            name: "Set expression",
            part_index,
            part_fp: PartFingerprint::of(part),
            note_index,
            old_note,
            new_note,
        })
    }
}

/// `UNote.SetExpression(project, track, abbr, values)` for a single value:
/// look up the descriptor, drop existing expressions with `abbr`, and
/// re-add the value when it is not `None` (clamped unless `clr`).
fn set_note_expression(
    project: &UProject,
    part: &UVoicePart,
    note: &UNote,
    abbr: &str,
    value: Option<f32>,
) -> UNote {
    let mut new_note = note.clone();
    let descriptor = project
        .tracks
        .get(part.track_no as usize)
        .and_then(|track| track.try_get_exp_descriptor(project, abbr));
    let Some(descriptor) = descriptor else {
        return new_note; // unregistered expression: no-op, like OpenUtau
    };
    new_note.phoneme_expressions.retain(|e| e.abbr != abbr);
    if let Some(value) = value {
        let mut exp = UExpression {
            index: Some(0),
            abbr: abbr.to_string(),
            value,
        };
        exp.clamp_value(descriptor.min, descriptor.max);
        new_note.phoneme_expressions.push(exp);
    }
    new_note
}

impl Command for SetExpressionCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        let part = locate_part_mut(project, self.part_index, &self.part_fp)?;
        let idx = crate::locate::locate_note_index(
            part,
            self.note_index,
            &self.old_note,
            Some(&self.new_note),
        )?;
        part.notes[idx] = self.new_note.clone();
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let part = locate_part_mut(project, self.part_index, &self.part_fp)?;
        let idx = crate::locate::locate_note_index(
            part,
            self.note_index,
            &self.new_note,
            Some(&self.old_note),
        )?;
        part.notes[idx] = self.old_note.clone();
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// Insert or replace an expression descriptor in the project registry
/// (`ConfigureExpressionsCommand` reduced to a single descriptor).
///
/// `project.expressions` is keyed by abbreviation; unexecute restores the
/// previous descriptor (or removes the key when there was none).
pub struct SetExpDescriptorCommand {
    name: &'static str,
    descriptor: UExpressionDescriptor,
    old_descriptor: Option<UExpressionDescriptor>,
}

impl SetExpDescriptorCommand {
    pub fn new(project: &UProject, descriptor: UExpressionDescriptor) -> Self {
        let old_descriptor = project.expressions.get(&descriptor.abbr).cloned();
        SetExpDescriptorCommand {
            name: "Set expression descriptor",
            descriptor,
            old_descriptor,
        }
    }
}

impl Command for SetExpDescriptorCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        project
            .expressions
            .insert(self.descriptor.abbr.clone(), self.descriptor.clone());
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        match &self.old_descriptor {
            Some(old) => {
                project
                    .expressions
                    .insert(self.descriptor.abbr.clone(), old.clone());
            }
            None => {
                project.expressions.remove(&self.descriptor.abbr);
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        self.name
    }
}
