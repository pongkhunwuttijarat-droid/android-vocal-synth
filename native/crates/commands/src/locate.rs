//! Internal helpers for locating domain objects inside a project.
//!
//! Commands snapshot values at construction time and must find them again
//! when they execute/unexecute, because earlier commands in the same undo
//! group may have shifted list indices. Every locator therefore tries the
//! construction-time index first (fast path) and falls back to a value
//! scan (robust path).

use domain::{UNote, UPart, UProject, UVoicePart};

/// Stable fingerprint of a voice part: `(track_no, position, name)`.
///
/// Used to re-locate a part after earlier commands in the same undo group
/// removed a different part and shifted `UProject.parts` indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartFingerprint {
    pub track_no: i32,
    pub position: i32,
    pub name: String,
}

impl PartFingerprint {
    pub(crate) fn of(part: &UVoicePart) -> Self {
        PartFingerprint {
            track_no: part.track_no,
            position: part.position,
            name: part.name.clone(),
        }
    }
}

fn fingerprint_matches(part: &UPart, fp: &PartFingerprint) -> bool {
    match part {
        UPart::Voice(vp) => {
            vp.track_no == fp.track_no && vp.position == fp.position && vp.name == fp.name
        }
        UPart::Wave(_) => false,
    }
}

/// Index of the voice part with the given fingerprint: the hinted index
/// when it still matches, otherwise the first match in `project.parts`.
pub(crate) fn locate_part_index(
    project: &UProject,
    hint: usize,
    fp: &PartFingerprint,
) -> Option<usize> {
    if project
        .parts
        .get(hint)
        .is_some_and(|p| fingerprint_matches(p, fp))
    {
        return Some(hint);
    }
    project
        .parts
        .iter()
        .position(|p| fingerprint_matches(p, fp))
}

/// Mutable access to the voice part with the given fingerprint.
pub(crate) fn locate_part_mut<'a>(
    project: &'a mut UProject,
    hint: usize,
    fp: &PartFingerprint,
) -> Result<&'a mut UVoicePart, String> {
    let idx = locate_part_index(project, hint, fp)
        .ok_or_else(|| "voice part not found in project".to_string())?;
    match project.parts.get_mut(idx) {
        Some(UPart::Voice(vp)) => Ok(vp),
        _ => Err("part at index is not a voice part".to_string()),
    }
}

/// Index of the note equal to `expected` inside `part`.
///
/// Prefers an exact value match; if the note was already edited by an
/// earlier command in the same group (so its value no longer equals
/// `expected`), falls back to the construction-time index `hint` when the
/// note there equals `expected` or `alt` (the other side of the edit).
pub(crate) fn locate_note_index(
    part: &UVoicePart,
    hint: usize,
    expected: &UNote,
    alt: Option<&UNote>,
) -> Result<usize, String> {
    if let Some(i) = part.notes.iter().position(|n| n == expected) {
        return Ok(i);
    }
    if let Some(n) = part.notes.get(hint) {
        if n == expected || alt.is_some_and(|a| n == a) {
            return Ok(hint);
        }
    }
    Err("note not found in part".to_string())
}

/// `UVoicePart.GetMinDurTickForNoteEdit(project, endTick)`: the tick of the
/// next bar beat after `endTick` (relative to the part position).
///
/// Used by note commands to grow the part duration when an edit would push
/// a note past the part end (OpenUtau `AddNoteCommand`/`MoveNoteCommand`/
/// `ResizeNoteCommand` constructors).
pub(crate) fn min_dur_tick_for_note_edit(
    project: &UProject,
    part: &UVoicePart,
    end_tick: i32,
) -> i32 {
    let end = part.position + end_tick;
    let (bar, beat, _) = project.time_axis.tick_to_bar_beat(end);
    project.time_axis.bar_beat_to_tick(bar, beat + 1) - part.position
}

/// Restore position for an item re-inserted by `unexecute` after a removal.
///
/// Prefers the construction-time index `hint` when the items around it are
/// still consistent with the canonical ordering (`key_of` is non-decreasing
/// across the list). Otherwise — e.g. when earlier removals in the same
/// undo group shifted the list — falls back to the canonical (sorted)
/// position, which restores the exact original order whenever the list was
/// in canonical order before the removals.
pub(crate) fn restore_insert_index<T, K: Ord>(
    items: &[T],
    hint: usize,
    key_of: impl Fn(&T) -> K,
    item_key: &K,
) -> usize {
    if hint <= items.len() {
        let before_ok = hint == 0 || key_of(&items[hint - 1]) <= *item_key;
        let after_ok = hint == items.len() || key_of(&items[hint]) >= *item_key;
        if before_ok && after_ok {
            return hint;
        }
    }
    items
        .iter()
        .position(|item| key_of(item) > *item_key)
        .unwrap_or(items.len())
}
