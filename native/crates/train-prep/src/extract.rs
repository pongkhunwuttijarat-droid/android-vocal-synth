//! Extraction: render each phrase through the R2 teacher and capture
//! (score-f0, mel) via the env-gated WORLD_DUMP_MEL_FILE hook.

use std::io::Write;
use std::path::{Path, PathBuf};

use synth_cli::engine::WorldlineR2Engine;
use synth_cli::pipeline::{self, PhraseRenderer};
use voicebank::Voicebank;

use crate::{PhonemeTiming, PhraseSpec, TrainEntry};

/// Compact binary train-set format (".tsbin") — the JSONL is ~5× the
/// binary size and takes seconds to parse; the binary loads in ms.
/// Magic "TSBN" + little-endian fields (see moe-train::data reader).
pub fn write_tsbin(path: &Path, entries: &[TrainEntry]) -> Result<(), String> {
    let mut buf: Vec<u8> = Vec::with_capacity(entries.len() * 4096);
    buf.extend_from_slice(b"TSBN");
    buf.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        buf.extend_from_slice(&(e.aliases.len() as u32).to_le_bytes());
        for a in &e.aliases {
            let b = a.as_bytes();
            buf.extend_from_slice(&(b.len() as u16).to_le_bytes());
            buf.extend_from_slice(b);
        }
        buf.extend_from_slice(&e.tone.to_le_bytes());
        buf.extend_from_slice(&e.duration_ms.to_le_bytes());
        buf.extend_from_slice(&(e.phonemes.len() as u32).to_le_bytes());
        for p in &e.phonemes {
            let b = p.phoneme.as_bytes();
            buf.extend_from_slice(&(b.len() as u16).to_le_bytes());
            buf.extend_from_slice(b);
            buf.extend_from_slice(&p.position_ms.to_le_bytes());
            buf.extend_from_slice(&p.duration_ms.to_le_bytes());
        }
        buf.extend_from_slice(&(e.frames as u32).to_le_bytes());
        for f in &e.f0_hz {
            buf.extend_from_slice(&f.to_le_bytes());
        }
        for m in &e.mel {
            for v in m {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    std::fs::write(path, &buf).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

/// Build a RenderInput for a phrase spec — mirrors
/// `pipeline::synth_note_input` but for 1..n notes (each alias as a
/// phonetic hint so the phonemizer emits it verbatim).
pub fn phrase_input(vb: &Voicebank, spec: &PhraseSpec) -> Result<pipeline::PhraseInput, String> {
    let project = domain_lite_project(spec)?;
    let part = &project.1;
    let phrases = pipeline::build_phrase_inputs(
        &project.0,
        &project.0.tracks[0],
        part,
        vb,
        // Universal (not legacy English): same path as production
        // inference (`from_track` default). English hard-match falls back
        // to the lyric verbatim on unknown words, producing phonemes the
        // bank can't render; Universal letter-splits + NearestPhoneme-maps.
        pipeline::PhonemizerKind::Universal,
    )?;
    phrases
        .into_iter()
        .next()
        .ok_or_else(|| "no phrase derived".to_string())
}

fn domain_lite_project(
    spec: &PhraseSpec,
) -> Result<(domain::UProject, domain::UVoicePart), String> {
    let project = domain::UProject::create();
    let ticks = project.time_axis.ms_to_tick(spec.duration_ms).max(1);
    let mut notes = Vec::with_capacity(spec.aliases.len());
    for (i, alias) in spec.aliases.iter().enumerate() {
        let mut note = project.create_note_at(spec.tone, i as i32 * ticks, ticks);
        note.lyric = format!("{alias}[{alias}]");
        notes.push(note);
    }
    let part = domain::UVoicePart {
        position: 0,
        notes,
        ..Default::default()
    };
    Ok((project, part))
}

/// Read the WORLD_DUMP_MEL_FILE payload:
/// [u32 total][f32 f0 × total][f32 mel × total × 128].
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

/// Render `spec` with the teacher engine and return the TrainEntry.
/// `dump_dir` holds the transient mel dump file (deleted after read);
/// the dump path is per-thread (parallel workers don't collide).
pub fn extract_phrase(
    engine: &WorldlineR2Engine,
    vb: &Voicebank,
    spec: &PhraseSpec,
    dump_dir: &Path,
    idx: usize,
) -> Result<(TrainEntry, Vec<f32>), String> {
    let input = phrase_input(vb, spec)?;

    // Per-phoneme timing from the feed (the student's alignment).
    let phonemes: Vec<PhonemeTiming> = input
        .input
        .phonemes
        .iter()
        .map(|p| PhonemeTiming {
            phoneme: p.phoneme.clone(),
            position_ms: p.position_ms,
            duration_ms: p.duration_ms,
        })
        .collect();

    // Teacher render with the per-thread dump hook.
    let dump_path: PathBuf = dump_dir.join(format!("mel-{idx:05}.bin"));
    worldline_plugin::r2_engine::set_thread_dump_path(Some(dump_path.clone()));
    let result = engine.render_phrase(&input.input);
    worldline_plugin::r2_engine::set_thread_dump_path(None);
    let audio = result.map_err(|e| format!("render {:?}: {e}", spec.aliases))?;

    let (frames, f0_hz, mel_flat) = read_dump(&dump_path)?;
    let _ = std::fs::remove_file(&dump_path);
    let mel: Vec<Vec<f32>> = mel_flat.chunks_exact(128).map(|c| c.to_vec()).collect();

    Ok((
        TrainEntry {
            aliases: spec.aliases.clone(),
            tone: spec.tone,
            duration_ms: spec.duration_ms,
            phonemes,
            frames,
            f0_hz,
            mel,
        },
        audio,
    ))
}

/// Extract a whole bank: render every spec, filter garbage, write JSONL
/// lines, return (passed, filtered_out, skipped) counts. When
/// `wav_dir` is set, the teacher (R2) wav per phrase is also saved
/// (`{idx:05}.wav`) — the golden reference for student rendering.
///
/// Parallel: phrase renders are independent, so N workers each open
/// their OWN R2 engine (ort sessions are read-only) and pull specs off
/// a shared atomic counter; results stay index-ordered.
pub fn extract_bank(
    so: &Path,
    mel_path: &Path,
    vocoder_path: &Path,
    vb: &Voicebank,
    specs: &[PhraseSpec],
    out_path: &Path,
    dump_dir: &Path,
    wav_dir: Option<&Path>,
) -> Result<(usize, usize, usize), String> {
    std::fs::create_dir_all(dump_dir).map_err(|e| format!("dump dir: {e}"))?;
    if let Some(w) = wav_dir {
        std::fs::create_dir_all(w).map_err(|e| format!("wav dir: {e}"))?;
    }
    let n = specs.len();
    let workers = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .min(8);
    eprintln!("[extract] {n} specs, {workers} workers");
    let results: std::sync::Arc<
        std::sync::Mutex<Vec<Option<Result<(TrainEntry, Vec<f32>), String>>>>,
    > = std::sync::Arc::new(std::sync::Mutex::new(vec![None; n]));
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..workers {
            let results = results.clone();
            let next = &next;
            s.spawn(move || {
                let engine = match WorldlineR2Engine::open(so, mel_path, vocoder_path, false) {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("[extract] worker engine open failed: {e}");
                        return;
                    }
                };
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let r = extract_phrase(&engine, vb, &specs[i], dump_dir, i);
                    results.lock().unwrap()[i] = Some(r);
                }
            });
        }
    });

    let results = results.lock().unwrap();
    let mut skipped = 0usize;
    let mut pairs: Vec<(TrainEntry, Vec<f32>)> = Vec::with_capacity(n);
    for (i, slot) in results.iter().enumerate() {
        match slot {
            Some(Ok(pair)) => pairs.push(pair.clone()),
            Some(Err(e)) => {
                eprintln!("[skip] {:?}: {e}", specs[i].aliases);
                skipped += 1;
            }
            None => {
                eprintln!("[skip] {:?}: worker failed (no result)", specs[i].aliases);
                skipped += 1;
            }
        }
    }
    drop(results);
    let entries: Vec<TrainEntry> = pairs.iter().map(|(e, _)| e.clone()).collect();
    let (passed, stats) = crate::filter::apply(&entries);
    eprintln!(
        "[filter] total={} passed={} too_short={} unvoiced={} flat_mel={}",
        stats.total, stats.passed, stats.too_short, stats.unvoiced, stats.flat_mel
    );
    // Write teacher wavs + JSONL for PASSED entries, index = passed order.
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(out_path)
            .map_err(|e| format!("create {}: {e}", out_path.display()))?,
    );
    let mut passed_order: Vec<usize> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        if crate::filter::passes(e) {
            passed_order.push(i);
        }
    }
    let mut passed_entries: Vec<TrainEntry> = Vec::with_capacity(passed_order.len());
    for (k, &src) in passed_order.iter().enumerate() {
        let (entry, audio) = &pairs[src];
        passed_entries.push(entry.clone());
        if let Some(w) = wav_dir {
            let wav = wavwriter::write_wav_16(audio, 44100);
            let p = w.join(format!("{k:05}.wav"));
            wavwriter::write_wav_file(&p, &wav)
                .map_err(|e| format!("write {}: {e}", p.display()))?;
        }
        let line = serde_json::to_string(entry).map_err(|e| format!("json: {e}"))?;
        file.write_all(line.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .map_err(|e| format!("write: {e}"))?;
    }
    file.flush().map_err(|e| format!("flush: {e}"))?;
    // Compact binary sibling (same entries, same order).
    write_tsbin(&out_path.with_extension("tsbin"), &passed_entries)?;
    Ok((
        passed_order.len(),
        stats.total - passed_order.len(),
        skipped,
    ))
}
