//! Read-only view DTOs shared by the session snapshot (§6.3) and the change
//! payloads of a mutation result (§6.4.3).
//!
//! Both surfaces describe the same entities, so the shapes live in one place:
//! a client patching its disposable cache from `changes[].data` must see
//! exactly the fields it would have received in a snapshot.

use domain::{
    UCurve, UNote, UPart, UProject, UTempo, UTimeSignature, UTrack, UVoicePart, UWavePart,
};
use serde::Serialize;

use crate::identity::{parts_of_track, IdentityTable};

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
    pub comment: String,
    pub key: i32,
    pub tempos: Vec<TempoView>,
    pub time_signatures: Vec<TimeSignatureMapView>,
    pub bpm: f64,
    pub time_signature: TimeSignatureView,
    pub tracks: Vec<TrackView>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TempoView {
    pub position_tick: i32,
    pub bpm: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TimeSignatureMapView {
    pub bar_position: i32,
    pub numerator: i32,
    pub denominator: i32,
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
    pub mix_fx: Option<domain::UMixFx>,
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
    pub mix_fx: Option<domain::UMixFx>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PartView {
    pub id: String,
    pub kind: &'static str,
    pub name: String,
    pub comment: String,
    pub position_ticks: i32,
    pub duration_ticks: i32,
    pub media_ref: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pronunciation: Option<domain::UPronunciation>,
    /// The note's pitch curve, in the units the domain and USTX already use:
    /// `x` is milliseconds from the note start and `y` is tenths of a semitone
    /// relative to `tone`. Without it in the view a host could send a pitch
    /// curve and then have no way to read back what it drew.
    pub pitch: domain::UPitch,
    pub vibrato: domain::UVibrato,
    pub tuning_cents: i32,
    pub phoneme_expressions: Vec<domain::UExpression>,
    pub phoneme_overrides: Vec<domain::UPhonemeOverride>,
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

pub(crate) fn note_view(
    id: &str,
    note: &UNote,
    pronunciation: Option<&domain::UPronunciation>,
) -> NoteView {
    NoteView {
        id: id.to_string(),
        position_ticks: note.position,
        duration_ticks: note.duration,
        tone: note.tone,
        lyric: note.lyric.clone(),
        pronunciation: pronunciation.cloned(),
        pitch: note.pitch.clone(),
        vibrato: note.vibrato.clone(),
        tuning_cents: note.tuning,
        phoneme_expressions: note.phoneme_expressions.clone(),
        phoneme_overrides: note.phoneme_overrides.clone(),
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
        mix_fx: track.mix_fx.clone(),
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
        kind: "voice",
        name: vp.name.clone(),
        comment: vp.comment.clone(),
        position_ticks: vp.position,
        duration_ticks: vp.duration,
        media_ref: None,
        notes,
        curves,
    }
}

/// Assemble the full snapshot, resolving every ID from `ids`.
///
/// Track/part ordering comes from [`parts_of_track`] so it matches the
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
        let track_parts = parts_of_track(project, track_index);
        let mut parts = Vec::with_capacity(track_parts.len());
        for (part_index, project_part) in track_parts.iter().enumerate() {
            let id = ids
                .part_id(track_index, part_index)
                .map(|id| id.as_str().to_string())
                .unwrap_or_default();
            match project_part {
                UPart::Voice(vp) => {
                    let notes = vp
                        .notes
                        .iter()
                        .enumerate()
                        .map(|(note_index, note)| {
                            let note_id = ids
                                .note_id(track_index, part_index, note_index)
                                .map(|id| id.as_str().to_string())
                                .unwrap_or_default();
                            note_view(
                                &note_id,
                                note,
                                project.rusted_utau_pronunciations.get(&note_id),
                            )
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
                UPart::Wave(wp) => parts.push(wave_part_view(&id, wp, project)),
            }
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
            mix_fx: track.mix_fx.clone(),
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
            comment: project.comment.clone(),
            key: project.key,
            tempos: project.tempos.iter().map(tempo_view).collect(),
            time_signatures: project
                .time_signatures
                .iter()
                .map(time_signature_view)
                .collect(),
            bpm,
            time_signature,
            tracks,
        },
    }
}

pub(crate) fn wave_part_view(id: &str, part: &UWavePart, project: &UProject) -> PartView {
    let full_end_tick = if part.file_duration_ms.is_finite() && part.file_duration_ms > 0.0 {
        project
            .time_axis
            .ms_to_tick(project.time_axis.tick_to_ms(part.position as f64) + part.file_duration_ms)
    } else {
        0
    };
    PartView {
        id: id.to_string(),
        kind: "wave",
        name: part.name.clone(),
        comment: part.comment.clone(),
        position_ticks: part.position,
        duration_ticks: full_end_tick
            .saturating_sub(part.position)
            .saturating_sub(part.skip)
            .saturating_sub(part.trim),
        media_ref: part.relative_path.clone(),
        notes: Vec::new(),
        curves: Vec::new(),
    }
}

pub(crate) fn tempo_view(tempo: &UTempo) -> TempoView {
    TempoView {
        position_tick: tempo.position,
        bpm: tempo.bpm,
    }
}

pub(crate) fn time_signature_view(sig: &UTimeSignature) -> TimeSignatureMapView {
    TimeSignatureMapView {
        bar_position: sig.bar_position,
        numerator: sig.beat_per_bar,
        denominator: sig.beat_unit,
    }
}

pub(crate) fn project_meta_view(project_id: &str, project: &UProject) -> serde_json::Value {
    let bpm = project
        .tempos
        .first()
        .map(|tempo| tempo.bpm)
        .unwrap_or(project.bpm);
    let time_signature = project
        .time_signatures
        .first()
        .map(|sig| TimeSignatureView {
            numerator: sig.beat_per_bar,
            denominator: sig.beat_unit,
        })
        .unwrap_or(TimeSignatureView {
            numerator: 4,
            denominator: 4,
        });
    serde_json::json!({
        "id": project_id,
        "name": project.name,
        "comment": project.comment,
        "key": project.key,
        "tempos": project.tempos.iter().map(tempo_view).collect::<Vec<_>>(),
        "time_signatures": project.time_signatures.iter().map(time_signature_view).collect::<Vec<_>>(),
        "bpm": bpm,
        "time_signature": time_signature,
    })
}
