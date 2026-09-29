//! Read-only view DTOs shared by the session snapshot (§6.3) and the change
//! payloads of a mutation result (§6.4.3).
//!
//! Both surfaces describe the same entities, so the shapes live in one place:
//! a client patching its disposable cache from `changes[].data` must see
//! exactly the fields it would have received in a snapshot.

use domain::{UCurve, UNote, UProject, UTrack, UVoicePart};
use serde::Serialize;

use crate::identity::{voice_parts_of_track, IdentityTable};

/// §6.3 session snapshot.
#[derive(Debug, Serialize)]
pub(crate) struct Snapshot {
    pub session_id: String,
    pub revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub ids_stable: bool,
    pub project: ProjectView,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectView {
    pub id: String,
    pub name: String,
    pub bpm: f64,
    pub time_signature: TimeSignatureView,
    pub tracks: Vec<TrackView>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TimeSignatureView {
    pub numerator: i32,
    pub denominator: i32,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TrackView {
    pub id: String,
    pub name: String,
    pub singer_id: Option<String>,
    pub mute: bool,
    pub solo: bool,
    pub volume_db: f64,
    pub pan: f64,
    pub parts: Vec<PartView>,
}

/// Track metadata without its parts — the payload of `track.changed`, which is
/// small enough to send on every mixer edit.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct TrackMetaView {
    pub id: String,
    pub name: String,
    pub singer_id: Option<String>,
    pub mute: bool,
    pub solo: bool,
    pub volume_db: f64,
    pub pan: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PartView {
    pub id: String,
    pub position_ticks: i32,
    pub duration_ticks: i32,
    pub notes: Vec<NoteView>,
    pub curves: Vec<CurveView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NoteView {
    pub id: String,
    pub position_ticks: i32,
    pub duration_ticks: i32,
    pub tone: i32,
    pub lyric: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CurveView {
    pub id: String,
    pub abbr: String,
    pub points: Vec<PointView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PointView {
    pub tick: i32,
    pub value: i32,
}

pub(crate) fn note_view(id: &str, note: &UNote) -> NoteView {
    NoteView {
        id: id.to_string(),
        position_ticks: note.position,
        duration_ticks: note.duration,
        tone: note.tone,
        lyric: note.lyric.clone(),
    }
}

pub(crate) fn curve_view(id: &str, curve: &UCurve) -> CurveView {
    CurveView {
        id: id.to_string(),
        abbr: curve.abbr.clone(),
        points: curve
            .xs
            .iter()
            .zip(curve.ys.iter())
            .map(|(tick, value)| PointView {
                tick: *tick,
                value: *value,
            })
            .collect(),
    }
}

pub(crate) fn track_meta_view(id: &str, track: &UTrack) -> TrackMetaView {
    TrackMetaView {
        id: id.to_string(),
        name: track.track_name.clone(),
        singer_id: track.singer.clone(),
        mute: track.mute,
        solo: track.solo,
        volume_db: track.volume,
        pan: track.pan,
    }
}

pub(crate) fn part_view(
    id: &str,
    vp: &UVoicePart,
    notes: Vec<NoteView>,
    curves: Vec<CurveView>,
) -> PartView {
    PartView {
        id: id.to_string(),
        position_ticks: vp.position,
        duration_ticks: vp.duration,
        notes,
        curves,
    }
}

/// Assemble the full snapshot, resolving every ID from `ids`.
///
/// Track/part ordering comes from [`voice_parts_of_track`] so it matches the
/// ordering the identity table was reconciled with.
pub(crate) fn snapshot(
    session_id: &str,
    revision: u64,
    can_undo: bool,
    can_redo: bool,
    ids_stable: bool,
    project: &UProject,
    ids: &IdentityTable,
) -> Snapshot {
    let time_signature = project
        .time_signatures
        .first()
        .map(|ts| TimeSignatureView {
            numerator: ts.beat_per_bar,
            denominator: ts.beat_unit,
        })
        .unwrap_or(TimeSignatureView {
            numerator: 4,
            denominator: 4,
        });
    let bpm = project.tempos.first().map(|t| t.bpm).unwrap_or(project.bpm);

    let mut tracks = Vec::with_capacity(project.tracks.len());
    for (track_index, track) in project.tracks.iter().enumerate() {
        let voice_parts = voice_parts_of_track(project, track_index);
        let mut parts = Vec::with_capacity(voice_parts.len());
        for (part_index, vp) in voice_parts.iter().enumerate() {
            let id = ids
                .part_id(track_index, part_index)
                .map(|id| id.as_str().to_string())
                .unwrap_or_default();
            let notes = vp
                .notes
                .iter()
                .enumerate()
                .map(|(note_index, note)| {
                    let note_id = ids
                        .note_id(track_index, part_index, note_index)
                        .map(|id| id.as_str().to_string())
                        .unwrap_or_default();
                    note_view(&note_id, note)
                })
                .collect();
            let curves = vp
                .curves
                .iter()
                .enumerate()
                .map(|(curve_index, curve)| {
                    let curve_id = ids
                        .curve_id(track_index, part_index, curve_index)
                        .map(|id| id.as_str().to_string())
                        .unwrap_or_default();
                    curve_view(&curve_id, curve)
                })
                .collect();
            parts.push(part_view(&id, vp, notes, curves));
        }
        tracks.push(TrackView {
            id: ids
                .track_id(track_index)
                .map(|id| id.as_str().to_string())
                .unwrap_or_default(),
            name: track.track_name.clone(),
            singer_id: track.singer.clone(),
            mute: track.mute,
            solo: track.solo,
            volume_db: track.volume,
            pan: track.pan,
            parts,
        });
    }

    Snapshot {
        session_id: session_id.to_string(),
        revision,
        can_undo,
        can_redo,
        ids_stable,
        project: ProjectView {
            id: ids.project_id.as_str().to_string(),
            name: project.name.clone(),
            bpm,
            time_signature,
            tracks,
        },
    }
}
