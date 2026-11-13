//! Ustx codec — Rust .ustx serialize/parse as an engine plugin (.so).
//!
//! FFI contract (mirror of mixer-fx's dlopen pattern):
//!   UstxDecode(ustx_yaml, len) -> json  — parse a `.ustx` file into a
//!                                         compact JSON view (the fields
//!                                         the app's project store needs)
//!   UstxEncode(json, len)       -> yaml  — build a `.ustx` file from the
//!                                         same JSON view
//!
//! The app (Dart) only ever handles bytes: it sends/receives JSON or
//! YAML strings and never knows the `.ustx` wire format. Ownership of the
//! returned buffer transfers to the caller (free with `UstxFree`).
//!
//! JSON view (what the app sees):
//! ```json
//! {
//!   "name": "song",
//!   "bpm": 110,
//!   "time_signature": "4/4",
//!   "tracks": [
//!     { "name": "Vocal", "color": "#7358E9", "notes": [
//!         { "lyric": "la[l A]", "pitch": 0, "position": 0.5, "duration": 1.0 }
//!     ]}
//!   ]
//! }
//! ```
//!
//! Note: `position`/`duration` are in BEATS in the JSON view; `.ustx`
//! stores ticks + tempo tracks, so encode/decode convert (192 ticks/beat
//! default, matching the app's current writer).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use domain::{UPart, UProject, UTrack};

/// Default ticks per beat — MUST match the app's `buildUstx` writer
/// (480 — the standard OpenUtau convention, shared with the feed/dsdur
/// ms-per-tick math). A 192 value here desynced the note positions from
/// the dsdur (×0.4) and rushed every render.
const TICKS_PER_BEAT: i32 = 480;

#[derive(serde::Serialize, serde::Deserialize)]
struct JsonNote {
    lyric: String,
    pitch: i32,
    position: f64,
    duration: f64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JsonCurve {
    #[serde(default)]
    abbr: String,
    #[serde(default)]
    xs: Vec<i32>,
    #[serde(default)]
    ys: Vec<i32>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JsonTrack {
    name: String,
    color: String,
    #[serde(default)]
    notes: Vec<JsonNote>,
    /// Expression curves of the track's first voice part (pitch "pitd",
    /// dynamics "dyn", ...). Round-tripped so the editor's drawn curve
    /// survives save/load.
    #[serde(default)]
    curves: Vec<JsonCurve>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JsonProject {
    name: String,
    bpm: f64,
    #[serde(default = "default_ts")]
    time_signature: String,
    #[serde(default)]
    tracks: Vec<JsonTrack>,
}

fn default_ts() -> String {
    "4/4".to_string()
}

/// Convert a loaded UProject into the compact JSON view.
fn project_to_json(project: &UProject) -> Result<JsonProject, String> {
    let bpm = project.tempos.first().map(|t| t.bpm).unwrap_or(120.0);
    let tracks = project
        .tracks
        .iter()
        .enumerate()
        .map(|(track_no, t): (usize, &UTrack)| {
            let mut notes = Vec::new();
            // Voice parts live on the PROJECT (not the track); pick the
            // parts whose track_no matches this track's position.
            for part in &project.parts {
                if let UPart::Voice(voice) = part {
                    if voice.track_no as usize == track_no {
                        for n in &voice.notes {
                            notes.push(JsonNote {
                                lyric: n.lyric.clone(),
                                pitch: n.tone,
                                position: n.position as f64 / TICKS_PER_BEAT as f64,
                                duration: n.duration as f64 / TICKS_PER_BEAT as f64,
                            });
                        }
                    }
                }
            }
            JsonTrack {
                name: t.track_name.clone(),
                color: t.track_color.clone(),
                notes,
                curves: track_curves(project, track_no),
            }
        })
        .collect();
    Ok(JsonProject {
        name: project.name.clone(),
        bpm,
        time_signature: default_ts(),
        tracks,
    })
}

/// Expression curves of a track's FIRST voice part (the editor keeps one
/// curve list per track). Empty when the track has no voice part.
fn track_curves(project: &UProject, track_no: usize) -> Vec<JsonCurve> {
    let mut out = Vec::new();
    for part in &project.parts {
        if let UPart::Voice(voice) = part {
            if voice.track_no as usize == track_no {
                for c in &voice.curves {
                    out.push(JsonCurve {
                        abbr: c.abbr.clone(),
                        xs: c.xs.clone(),
                        ys: c.ys.clone(),
                    });
                }
                break; // first voice part only
            }
        }
    }
    out
}

/// Build a UProject from the JSON view.
fn json_to_project(json: &JsonProject) -> Result<UProject, String> {
    let mut project = UProject::create();
    project.name = json.name.clone();
    project.ustx_version = Some(domain::UstxVersion::new(0, 9));
    if let Some(tempo) = project.tempos.first_mut() {
        tempo.bpm = json.bpm;
    }
    // Rebuild the time axis after mutating the tempo.
    project
        .time_axis
        .build_segments(&project.time_signatures, &project.tempos)
        .map_err(|e| format!("time axis: {e}"))?;
    // Rebuild tracks: the JSON view carries one track per entry; voice
    // parts carry the notes (track_no = position).
    project.tracks.clear();
    for (i, t) in json.tracks.iter().enumerate() {
        let mut track = UTrack::new(t.name.clone());
        track.track_color = t.color.clone();
        project.tracks.push(track);
        let mut part = domain::UVoicePart::new(format!("Part{}", i + 1));
        part.track_no = i as i32;
        part.position = 0;
        part.duration = t
            .notes
            .iter()
            .map(|n| (n.duration * TICKS_PER_BEAT as f64).round() as i32)
            .max()
            .unwrap_or(192);
        for n in &t.notes {
            let mut unote = project.create_note_at(
                n.pitch,
                (n.position * TICKS_PER_BEAT as f64).round() as i32,
                (n.duration * TICKS_PER_BEAT as f64).round() as i32,
            );
            unote.lyric = n.lyric.clone();
            part.notes.push(unote);
        }
        // Restore the track's expression curves (round-trip the editor's
        // drawn pitch/dynamics curve).
        part.curves = t
            .curves
            .iter()
            .map(|c| domain::UCurve {
                abbr: c.abbr.clone(),
                xs: c.xs.clone(),
                ys: c.ys.clone(),
            })
            .collect();
        // OpenUtau contract: the WIRE format carries parts in
        // `voice_parts` (the `parts` field is #[serde(skip)] in-memory);
        // after_load merges voice_parts → parts. Keep both in sync.
        project.parts.push(UPart::Voice(part.clone()));
        project.voice_parts.get_or_insert_with(Vec::new).push(part);
    }
    project
        .time_axis
        .build_segments(&project.time_signatures, &project.tempos)
        .map_err(|e| format!("time axis rebuild: {e}"))?;
    Ok(project)
}

// ---------------------------------------------------------------------------
// FFI surface
// ---------------------------------------------------------------------------

/// Decode `.ustx` YAML into the JSON view. Returns a malloc-free C string
/// owned by the caller (free with `UstxFree`). NULL on error.
#[unsafe(no_mangle)]
pub extern "C" fn UstxDecode(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return ptr::null_mut();
    }
    let yaml = match unsafe { CStr::from_ptr(input) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    // `UProject::parse_yaml` is the canonical loader and strips the UTF-8 BOM
    // every OpenUtau-written `.ustx` starts with. Parsing with serde_yaml
    // directly here made this FFI entry reject every real project, with a
    // misleading "more than one document" error (the files have zero `---`).
    let project: UProject = match UProject::parse_yaml(yaml) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ustx-codec: {e}");
            return ptr::null_mut();
        }
    };
    match project_to_json(&project) {
        Ok(view) => match serde_json::to_string(&view) {
            Ok(s) => match CString::new(s) {
                Ok(c) => c.into_raw(),
                Err(_) => ptr::null_mut(),
            },
            Err(_) => ptr::null_mut(),
        },
        Err(e) => {
            eprintln!("ustx-codec: {e}");
            ptr::null_mut()
        }
    }
}

/// Encode the JSON view into `.ustx` YAML. Returns a C string owned by
/// the caller (free with `UstxFree`). NULL on error.
#[unsafe(no_mangle)]
pub extern "C" fn UstxEncode(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return ptr::null_mut();
    }
    let json_str = match unsafe { CStr::from_ptr(input) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    let view: JsonProject = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ustx-codec: json parse: {e}");
            return ptr::null_mut();
        }
    };
    let project = match json_to_project(&view) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ustx-codec: {e}");
            return ptr::null_mut();
        }
    };
    match serde_yaml::to_string(&project) {
        Ok(s) => match CString::new(s) {
            Ok(c) => c.into_raw(),
            Err(_) => ptr::null_mut(),
        },
        Err(_) => ptr::null_mut(),
    }
}

/// Free a buffer returned by [`UstxDecode`] / [`UstxEncode`].
#[unsafe(no_mangle)]
pub extern "C" fn UstxFree(p: *mut c_char) {
    if !p.is_null() {
        unsafe {
            drop(CString::from_raw(p));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"ustx_version: '0.9'
name: midnight_demo
resolution_ppq: 480
tempos:
  - position: 0
    bpm: 110
time_signatures:
  - bar_position: 0
    beat_per_bar: 4
    beat_unit: 4
tracks:
  - track_name: Vocal Scale
    color: '#7358E9'
    singer: ''
    phonemizer: ''
parts:
  - name: Part1
    track_no: 0
    position: 0
    duration: 480
    notes:
      - position: 240
        duration: 480
        tone: 0
        lyric: la[l A]
"#;

    #[test]
    fn decode_sample_yaml_to_json() {
        // The .ustx wire format stores parts under `voice_parts:` (the
        // `parts:` list is in-memory, #[serde(skip)]); after_load merges
        // voice_parts → parts. Use the real wire shape here.
        let c = CString::new(SAMPLE.replace("parts:", "voice_parts:")).unwrap();
        let out = UstxDecode(c.as_ptr());
        assert!(!out.is_null(), "UstxDecode returned NULL");
        let s = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        unsafe { UstxFree(out) };
        let view: JsonProject = serde_json::from_str(&s).unwrap();
        assert_eq!(view.tracks.len(), 1);
        assert_eq!(view.tracks[0].name, "Vocal Scale");
        assert_eq!(view.tracks[0].notes.len(), 1);
        assert_eq!(view.tracks[0].notes[0].lyric, "la[l A]");
        assert_eq!(view.tracks[0].notes[0].pitch, 0);
        // 240 ticks / 480 = 0.5 beats
        assert!((view.tracks[0].notes[0].position - 0.5).abs() < 1e-9);
        // 480 ticks / 480 = 1.0 beat
        assert!((view.tracks[0].notes[0].duration - 1.0).abs() < 1e-9);
    }

    #[test]
    fn null_input_returns_null() {
        assert!(UstxDecode(ptr::null()).is_null());
        assert!(UstxEncode(ptr::null()).is_null());
    }

    #[test]
    fn decode_accepts_a_utf8_bom_like_every_real_openutau_file() {
        // OpenUtau saves with `File.WriteAllText(..., Encoding.UTF8)`, which
        // emits a BOM preamble (`core/OpenUtau.Core/Format/USTx.cs`). Before
        // this entry used `UProject::parse_yaml`, a BOM made serde_yaml see two
        // documents and this returned NULL for every real project — with a
        // misleading "more than one document" error, on files that contain no
        // document separators at all.
        let body = SAMPLE.replace("parts:", "voice_parts:");
        let with_bom = CString::new(format!("\u{feff}{body}")).unwrap();
        let out = UstxDecode(with_bom.as_ptr());
        assert!(
            !out.is_null(),
            "UstxDecode must accept a BOM-prefixed .ustx — this is what every \
             real OpenUtau file starts with"
        );
        let s = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        unsafe { UstxFree(out) };
        let view: JsonProject = serde_json::from_str(&s).expect("decode json");
        assert_eq!(view.tracks.len(), 1);
        assert_eq!(view.tracks[0].name, "Vocal Scale");
        assert_eq!(
            view.tracks[0].notes[0].lyric, "la[l A]",
            "a BOM must not change the decoded document"
        );
    }

    #[test]
    fn curves_round_trip_through_encode_decode() {
        // The JSON view carries the track's expression curves; they must
        // survive encode → decode (the editor's drawn curve is saved via
        // this path).
        let json = serde_json::json!({
            "name": "song",
            "bpm": 110,
            "time_signature": "4/4",
            "tracks": [{
                "name": "Vocal",
                "color": "#000000",
                "notes": [
                    {"lyric": "la[l A]", "pitch": 0, "position": 0.0, "duration": 1.0}
                ],
                "curves": [{
                    "abbr": "pitd",
                    "xs": [0, 5, 10],
                    "ys": [0, 100, -50]
                }]
            }]
        });
        let c = CString::new(json.to_string()).unwrap();
        let yaml_out = UstxEncode(c.as_ptr());
        assert!(!yaml_out.is_null());
        let yaml = unsafe { CStr::from_ptr(yaml_out) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { UstxFree(yaml_out) };

        // Decode back and check the curve survived.
        let c2 = CString::new(yaml).unwrap();
        let json_out = UstxDecode(c2.as_ptr());
        assert!(!json_out.is_null());
        let s = unsafe { CStr::from_ptr(json_out) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { UstxFree(json_out) };
        let view: JsonProject = serde_json::from_str(&s).unwrap();
        let curves = &view.tracks[0].curves;
        assert_eq!(curves.len(), 1);
        assert_eq!(curves[0].abbr, "pitd");
        assert_eq!(curves[0].xs, vec![0, 5, 10]);
        assert_eq!(curves[0].ys, vec![0, 100, -50]);
    }
}
