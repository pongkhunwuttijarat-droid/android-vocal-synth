//! Real-project extraction: cut a real .ustx project (lyrics/pitch as
//! composed) into training entries via the teacher (R2) engine.
//!
//! Unlike synthetic alias phrases, real projects carry TRUE coarticulation:
//! note transitions, real pitch curves, real lyric sequences. Each project
//! phrase (from `build_phrase_inputs`) becomes one TrainEntry whose
//! `aliases` = the phrase's lyric sequence (for display) and phonemes =
//! the feed-derived timing the student aligns to.

use std::path::{Path, PathBuf};

use synth_cli::engine::WorldlineR2Engine;
use synth_cli::pipeline::{self, PhraseRenderer};
use voicebank::Voicebank;

use crate::{PhonemeTiming, TrainEntry};

/// Extract all phrases of a real project file into TrainEntries.
/// `kind` = phonemizer ("english"/"japanese"/None for from_track).
/// Teacher wavs (R2) optionally saved as `{idx:05}.wav` (passed order).
pub fn extract_project(
    project_path: &Path,
    so: &Path,
    mel_path: &Path,
    vocoder_path: &Path,
    vb: &Voicebank,
    kind: Option<pipeline::PhonemizerKind>,
    dump_dir: &Path,
    wav_dir: Option<&Path>,
) -> Result<Vec<(TrainEntry, Vec<f32>)>, String> {
    let project = pipeline::load_project(project_path)?;
    let track = project
        .tracks
        .first()
        .ok_or_else(|| "project has no tracks".to_string())?;
    let part = project
        .parts
        .iter()
        .find_map(|p| match p {
            domain::UPart::Voice(v) => Some(v),
            _ => None,
        })
        .ok_or_else(|| "project has no voice part".to_string())?;
    let kind = kind.unwrap_or_else(|| pipeline::PhonemizerKind::from_track(track));
    let inputs = pipeline::build_phrase_inputs(&project, track, part, vb, kind)?;
    eprintln!(
        "[project-extract] {}: {} phrases",
        project_path.display(),
        inputs.len()
    );

    std::fs::create_dir_all(dump_dir).map_err(|e| format!("dump dir: {e}"))?;
    if let Some(w) = wav_dir {
        std::fs::create_dir_all(w).map_err(|e| format!("wav dir: {e}"))?;
    }
    let engine = WorldlineR2Engine::open(so, mel_path, vocoder_path, false)
        .map_err(|e| format!("open r2 engine: {e}"))?;

    let mut out = Vec::with_capacity(inputs.len());
    for (i, pi) in inputs.iter().enumerate() {
        let phonemes: Vec<PhonemeTiming> = pi
            .input
            .phonemes
            .iter()
            .map(|p| PhonemeTiming {
                phoneme: p.phoneme.clone(),
                position_ms: p.position_ms,
                duration_ms: p.duration_ms,
            })
            .collect();
        let lyrics: Vec<String> = pi.input.notes.iter().map(|n| n.lyric.clone()).collect();
        let dump_path: PathBuf = dump_dir.join(format!("proj-{i:05}.bin"));
        worldline_plugin::r2_engine::set_thread_dump_path(Some(dump_path.clone()));
        let result = engine.render_phrase(&pi.input);
        worldline_plugin::r2_engine::set_thread_dump_path(None);
        let audio = match result {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[skip] phrase {i} ({lyrics:?}): {e}");
                continue;
            }
        };
        let (frames, f0_hz, mel_flat) = read_dump(&dump_path)?;
        let _ = std::fs::remove_file(&dump_path);
        let mel: Vec<Vec<f32>> = mel_flat.chunks_exact(128).map(|c| c.to_vec()).collect();
        // tone/duration: representative from first note (display only —
        // the student's real inputs are phonemes + f0 + pos_frac).
        let tone = pi.input.notes.first().map(|n| n.tone).unwrap_or(60);
        let duration_ms = phonemes.iter().map(|p| p.duration_ms).sum();
        out.push((
            TrainEntry {
                aliases: lyrics,
                tone,
                duration_ms,
                phonemes,
                frames,
                f0_hz,
                mel,
            },
            audio,
        ));
    }
    Ok(out)
}

fn read_dump(path: &Path) -> Result<(usize, Vec<f32>, Vec<f32>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read dump {}: {e}", path.display()))?;
    if bytes.len() < 4 {
        return Err("dump too small".to_string());
    }
    let total = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let need = 4 + total * 4 + total * 128 * 4;
    if bytes.len() != need {
        return Err(format!(
            "dump size mismatch: {} bytes, expected {need} (total={total})",
            bytes.len()
        ));
    }
    let f32s: Vec<f32> = bytes[4..]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let f0 = f32s[..total].to_vec();
    let mel = f32s[total..].to_vec();
    Ok((total, f0, mel))
}
