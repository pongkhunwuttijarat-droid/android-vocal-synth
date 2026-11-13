//! The browser code path, exercised natively.
//!
//! `Engine::render_project` is the exact function the JS host calls; the
//! only wasm-specific part is the wasm-bindgen glue. This test drives the
//! same in-memory flow (oto.ini bytes + samples in a `MemorySource`, with
//! the engine itself never reading the filesystem) against the real Teto
//! bank, and checks the result against the disk-backed render.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use engine_wasm::{Engine, MemoryRenderer};
use synth_cli::pipeline::PhraseRenderer;
use worldline_rs::SampleSource;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn teto_library() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

fn demo_song() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/synth-cli/tests/data/demo-song.ustx")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Install the Teto bank into `engine` from the real library dir on disk.
/// The engine sees only bytes — the host-side reading is what the browser
/// does with OPFS.
fn install_teto(engine: &mut Engine, library: &Path) {
    // The bank's oto.ini lives in `voice/`, alongside the wavs — so the
    // logical dir is `voice` and the keys come out as `voice/<name>.wav`.
    engine.set_voicebank(
        "teto-english",
        "Teto",
        "voice",
        &read(&library.join("voice/oto.ini")),
    );
    let mut with_frq = 0usize;
    let mut total = 0usize;
    for key in engine.wav_keys().lines() {
        let path = library.join(key);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        // UTAU banks ship `<stem>_wav.frq` next to each wav; the reference
        // renderer always prefers it over F0 estimation.
        let frq_path = voicebank::frq_path_for_wav(&path);
        total += 1;
        match std::fs::read(&frq_path) {
            Ok(frq) => {
                if engine
                    .add_sample_with_frq(key, &bytes, &frq)
                    .expect("add_sample_with_frq")
                {
                    with_frq += 1;
                }
            }
            Err(_) => engine.add_sample(key, &bytes).expect("add_sample"),
        }
    }
    assert!(total > 100, "voicebank exposed only {total} wav keys");
    assert_eq!(with_frq, total, "not every sample got its .frq attached");
}

/// The in-memory (`MemorySource`) render must match the disk render.
#[test]
fn memory_source_render_matches_disk_render() {
    let library = teto_library();
    if !library.is_dir() {
        eprintln!("skip: {} not present", library.display());
        return;
    }

    let mut engine = Engine::new();
    install_teto(&mut engine, &library);
    assert!(engine.alias_count() > 100, "otos: {}", engine.alias_count());
    assert!(
        engine.sample_count() > 100,
        "samples: {}",
        engine.sample_count()
    );

    let project_yaml = String::from_utf8(read(&demo_song())).expect("utf8 project");
    let mem_pcm = engine
        .render_project(&project_yaml, 0)
        .expect("in-memory render");

    // ---- reference: the disk-backed pipeline ----
    let project = synth_cli::pipeline::load_project(&demo_song()).expect("load project");
    let voicebank = synth_cli::pipeline::load_voicebank(&library).expect("load voicebank");
    let track = project.tracks.first().expect("track");
    let kind = synth_cli::pipeline::PhonemizerKind::from_track(track);
    let renderer = synth_cli::engine::WorldlineRsEngine::open(false).expect("open r1-rs");
    let mut cache = None;
    let cancel = AtomicBool::new(false);
    let disk_report = synth_cli::pipeline::render_project(
        &project,
        &voicebank,
        &renderer,
        0,
        kind,
        false,
        &mut cache,
        None,
        Some(&cancel),
    )
    .expect("disk render");

    assert_eq!(
        mem_pcm.len(),
        disk_report.samples.len(),
        "in-memory and disk renders differ in length"
    );
    assert!(peak(&mem_pcm) > 1e-4, "in-memory render is silent");
    let mut max_diff = 0.0f32;
    for (a, b) in mem_pcm.iter().zip(&disk_report.samples) {
        max_diff = max_diff.max((a - b).abs());
    }
    println!(
        "in-memory vs disk: {} samples, peak {:.4}, max|diff| {:.3e}",
        mem_pcm.len(),
        peak(&mem_pcm),
        max_diff
    );
    assert!(
        max_diff < 1e-6,
        "memory and disk renders diverged: {max_diff:.3e}"
    );
}

/// `render_project_wav` returns a decodable WAV of the same audio.
#[test]
fn wav_output_round_trips() {
    let library = teto_library();
    if !library.is_dir() {
        eprintln!("skip: {} not present", library.display());
        return;
    }
    let mut engine = Engine::new();
    install_teto(&mut engine, &library);
    let project_yaml = String::from_utf8(read(&demo_song())).expect("utf8 project");
    let wav = engine
        .render_project_wav(&project_yaml, 0)
        .expect("wav render");
    let parsed = voicebank::parse_wav(&wav).expect("parse rendered wav");
    assert_eq!(parsed.sample_rate, 44100);
    assert!(parsed.samples.len() > 44100, "expected >1 s of audio");
    assert!(peak(&parsed.samples) > 1e-4, "wav render is silent");
}

/// The renderer the wasm engine uses must work with samples handed to it
/// directly (the `MemorySource` contract), with no filesystem access.
#[test]
fn memory_renderer_renders_a_phrase_from_loaded_samples() {
    let library = teto_library();
    if !library.is_dir() {
        eprintln!("skip: {} not present", library.display());
        return;
    }
    let voicebank = synth_cli::pipeline::load_voicebank(&library).expect("load voicebank");
    let project = synth_cli::pipeline::load_project(&demo_song()).expect("load project");
    let track = project.tracks.first().expect("track");
    let part = project
        .parts
        .iter()
        .find_map(|p| match p {
            domain::UPart::Voice(v) if v.track_no == 0 => Some(v),
            _ => None,
        })
        .expect("voice part");
    let phrases = synth_cli::pipeline::build_phrase_inputs(
        &project,
        track,
        part,
        &voicebank,
        synth_cli::pipeline::PhonemizerKind::Universal,
    )
    .expect("build phrases");
    assert_eq!(phrases.len(), 1);

    // Load exactly the samples this phrase references (host-side read).
    let mut source = worldline_rs::MemorySource::new();
    if let Some(sb) = phrases[0].input.sample_based.as_ref() {
        for oto in &sb.oto {
            if source.contains(&oto.wav_path) {
                continue;
            }
            let bytes = std::fs::read(&oto.wav_path).expect("read sample");
            source
                .insert_wav_bytes(&oto.wav_path, &bytes)
                .expect("insert sample");
        }
    }
    assert!(!source.is_empty(), "phrase referenced no samples");

    let renderer = MemoryRenderer::new(source);
    let out = renderer
        .render_phrase(&phrases[0].input)
        .expect("memory render phrase");
    assert!(!out.is_empty(), "no samples rendered");
    assert!(peak(&out) > 1e-4, "rendered phrase is silent");
}
