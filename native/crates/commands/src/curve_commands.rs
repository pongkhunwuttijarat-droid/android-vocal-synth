//! Curve stroke commands (CONTRACT-v1 §6.6).
//!
//! A freehand stroke is one persistent mutation and one undo step. The frontend
//! may sample densely, but Rust owns canonicalization: ordering, 5-tick
//! snapping, duplicate resolution, value clamping, and the interval semantics
//! that decide which existing points survive.
//!
//! Both modes snapshot the whole curve list of the part (`prior`), so
//! `unexecute` restores the previous state byte-exactly regardless of how the
//! stroke reshaped the list — the same technique `SetCurvePointsCommand` uses,
//! applied to the one part the stroke touches.

use domain::{snap_tick, UCurve, UPart, UProject, UVoicePart};

use crate::locate::{locate_part_mut, PartFingerprint};
use crate::Command;

/// Snapping interval for stroke points (`UCurve::INTERVAL`).
pub const STROKE_INTERVAL: i32 = UCurve::INTERVAL;

/// Which interval semantics to apply (§6.6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeMode {
    /// Replace the open interval `(start, end)`, preserving both endpoints so
    /// the curve stays continuous with what was already drawn.
    Replace,
    /// Reset the closed interval `[start, end]` to the descriptor default,
    /// removing the curve entirely when nothing is left.
    Clear,
}

/// A canonicalized stroke: what will actually be stored.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalStroke {
    pub mode: StrokeMode,
    pub start_tick: i32,
    pub end_tick: i32,
    /// Sorted, unique, snapped, clamped points.
    pub points: Vec<(i32, i32)>,
    /// How many values had to be clamped into the descriptor range.
    pub clamped_values: usize,
    /// How many samples collapsed onto an already-used tick (last one wins).
    pub collapsed_points: usize,
}

impl CanonicalStroke {
    /// True when applying this stroke cannot change the document.
    pub fn is_noop_shape(&self) -> bool {
        self.mode == StrokeMode::Clear && self.points.is_empty()
    }
}

/// Canonicalize raw stroke samples (§6.6.2).
///
/// Pure and deterministic: sorting is stable by `(raw tick, input order)`,
/// snapping is half-to-even on the 5-tick grid, later samples win at the same
/// snapped tick, and values clamp into `[min, max]`.
pub fn canonicalize(
    raw: &[(i32, i32)],
    start_tick: i32,
    end_tick: i32,
    mode: StrokeMode,
    min_value: i32,
    max_value: i32,
) -> CanonicalStroke {
    let mut ordered: Vec<usize> = (0..raw.len()).collect();
    ordered.sort_by_key(|&i| (raw[i].0, i));

    let mut points: Vec<(i32, i32)> = Vec::with_capacity(raw.len());
    let mut clamped_values = 0usize;
    let mut collapsed_points = 0usize;

    for i in ordered {
        let (tick, value) = raw[i];
        let snapped = snap_tick(tick, STROKE_INTERVAL).clamp(start_tick, end_tick);
        let clamped = value.clamp(min_value, max_value);
        if clamped != value {
            clamped_values += 1;
        }
        match points.last_mut() {
            // Same snapped tick: the later sample wins (§6.6.2 rule 3).
            Some(last) if last.0 == snapped => {
                last.1 = clamped;
                collapsed_points += 1;
            }
            _ => points.push((snapped, clamped)),
        }
    }

    CanonicalStroke {
        mode,
        start_tick,
        end_tick,
        points,
        clamped_values,
        collapsed_points,
    }
}

/// Where a stroke applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrokeTarget {
    /// Replace inside an existing curve at this index.
    Existing(usize),
    /// Append a new curve with this abbreviation.
    Create(String),
}

/// Apply a canonical stroke to a copy of the part's curve list.
///
/// Returns the new list plus the index the curve ended up at (`None` when a
/// `clear` removed it). Never mutates the input.
pub fn apply_stroke_to_curves(
    curves: &[UCurve],
    target: &StrokeTarget,
    default_value: i32,
    stroke: &CanonicalStroke,
) -> Result<(Vec<UCurve>, Option<usize>), String> {
    let mut out: Vec<UCurve> = curves.to_vec();
    let index = match target {
        StrokeTarget::Existing(i) => {
            if *i >= out.len() {
                return Err(format!("curve index {i} is out of range"));
            }
            *i
        }
        StrokeTarget::Create(abbr) => {
            out.push(UCurve::new(abbr.clone()));
            out.len() - 1
        }
    };

    // Boundary values are read BEFORE the interval is touched: they are the
    // continuity anchors and must use the descriptor default (not 0) when the
    // curve has no point there. `voic` defaults to 100, so `unwrap_or(0)` would
    // silently bend the curve at every stroke edge.
    let (left, right) = {
        let curve = &out[index];
        (
            curve.sample(stroke.start_tick).unwrap_or(default_value),
            curve.sample(stroke.end_tick).unwrap_or(default_value),
        )
    };

    match stroke.mode {
        StrokeMode::Replace => {
            let curve = &mut out[index];
            curve.delete_between_exclusive(stroke.start_tick, stroke.end_tick);
            if curve.xs.binary_search(&stroke.start_tick).is_err() {
                curve.insert(stroke.start_tick, left);
            }
            if curve.xs.binary_search(&stroke.end_tick).is_err() {
                curve.insert(stroke.end_tick, right);
            }
            for (tick, value) in &stroke.points {
                curve.insert(*tick, *value);
            }
            Ok((out, Some(index)))
        }
        StrokeMode::Clear => {
            let curve = &mut out[index];
            curve.delete_between_inclusive(stroke.start_tick, stroke.end_tick);
            if curve.xs.is_empty() {
                out.remove(index);
                return Ok((out, None));
            }
            Ok((out, Some(index)))
        }
    }
}

/// Replace/clear one curve interval as a single reversible edit.
pub struct ApplyCurveStrokeCommand {
    part: PartFingerprint,
    hint: usize,
    prior: Vec<UCurve>,
    after: Vec<UCurve>,
    /// Index of the stroke's curve in `after`, or `None` if it was removed.
    after_index: Option<usize>,
    name: String,
}

impl ApplyCurveStrokeCommand {
    /// Build the command from the current project state.
    pub fn new(
        project: &UProject,
        part_hint: usize,
        target: &StrokeTarget,
        default_value: i32,
        stroke: &CanonicalStroke,
    ) -> Result<Self, String> {
        let vp = voice_part_at(project, part_hint)?;
        let (after, after_index) =
            apply_stroke_to_curves(&vp.curves, target, default_value, stroke)?;
        let name = match stroke.mode {
            StrokeMode::Replace => "Draw curve",
            StrokeMode::Clear => "Clear curve",
        };
        Ok(Self {
            part: PartFingerprint::of(vp),
            hint: part_hint,
            prior: vp.curves.clone(),
            after,
            after_index,
            name: name.to_string(),
        })
    }

    /// The curve list the command installs.
    pub fn after_curves(&self) -> &[UCurve] {
        &self.after
    }

    /// Index of the affected curve after the edit (`None` = it was removed).
    pub fn after_index(&self) -> Option<usize> {
        self.after_index
    }
}

impl Command for ApplyCurveStrokeCommand {
    fn execute(&mut self, project: &mut UProject) -> Result<(), String> {
        let vp = locate_part_mut(project, self.hint, &self.part)?;
        vp.curves = self.after.clone();
        Ok(())
    }

    fn unexecute(&mut self, project: &mut UProject) -> Result<(), String> {
        let vp = locate_part_mut(project, self.hint, &self.part)?;
        vp.curves = self.prior.clone();
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

fn voice_part_at(project: &UProject, hint: usize) -> Result<&UVoicePart, String> {
    match project.parts.get(hint) {
        Some(UPart::Voice(vp)) => Ok(vp),
        Some(UPart::Wave(_)) => Err(format!("part index {hint} is not a voice part")),
        None => Err(format!("part index {hint} is out of range")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UndoStack;
    use domain::UProject;

    fn curve(abbr: &str, xs: Vec<i32>, ys: Vec<i32>) -> UCurve {
        UCurve {
            abbr: abbr.to_string(),
            xs,
            ys,
        }
    }

    #[test]
    fn canonicalize_sorts_snaps_and_resolves_duplicates() {
        // Half-to-even on the 5-tick grid: 7 -> 5, 12 -> 10. Two samples that
        // snap onto the same tick collapse, and the later one wins.
        let raw = vec![(12, 30), (7, 10), (0, 0), (12, 40)];
        let stroke = canonicalize(&raw, 0, 100, StrokeMode::Replace, -240, 120);

        assert_eq!(stroke.points, vec![(0, 0), (5, 10), (10, 40)]);
        assert_eq!(stroke.points.len(), 3);
        assert_eq!(stroke.collapsed_points, 1);
        assert_eq!(stroke.clamped_values, 0);

        // 13 snaps to 15, not 10 — snapping rounds, it does not truncate.
        let raw = vec![(12, 1), (13, 2)];
        let stroke = canonicalize(&raw, 0, 100, StrokeMode::Replace, -240, 120);
        assert_eq!(stroke.points, vec![(10, 1), (15, 2)]);
        assert_eq!(stroke.collapsed_points, 0);
    }

    #[test]
    fn canonicalize_clamps_values_and_ticks() {
        let raw = vec![(-50, 9999), (500, -9999)];
        let stroke = canonicalize(&raw, 0, 100, StrokeMode::Replace, -240, 120);

        assert_eq!(stroke.points, vec![(0, 120), (100, -240)]);
        assert_eq!(stroke.clamped_values, 2);
    }

    #[test]
    fn replace_preserves_endpoints_and_continuity() {
        // A curve with a non-zero default (voic-like) and points only outside
        // the stroke interval: the edges must take the default, not 0.
        let curves = vec![curve("voic", vec![0, 1000], vec![100, 100])];
        let stroke = canonicalize(
            &[(240, 80), (260, 70)],
            200,
            300,
            StrokeMode::Replace,
            0,
            100,
        );

        let (out, index) =
            apply_stroke_to_curves(&curves, &StrokeTarget::Existing(0), 100, &stroke)
                .expect("apply");

        assert_eq!(index, Some(0));
        let c = &out[0];
        assert_eq!(c.xs, vec![0, 200, 240, 260, 300, 1000]);
        assert_eq!(c.ys, vec![100, 100, 80, 70, 100, 100]);
        assert_eq!(c.sample(200), Some(100), "edge uses the descriptor default");
    }

    #[test]
    fn replace_drops_only_the_open_interval() {
        let curves = vec![curve(
            "dyn",
            vec![0, 200, 240, 300, 500],
            vec![0, 10, 20, 30, 40],
        )];
        let stroke = canonicalize(&[], 200, 300, StrokeMode::Replace, -240, 120);
        // An empty `replace` still rewrites the interval to its anchors.
        let (out, _) =
            apply_stroke_to_curves(&curves, &StrokeTarget::Existing(0), 0, &stroke).expect("apply");

        assert_eq!(out[0].xs, vec![0, 200, 300, 500]);
        assert_eq!(
            out[0].ys,
            vec![0, 10, 30, 40],
            "the 240 point is inside the interval"
        );
    }

    #[test]
    fn clear_removes_the_inclusive_interval_and_drops_empty_curves() {
        // Inclusive: both endpoints go, so the point at 200 and at 300 are
        // removed and only the point outside the interval survives.
        let curves = vec![curve("dyn", vec![0, 200, 300], vec![0, 10, 30])];
        let stroke = canonicalize(&[], 200, 300, StrokeMode::Clear, -240, 120);

        let (out, index) =
            apply_stroke_to_curves(&curves, &StrokeTarget::Existing(0), 0, &stroke).expect("apply");

        assert_eq!(index, Some(0), "the curve still has a point at 0");
        assert_eq!(out[0].xs, vec![0]);
        assert_eq!(out[0].ys, vec![0]);

        // Covering every point removes the curve itself.
        let stroke = canonicalize(&[], 0, 300, StrokeMode::Clear, -240, 120);
        let (out, index) =
            apply_stroke_to_curves(&curves, &StrokeTarget::Existing(0), 0, &stroke).expect("apply");
        assert_eq!(index, None, "an emptied curve is removed");
        assert!(out.is_empty());
    }

    #[test]
    fn create_appends_a_curve() {
        let curves: Vec<UCurve> = Vec::new();
        let stroke = canonicalize(
            &[(100, 5), (200, 15)],
            0,
            480,
            StrokeMode::Replace,
            -240,
            120,
        );

        let (out, index) =
            apply_stroke_to_curves(&curves, &StrokeTarget::Create("dyn".into()), 0, &stroke)
                .expect("apply");

        assert_eq!(index, Some(0));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].abbr, "dyn");
        assert_eq!(out[0].xs, vec![0, 100, 200, 480]);
    }

    #[test]
    fn the_command_round_trips_exactly_through_the_undo_stack() {
        let mut project = UProject::create();
        project.tracks.push(domain::UTrack::new("Track1"));
        let mut part = UVoicePart::new("Part1");
        part.curves = vec![curve("dyn", vec![0, 480], vec![0, 20])];
        project.parts.push(UPart::Voice(part));
        let before = project.clone();

        let stroke = canonicalize(
            &[(120, 5), (240, -10)],
            0,
            480,
            StrokeMode::Replace,
            -240,
            120,
        );
        let command =
            ApplyCurveStrokeCommand::new(&project, 0, &StrokeTarget::Existing(0), 0, &stroke)
                .expect("build");

        let mut stack = UndoStack::new();
        stack
            .execute(&mut project, Box::new(command))
            .expect("execute");
        assert_eq!(stack.undo_len(), 1, "one stroke is one step");
        assert_ne!(project, before);
        assert_eq!(project.parts[0], {
            let mut p = before.parts[0].clone();
            if let UPart::Voice(vp) = &mut p {
                vp.curves[0].xs = vec![0, 120, 240, 480];
                vp.curves[0].ys = vec![0, 5, -10, 20];
            }
            p
        });

        stack.undo(&mut project).expect("undo");
        assert_eq!(project, before, "undo must restore the curve list exactly");
        assert_eq!(stack.undo_len(), 0);
        assert!(stack.can_redo());
    }
}
