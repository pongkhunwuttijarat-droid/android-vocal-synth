//! Adapter to the managed OpenUtau phonemizer host.
//!
//! The host receives only an immutable project snapshot and a voicebank path.
//! It returns note-relative aliases; Rust validates their identity, applies
//! OTO/timing rules, and remains responsible for render planning and audio.

use domain::{UProject, UVoicePart};
#[cfg(not(target_arch = "wasm32"))]
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::process::{Command, Stdio};
use voicebank::Voicebank;

pub const JAPANESE_PRESAMP: &str = "OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer";
#[cfg(not(target_arch = "wasm32"))]
const MAX_PROJECT_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_PHONEME_RESULTS: usize = 1_000_000;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Request<'a> {
    protocol: u32,
    project_yaml: String,
    voicebank_directory: &'a str,
    track: i32,
    part: usize,
    phonemizer: &'static str,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Response {
    protocol: u32,
    track: i32,
    part: usize,
    phonemizer: String,
    phones: Vec<Phone>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Phone {
    note_index: usize,
    alias: String,
    position: i32,
    index: i32,
    extended_end: i32,
}

pub fn derives_for_part(
    project: &UProject,
    part: &UVoicePart,
    voicebank: &Voicebank,
) -> Result<Vec<domain::UPhoneme>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (project, part, voicebank);
        return Err(format!(
            "{JAPANESE_PRESAMP} requires the managed OpenUtau host, which is unavailable on Direct-WASM"
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        derive_native(project, part, voicebank)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn derive_native(
    project: &UProject,
    part: &UVoicePart,
    voicebank: &Voicebank,
) -> Result<Vec<domain::UPhoneme>, String> {
    let track_no = part.track_no;
    let mut track_parts: Vec<&UVoicePart> = project
        .parts
        .iter()
        .filter_map(|entry| match entry {
            domain::UPart::Voice(voice) if voice.track_no == track_no => Some(voice),
            _ => None,
        })
        .collect();
    track_parts.sort_by_key(|candidate| candidate.position);
    let part_index = track_parts
        .iter()
        .position(|candidate| std::ptr::eq(*candidate, part))
        .ok_or_else(|| "voice part is not present in the immutable project snapshot".to_string())?;
    let mut snapshot = project.clone();
    snapshot.before_save();
    let yaml = serde_yaml::to_string(&snapshot)
        .map_err(|error| format!("serialize project snapshot for OpenUtau phonemizer: {error}"))?;
    if yaml.len() > MAX_PROJECT_SNAPSHOT_BYTES {
        return Err(format!(
            "OpenUtau phonemizer project snapshot exceeds {MAX_PROJECT_SNAPSHOT_BYTES} bytes"
        ));
    }
    let voicebank_directory = voicebank.path.to_string_lossy().into_owned();
    let request = Request {
        protocol: 1,
        project_yaml: yaml,
        voicebank_directory: &voicebank_directory,
        track: track_no,
        part: part_index,
        phonemizer: JAPANESE_PRESAMP,
    };
    let host = std::env::var_os("RUSTED_UTAU_OPENUTAU_PHONEMIZER_HOST")
        .or_else(|| {
            for candidate in [
                "native/tools/openutau-phonemizer-host/bin/Release/net8.0/OpenUtauPhonemizerHost.dll",
                "tools/openutau-phonemizer-host/bin/Release/net8.0/OpenUtauPhonemizerHost.dll",
            ] {
                let p = std::path::Path::new(candidate);
                if p.exists() {
                    return Some(p.into());
                }
            }
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    let candidate = dir.join("tools/openutau-phonemizer-host/bin/Release/net8.0/OpenUtauPhonemizerHost.dll");
                    if candidate.exists() {
                        return Some(candidate.into());
                    }
                }
            }
            None
        })
        .ok_or_else(|| {
            format!(
                "track selects {JAPANESE_PRESAMP}, but RUSTED_UTAU_OPENUTAU_PHONEMIZER_HOST is not configured"
            )
        })?;
    let dotnet = std::env::var_os("RUSTED_UTAU_DOTNET")
        .or_else(|| {
            if let Some(home) = std::env::var_os("HOME") {
                let user_dotnet = std::path::PathBuf::from(home).join(".dotnet/dotnet");
                if user_dotnet.exists() {
                    return Some(user_dotnet.into());
                }
            }
            None
        })
        .unwrap_or_else(|| "dotnet".into());
    let mut child = Command::new(dotnet)
        .arg(host)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start OpenUtau phonemizer host: {error}"))?;
    let payload = serde_json::to_vec(&request)
        .map_err(|error| format!("encode OpenUtau phonemizer request: {error}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "OpenUtau host stdin was not available".to_string())?
        .write_all(&payload)
        .map_err(|error| format!("send project snapshot to OpenUtau host: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait for OpenUtau phonemizer host: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "OpenUtau phonemizer host failed ({}): {}",
            output.status,
            detail.trim()
        ));
    }
    let response: Response = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("decode OpenUtau phonemizer response: {error}"))?;
    if response.protocol != 1
        || response.track != track_no
        || response.part != part_index
        || response.phonemizer != JAPANESE_PRESAMP
    {
        return Err(
            "OpenUtau phonemizer response identity/version does not match the request".into(),
        );
    }
    if response.phones.len() > MAX_PHONEME_RESULTS {
        return Err(format!(
            "OpenUtau phonemizer returned more than {MAX_PHONEME_RESULTS} phones"
        ));
    }
    let mut source_note_indexes: Vec<usize> = (0..part.notes.len()).collect();
    source_note_indexes.sort_by_key(|&index| part.notes[index].position);
    let mut phonemes = Vec::with_capacity(response.phones.len());
    let mut extended_ends = Vec::with_capacity(response.phones.len());
    for phone in response.phones {
        let source_note_index = source_note_indexes
            .get(phone.note_index)
            .copied()
            .ok_or_else(|| {
                format!(
                    "OpenUtau phonemizer returned invalid note index {}",
                    phone.note_index
                )
            })?;
        let note = &part.notes[source_note_index];
        let part_position = note.position.saturating_add(phone.position);
        let duration = phone.extended_end.saturating_sub(part_position);
        if duration <= 0 {
            return Err(format!(
                "OpenUtau phonemizer returned a non-positive duration for note {}",
                phone.note_index
            ));
        }
        phonemes.push(phonemizer::make_phoneme(
            phone.alias,
            phone.position,
            duration,
            phone.index,
            note.tone,
            source_note_index,
        ));
        extended_ends.push(phone.extended_end);
    }
    let mut ordered: Vec<usize> = (0..phonemes.len()).collect();
    ordered.sort_by_key(|&index| {
        let phone = &phonemes[index];
        let note = &part.notes[phone.parent.expect("host phone has a validated parent")];
        note.position.saturating_add(phone.position)
    });
    for pair in ordered.windows(2) {
        let current_index = pair[0];
        let next_index = pair[1];
        let current_note_index = phonemes[current_index]
            .parent
            .expect("host phone has a validated parent");
        let next_note_index = phonemes[next_index]
            .parent
            .expect("host phone has a validated parent");
        let current_note_position = part.notes[current_note_index].position;
        let next_note_position = part.notes[next_note_index].position;
        let current_position =
            current_note_position.saturating_add(phonemes[current_index].position);
        let next_position = next_note_position.saturating_add(phonemes[next_index].position);
        let safe_position = current_position.min(next_position.saturating_sub(10));
        phonemes[current_index].position = safe_position.saturating_sub(current_note_position);
        let end = extended_ends[current_index].min(next_position);
        phonemes[current_index].duration = end
            .saturating_sub(current_note_position.saturating_add(phonemes[current_index].position));
        if phonemes[current_index].duration <= 0 {
            return Err(format!(
                "OpenUtau phonemizer produced overlapping phones at source note {current_note_index}"
            ));
        }
    }
    Ok(phonemes)
}
