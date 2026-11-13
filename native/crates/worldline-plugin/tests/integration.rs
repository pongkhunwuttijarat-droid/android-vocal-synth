//! Integration tests against the real `libworldline.so` (ignored by
//! default — they need the prebuilt library).
//!
//! ```sh
//! WORLDLINE_SO=.../native/build/build-linux/libworldline.so \
//!     cargo test -p worldline-plugin -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use feed::render_input::{
    Curves, OtoEntry, PhraseInfo, RenderInput, RenderNote, RenderPhoneme, SampleBased,
};

const DEFAULT_SO: &str =
    "/home/seal/project/android-voice-synth/native/build/build-linux/libworldline.so";
const SAMPLE_RATE: u32 = 44100;

fn so_path() -> PathBuf {
    if let Ok(path) = std::env::var("WORLDLINE_SO") {
        return PathBuf::from(path);
    }
    let path = PathBuf::from(DEFAULT_SO);
    assert!(
        path.exists(),
        "built libworldline.so not found at {path:?}; set WORLDLINE_SO"
    );
    path
}

fn mock_wav_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mock-voicebank/voice/_3_h3_3-.wav")
}

/// One "3 h3" phoneme at C4, 120 bpm, 300 ms, from the mock voicebank.
fn mock_input() -> RenderInput {
    let grid_len = 107; // (240 leading + 288 duration ticks) / 5 + 1
    RenderInput {
        phrase: PhraseInfo {
            position_ms: 500.0,
            duration_ms: 300.0,
            leading_ms: 250.0,
            leading_ticks: 240,
            tempo_map: vec![],
            time_axis_hint: Some("120bpm 4/4".into()),
        },
        notes: vec![RenderNote {
            lyric: "3".into(),
            tone: 60,
            position_ms: 500.0,
            duration_ms: 300.0,
        }],
        phonemes: vec![RenderPhoneme {
            phoneme: "3 h3".into(),
            position_ms: 500.0,
            duration_ms: 300.0,
            leading_ms: 250.0,
            overlap_ms: 83.333,
            tone: 60,
            adjusted_tempo: 120.0,
            consonant_velocity: 100.0,
            volume: 100.0,
            modulation: 0.0,
            tone_shift: 0,
            index: 0,
            parent_note: 0,
        }],
        pitches_cents: vec![6000; grid_len],
        pitches_cents_f32: vec![6000.0; grid_len],
        curve_grid_start_tick: 0,
        render_origin_ms: 250.0,
        curves: Curves {
            dynamics: vec![1.0; grid_len],
            gender: vec![0.0; grid_len],
            breathiness: vec![0.0; grid_len],
            tension: vec![0.0; grid_len],
            voicing: vec![100.0; grid_len],
            extra: Vec::new(),
        },
        sample_based: Some(SampleBased {
            oto: vec![OtoEntry {
                alias: "3 h3".into(),
                file: "_3_h3_3-.wav".into(),
                wav_path: mock_wav_path().to_string_lossy().into_owned(),
                offset: 442.3,
                consonant: 375.0,
                cutoff: -583.333,
                preutter: 250.0,
                overlap: 83.333,
                envelope: Vec::new(),
                flags: String::new(),
            }],
            wav_path: Some(mock_wav_path().to_string_lossy().into_owned()),
            envelope: Vec::new(),
            flags: String::new(),
        }),
        neural: None,
    }
}

/// A longer, controlled passage built from the real mock voicebank OTO/WAV.
/// Repeats the mapped syllable across 40 half-second notes so callback timing
/// is not dominated by a single short synthesis request.
fn long_mock_input() -> RenderInput {
    let mut input = mock_input();
    const NOTES: usize = 40;
    const NOTE_MS: f64 = 500.0;
    let first_note = input.notes[0].clone();
    let first_phoneme = input.phonemes[0].clone();
    let first_oto = input.sample_based.as_ref().unwrap().oto[0].clone();

    for index in 0..NOTES {
        let start_ms = first_note.position_ms + index as f64 * NOTE_MS;
        let tone = [60, 62, 64, 67, 65, 64, 62, 59][index % 8];
        let mut note = first_note.clone();
        note.position_ms = start_ms;
        note.duration_ms = NOTE_MS;
        note.tone = tone;
        input.notes.push(note);

        let mut phoneme = first_phoneme.clone();
        phoneme.position_ms = start_ms;
        phoneme.duration_ms = NOTE_MS;
        phoneme.tone = tone;
        phoneme.parent_note = index;
        input.phonemes.push(phoneme);
        input
            .sample_based
            .as_mut()
            .unwrap()
            .oto
            .push(first_oto.clone());
    }

    // Replace the initial seed note/phoneme/OTO with the generated first
    // note, rather than counting the seed twice.
    input.notes.remove(0);
    input.phonemes.remove(0);
    input.sample_based.as_mut().unwrap().oto.remove(0);
    input.phrase.duration_ms = NOTES as f64 * NOTE_MS;
    let grid_len = ((input.phrase.leading_ticks as f64 + input.phrase.duration_ms / 1000.0 * 960.0)
        / 5.0)
        .ceil() as usize
        + 1;
    input.pitches_cents = vec![6000; grid_len];
    input.pitches_cents_f32 = vec![6000.0; grid_len];
    input.curves.dynamics = vec![1.0; grid_len];
    input.curves.gender = vec![0.0; grid_len];
    input.curves.breathiness = vec![0.0; grid_len];
    input.curves.tension = vec![0.0; grid_len];
    input.curves.voicing = vec![100.0; grid_len];
    input
}

/// A controlled passage of `notes_count` 500 ms notes using real samples
/// from the complete Teto Japanese integrated voicebank unpacked in the workspace.
fn full_voicebank_input_with_notes(notes_count: usize) -> RenderInput {
    let mut input = mock_input();
    let bank_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../test-ustx-example/voicebank/extracted/重音テト OU用日本語統合ライブラリー");
    let bank = voicebank::load_voicebank(&bank_root)
        .unwrap_or_else(|e| panic!("load full Teto voicebank {}: {e}", bank_root.display()));
    const ALIASES: [&str; 5] = ["あ", "い", "う", "え", "お"];
    const NOTE_MS: f64 = 500.0;
    let first_note = input.notes[0].clone();
    let first_phoneme = input.phonemes[0].clone();
    let sample_based = input.sample_based.as_mut().unwrap();
    sample_based.oto.clear();

    input.notes.clear();
    input.phonemes.clear();
    for index in 0..notes_count {
        let alias = ALIASES[index % ALIASES.len()];
        let mapped = bank
            .oto_map
            .get(alias)
            .unwrap_or_else(|| panic!("full Teto voicebank has no alias {alias:?}"));
        let wav_path = bank.oto_sets[mapped.set_index].wav_path(&mapped.wav);
        let start_ms = 500.0 + index as f64 * NOTE_MS;
        let tone = [60, 62, 64, 67, 65, 64, 62, 59][index % 8];

        let mut note = first_note.clone();
        note.lyric = alias.to_string();
        note.position_ms = start_ms;
        note.duration_ms = NOTE_MS;
        note.tone = tone;
        input.notes.push(note);

        let mut phoneme = first_phoneme.clone();
        phoneme.phoneme = alias.to_string();
        phoneme.position_ms = start_ms;
        phoneme.duration_ms = NOTE_MS;
        phoneme.tone = tone;
        phoneme.parent_note = index;
        input.phonemes.push(phoneme);

        sample_based.oto.push(OtoEntry {
            alias: alias.to_string(),
            file: mapped.wav.clone(),
            wav_path: wav_path.to_string_lossy().into_owned(),
            offset: mapped.offset,
            consonant: mapped.consonant,
            cutoff: mapped.cutoff,
            preutter: mapped.preutter,
            overlap: mapped.overlap,
            envelope: Vec::new(),
            flags: String::new(),
        });
    }
    sample_based.wav_path = sample_based.oto.first().map(|oto| oto.wav_path.clone());
    input.phrase.duration_ms = notes_count as f64 * NOTE_MS;
    let grid_len = ((input.phrase.leading_ticks as f64 + input.phrase.duration_ms / 1000.0 * 960.0)
        / 5.0)
        .ceil() as usize
        + 1;
    input.pitches_cents = vec![6000; grid_len];
    input.pitches_cents_f32 = vec![6000.0; grid_len];
    input.curves.dynamics = vec![1.0; grid_len];
    input.curves.gender = vec![0.0; grid_len];
    input.curves.breathiness = vec![0.0; grid_len];
    input.curves.tension = vec![0.0; grid_len];
    input.curves.voicing = vec![100.0; grid_len];
    input
}

fn long_full_voicebank_input() -> RenderInput {
    full_voicebank_input_with_notes(40)
}

/// Render one phoneme from the mock voicebank through the real .so and
/// check that non-silent PCM comes out; the second render must also work
/// (a fresh PhraseSynth is spawned per phrase).
#[test]
#[ignore = "requires the built libworldline.so; run with WORLDLINE_SO=... cargo test -- --ignored --nocapture"]
fn renders_one_phoneme_to_pcm() {
    let path = so_path();
    println!("loading {path:?}");
    let renderer = worldline_plugin::WorldlineRenderer::open(&path).expect("open renderer");

    let samples = renderer
        .render_phrase(&mock_input())
        .expect("render phrase");
    println!(
        "samples: {} ({} ms), peak: {:.4}",
        samples.len(),
        samples.len() as f64 * 1000.0 / SAMPLE_RATE as f64,
        samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()))
    );
    assert!(!samples.is_empty(), "synth produced no samples");
    assert!(
        samples.len() > SAMPLE_RATE as usize / 10,
        "expected at least 100 ms of audio, got {} samples",
        samples.len()
    );
    let peak = samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()));
    assert!(peak > 1e-4, "output is silent (peak {peak})");

    // A second render must produce nearly identical output (fresh
    // PhraseSynth, same input). NOT bit-exact: the C++ WORLD/pyin DSP
    // reads some uninitialized heap memory (values depend on heap
    // layout), so output varies ~2% peak between runs/threads — the
    // same reason OpenUtau golden tests use tolerances, not ==.
    let again = renderer
        .render_phrase(&mock_input())
        .expect("second render");
    assert_eq!(again.len(), samples.len());
    let max_diff = again
        .iter()
        .zip(&samples)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let mean_diff = again
        .iter()
        .zip(&samples)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / again.len() as f32;
    let peak = samples.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()));
    println!("determinism: max_diff={max_diff:.2e} mean_diff={mean_diff:.2e} peak={peak:.4}");
    // Same input → same output within DSP noise (~2% of peak).
    assert!(
        max_diff < peak * 0.05,
        "two renders of the same input differ beyond DSP noise \
         (max_diff={max_diff:.2e}, peak={peak:.4})"
    );
}

/// The pure conversion half must agree with the .so-backed half on the
/// request fields the .so consumes.
#[test]
#[ignore = "requires the built libworldline.so; run with WORLDLINE_SO=... cargo test -- --ignored --nocapture"]
fn conversion_and_render_agree() {
    let input = mock_input();
    let requests = worldline_plugin::build_requests(&input).expect("build requests");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    println!(
        "request: tone={} con_vel={} offset={} required_length={} consonant={} cut_off={} volume={} bend_len={}",
        request.tone,
        request.con_vel,
        request.offset,
        request.required_length,
        request.consonant,
        request.cut_off,
        request.volume,
        request.pitch_bend.len()
    );
    // smoke.c invariant: length_ms must not exceed required_length.
    assert!(request.length_ms <= request.required_length);
    assert_eq!(request.tone, 60);
    assert_eq!(request.con_vel, 100.0);
    assert_eq!(request.volume, 100.0);
    assert_eq!(request.flag_P, 86);
    assert_eq!(request.flag_Mv, 100);
    assert!(!request.pitch_bend.is_empty());
    assert!(request.pitch_bend.iter().all(|&b| b == 0));

    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");
    let samples = renderer.render_phrase(&input).expect("render phrase");
    assert!(!samples.is_empty());
}

/// Compare the finalized-prefix POC against the legacy full-phrase renderer.
/// This records first-block latency and verifies the concatenated stream is
/// materially identical before the stream is connected to the project sink.
#[test]
#[ignore = "requires the newly built libworldline.so; set WORLDLINE_SO and pass --ignored"]
fn streamed_pcm_matches_whole_phrase_and_arrives_before_return() {
    use std::time::Instant;

    std::env::set_var("WORLDLINE_PROFILE_PREP", "1");
    let path = so_path();
    let renderer = worldline_plugin::WorldlineRenderer::open(&path).expect("open renderer");

    let baseline_start = Instant::now();
    let baseline = renderer
        .render_phrase(&mock_input())
        .expect("whole-phrase baseline");
    let baseline_elapsed = baseline_start.elapsed();

    let stream_start = Instant::now();
    let mut first_block_after = None;
    let mut blocks = 0usize;
    let mut streamed = Vec::new();
    let total = renderer
        .render_phrase_stream(&mock_input(), 1024, |block| {
            if first_block_after.is_none() {
                first_block_after = Some(stream_start.elapsed());
            }
            blocks += 1;
            streamed.extend_from_slice(block);
            Ok(())
        })
        .expect("streamed phrase");
    let stream_elapsed = stream_start.elapsed();

    assert_eq!(streamed.len(), total);
    assert_eq!(streamed.len(), baseline.len());
    assert!(blocks > 1, "expected multiple streamed blocks");
    let peak = baseline.iter().fold(0.0f32, |p, x| p.max(x.abs()));
    let max_error = baseline
        .iter()
        .zip(&streamed)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let dot = baseline
        .iter()
        .zip(&streamed)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum::<f64>();
    let norm_a = baseline.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
    let norm_b = streamed.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
    let correlation = dot / (norm_a * norm_b).sqrt().max(f64::MIN_POSITIVE);

    println!(
        "baseline_total_ms={} stream_first_block_ms={} stream_total_ms={} blocks={} frames={} max_error={:.6e} peak={:.6e} corr={:.8}",
        baseline_elapsed.as_secs_f64() * 1000.0,
        first_block_after.expect("at least one PCM callback").as_secs_f64() * 1000.0,
        stream_elapsed.as_secs_f64() * 1000.0,
        blocks,
        total,
        max_error,
        peak,
        correlation,
    );
    assert!(
        correlation > 0.99,
        "streamed PCM correlation {correlation} <= 0.99"
    );

    let mut rejected_blocks = 0;
    let error = renderer
        .render_phrase_stream(&mock_input(), 1024, |_| {
            rejected_blocks += 1;
            Err("stop stream probe".to_string())
        })
        .expect_err("callback failure must abort the native pulse loop");
    assert_eq!(rejected_blocks, 1);
    assert!(error.to_string().contains("stop stream probe"));
    std::env::remove_var("WORLDLINE_PROFILE_PREP");
}

/// Longer passage measurement, using 40 notes / 20 seconds of project time.
#[test]
#[ignore = "requires the newly built libworldline.so; set WORLDLINE_SO and pass --ignored"]
fn long_passage_streamed_pcm_matches_whole_phrase_and_arrives_before_return() {
    use std::time::Instant;

    std::env::set_var("WORLDLINE_PROFILE_PREP", "1");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");
    let input = long_mock_input();

    let baseline_start = Instant::now();
    let baseline = renderer
        .render_phrase(&input)
        .expect("whole-phrase baseline");
    let baseline_elapsed = baseline_start.elapsed();

    let stream_start = Instant::now();
    let mut first_block_after = None;
    let mut blocks = 0usize;
    let mut streamed = Vec::new();
    let total = renderer
        .render_phrase_stream(&input, 4096, |block| {
            first_block_after.get_or_insert_with(|| stream_start.elapsed());
            blocks += 1;
            streamed.extend_from_slice(block);
            Ok(())
        })
        .expect("streamed passage");
    let stream_elapsed = stream_start.elapsed();

    assert_eq!(streamed.len(), total);
    assert_eq!(streamed.len(), baseline.len());
    assert!(blocks > 1, "expected multiple streamed blocks");
    let peak = baseline.iter().fold(0.0f32, |p, x| p.max(x.abs()));
    let max_error = baseline
        .iter()
        .zip(&streamed)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let dot = baseline
        .iter()
        .zip(&streamed)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum::<f64>();
    let norm_a = baseline.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
    let norm_b = streamed.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
    let correlation = dot / (norm_a * norm_b).sqrt().max(f64::MIN_POSITIVE);
    let audio_ms = total as f64 * 1000.0 / SAMPLE_RATE as f64;
    println!(
        "long passage: notes={} audio_ms={audio_ms:.1} baseline_total_ms={:.2} stream_first_block_ms={:.2} stream_total_ms={:.2} blocks={blocks} frames={total} max_error={max_error:.6e} peak={peak:.6e} corr={correlation:.8}",
        input.notes.len(),
        baseline_elapsed.as_secs_f64() * 1000.0,
        first_block_after.expect("at least one PCM callback").as_secs_f64() * 1000.0,
        stream_elapsed.as_secs_f64() * 1000.0,
    );
    assert_eq!(max_error, 0.0, "stream differs from whole-phrase PCM");
    assert!(
        correlation > 0.99,
        "streamed PCM correlation {correlation} <= 0.99"
    );
    std::env::remove_var("WORLDLINE_PROFILE_PREP");
}

/// Repeat the 20-second stream parity measurement with full Teto bank samples.
#[test]
#[ignore = "requires the newly built libworldline.so and extracted full Teto bank; pass --ignored"]
fn full_voicebank_passage_streamed_pcm_matches_whole_phrase() {
    use std::time::Instant;

    std::env::set_var("WORLDLINE_PROFILE_PREP", "1");
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");
    let input = long_full_voicebank_input();

    std::env::set_var("WORLDLINE_PREP_THREADS", "1");
    let baseline_start = Instant::now();
    let baseline = renderer
        .render_phrase(&input)
        .expect("single-thread whole-phrase baseline");
    let baseline_elapsed = baseline_start.elapsed();

    std::env::set_var("WORLDLINE_PREP_THREADS", "4");
    let parallel_start = Instant::now();
    let parallel = renderer
        .render_phrase(&input)
        .expect("four-thread whole-phrase render");
    let parallel_elapsed = parallel_start.elapsed();

    let stream_start = Instant::now();
    let mut first_block_after = None;
    let mut blocks = 0usize;
    let mut streamed = Vec::new();
    let total = renderer
        .render_phrase_stream(&input, 4096, |block| {
            first_block_after.get_or_insert_with(|| stream_start.elapsed());
            blocks += 1;
            streamed.extend_from_slice(block);
            Ok(())
        })
        .expect("streamed full voicebank passage");
    let stream_elapsed = stream_start.elapsed();

    assert_eq!(streamed.len(), total);
    assert_eq!(streamed.len(), baseline.len());
    assert_eq!(parallel.len(), baseline.len());
    assert!(blocks > 1, "expected multiple streamed blocks");
    let parallel_max_error = baseline
        .iter()
        .zip(&parallel)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let max_error = baseline
        .iter()
        .zip(&streamed)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let dot = baseline
        .iter()
        .zip(&streamed)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum::<f64>();
    let norm_a = baseline.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
    let norm_b = streamed.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
    let correlation = dot / (norm_a * norm_b).sqrt().max(f64::MIN_POSITIVE);
    let audio_ms = total as f64 * 1000.0 / SAMPLE_RATE as f64;
    println!(
        "full Teto bank parallel prep: aliases=あいうえお notes={} audio_ms={audio_ms:.1} sequential_total_ms={:.2} parallel_4_total_ms={:.2} stream_first_block_ms={:.2} stream_total_ms={:.2} blocks={blocks} frames={total} sequential_vs_stream_error={max_error:.6e} sequential_vs_parallel_error={parallel_max_error:.6e} corr={correlation:.8}",
        input.notes.len(),
        baseline_elapsed.as_secs_f64() * 1000.0,
        parallel_elapsed.as_secs_f64() * 1000.0,
        first_block_after.expect("at least one PCM callback").as_secs_f64() * 1000.0,
        stream_elapsed.as_secs_f64() * 1000.0,
    );
    assert_eq!(max_error, 0.0, "stream differs from whole-phrase PCM");
    assert_eq!(
        parallel_max_error, 0.0,
        "parallel PCM differs from sequential PCM"
    );
    assert!(
        correlation > 0.99,
        "streamed PCM correlation {correlation} <= 0.99"
    );
    std::env::remove_var("WORLDLINE_PREP_THREADS");
    std::env::remove_var("WORLDLINE_PROFILE_PREP");
}

static STREAM_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn pcm_correlation(a: &[f32], b: &[f32]) -> f64 {
    let dot = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum::<f64>();
    let na = a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
    let nb = b.iter().map(|y| f64::from(*y).powi(2)).sum::<f64>();
    dot / (na * nb).sqrt().max(f64::MIN_POSITIVE)
}

fn pcm_max_error(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max)
}

#[test]
#[ignore = "requires libworldline.so and full Teto bank; pass --ignored"]
fn t050_stream_workers_backpressure_cache_and_cancel() {
    let _env_guard = STREAM_ENV_LOCK.lock().unwrap();
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");
    let input = full_voicebank_input_with_notes(40);

    renderer.clear_caches();
    let offline = renderer.render_phrase(&input).expect("offline baseline");

    // 1 prep worker + bounded max_in_flight=3 (cold cache)
    renderer.clear_caches();
    std::env::set_var("WORLDLINE_STREAM_PREP_THREADS", "1");
    std::env::set_var("WORLDLINE_STREAM_MAX_INFLIGHT", "3");
    let mut cold1_pcm = Vec::new();
    let (total1, stats1) = renderer
        .render_phrase_stream_with_stats(&input, 4096, |block| {
            cold1_pcm.extend_from_slice(block);
            Ok(())
        })
        .expect("cold 1-prep stream");
    assert_eq!(total1, offline.len());
    assert_eq!(stats1.synth_workers, 1);
    assert_eq!(stats1.prep_workers, 1);
    assert!(
        stats1.peak_buffered_models <= 3,
        "peak buffered models {} exceeded bound 3",
        stats1.peak_buffered_models
    );
    assert_eq!(stats1.cache_hits + stats1.cache_misses, 40);
    assert!(
        stats1.cache_misses >= 5,
        "expected >= 5 cold misses for 5 distinct aliases, got {}",
        stats1.cache_misses
    );
    assert_eq!(pcm_max_error(&offline, &cold1_pcm), 0.0);

    // 2 prep workers + default/bounded max_in_flight (cold cache)
    renderer.clear_caches();
    std::env::set_var("WORLDLINE_STREAM_PREP_THREADS", "2");
    std::env::set_var("WORLDLINE_STREAM_MAX_INFLIGHT", "4");
    let mut cold2_pcm = Vec::new();
    let (total2, stats2) = renderer
        .render_phrase_stream_with_stats(&input, 4096, |block| {
            cold2_pcm.extend_from_slice(block);
            Ok(())
        })
        .expect("cold 2-prep stream");
    assert_eq!(total2, offline.len());
    assert_eq!(stats2.synth_workers, 1);
    assert_eq!(stats2.prep_workers, 2);
    assert!(
        stats2.peak_buffered_models <= 4,
        "peak buffered models {} exceeded bound 4",
        stats2.peak_buffered_models
    );
    assert_eq!(stats2.cache_hits + stats2.cache_misses, 40);
    assert!(stats2.cache_misses >= 5);
    assert_eq!(pcm_max_error(&offline, &cold2_pcm), 0.0);

    // Warmed cache run immediately after: all cache hits, < 500 ms TTFS
    let mut warm_pcm = Vec::new();
    let (total_warm, stats_warm) = renderer
        .render_phrase_stream_with_stats(&input, 4096, |block| {
            warm_pcm.extend_from_slice(block);
            Ok(())
        })
        .expect("warm stream");
    assert_eq!(total_warm, offline.len());
    assert_eq!(stats_warm.cache_hits, 40);
    assert_eq!(stats_warm.cache_misses, 0);
    assert!(
        stats_warm.first_block_ms < 500.0,
        "warm first_block_ms {:.2} >= 500 ms",
        stats_warm.first_block_ms
    );
    assert_eq!(pcm_max_error(&offline, &warm_pcm), 0.0);

    // Mid-stream cooperative cancellation on cold cache
    renderer.clear_caches();
    let mut cb_count = 0usize;
    let cancel_err = renderer
        .render_phrase_stream_with_stats(&input, 4096, |_| {
            cb_count += 1;
            if cb_count >= 1 {
                return Err("user pressed stop".to_string());
            }
            Ok(())
        })
        .expect_err("expected cooperative cancel error");
    assert!(format!("{cancel_err}").contains("stop"), "{cancel_err}");
    assert_eq!(cb_count, 1);

    std::env::remove_var("WORLDLINE_STREAM_PREP_THREADS");
    std::env::remove_var("WORLDLINE_STREAM_MAX_INFLIGHT");

    println!(
        "T050 C++ 20s 40-note stats: \
         cold_1p(ttfs={:.2}ms total={:.2}ms prep_wait={:.2}ms bp_wait={:.2}ms peak_models={} peak_frames={}) \
         cold_2p(ttfs={:.2}ms total={:.2}ms prep_wait={:.2}ms bp_wait={:.2}ms peak_models={} peak_frames={}) \
         warm_2p(ttfs={:.2}ms total={:.2}ms hits={} peak_models={} peak_frames={}) max_err={:.3e} corr={:.8}",
        stats1.first_block_ms,
        stats1.total_ms,
        stats1.prep_wait_ms,
        stats1.backpressure_wait_ms,
        stats1.peak_buffered_models,
        stats1.peak_timeline_frames,
        stats2.first_block_ms,
        stats2.total_ms,
        stats2.prep_wait_ms,
        stats2.backpressure_wait_ms,
        stats2.peak_buffered_models,
        stats2.peak_timeline_frames,
        stats_warm.first_block_ms,
        stats_warm.total_ms,
        stats_warm.cache_hits,
        stats_warm.peak_buffered_models,
        stats_warm.peak_timeline_frames,
        pcm_max_error(&offline, &cold2_pcm),
        pcm_correlation(&offline, &cold2_pcm),
    );
}

#[test]
#[ignore = "requires libworldline.so and full Teto bank; pass --ignored"]
fn t050_stream_out_of_order_prep_matches_offline() {
    let _env_guard = STREAM_ENV_LOCK.lock().unwrap();
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");
    let mut input = full_voicebank_input_with_notes(8);
    // Stretch note 0 heavily so prep worker 1 finishes short notes 1..7 first.
    input.notes[0].duration_ms = 2500.0;
    input.phonemes[0].duration_ms = 2500.0;
    let mut cursor = 500.0 + 2500.0;
    for i in 1..input.notes.len() {
        input.notes[i].position_ms = cursor;
        input.notes[i].duration_ms = 180.0;
        input.phonemes[i].position_ms = cursor;
        input.phonemes[i].duration_ms = 180.0;
        cursor += 180.0;
    }
    input.phrase.duration_ms = cursor - 500.0;
    let grid_len = ((input.phrase.leading_ticks as f64 + input.phrase.duration_ms / 1000.0 * 960.0)
        / 5.0)
        .ceil() as usize
        + 1;
    input.pitches_cents = vec![6000; grid_len];
    input.pitches_cents_f32 = vec![6000.0; grid_len];
    input.curves.dynamics = vec![1.0; grid_len];
    input.curves.gender = vec![0.0; grid_len];
    input.curves.breathiness = vec![0.0; grid_len];
    input.curves.tension = vec![0.0; grid_len];
    input.curves.voicing = vec![100.0; grid_len];

    renderer.clear_caches();
    let offline = renderer.render_phrase(&input).expect("offline");

    renderer.clear_caches();
    std::env::set_var("WORLDLINE_STREAM_PREP_THREADS", "2");
    let mut streamed = Vec::new();
    let (total, stats) = renderer
        .render_phrase_stream_with_stats(&input, 2048, |block| {
            streamed.extend_from_slice(block);
            Ok(())
        })
        .expect("out-of-order stream");
    std::env::remove_var("WORLDLINE_STREAM_PREP_THREADS");

    assert_eq!(total, offline.len());
    let max_err = pcm_max_error(&offline, &streamed);
    println!(
        "T050 out-of-order prep: ttfs={:.2}ms total={:.2}ms prep_workers={} peak_models={} max_err={max_err:.3e}",
        stats.first_block_ms, stats.total_ms, stats.prep_workers, stats.peak_buffered_models
    );
    assert_eq!(max_err, 0.0);
}

#[test]
#[ignore = "requires libworldline.so and full Teto bank; pass --ignored"]
fn t050_stream_60s_stress_and_localized_warm_edit_under_500ms() {
    let _env_guard = STREAM_ENV_LOCK.lock().unwrap();
    let renderer = worldline_plugin::WorldlineRenderer::open(so_path()).expect("open renderer");
    let input = full_voicebank_input_with_notes(120);

    renderer.clear_caches();
    let offline = renderer.render_phrase(&input).expect("60s offline");

    renderer.clear_caches();
    std::env::set_var("WORLDLINE_STREAM_PREP_THREADS", "2");
    let mut cold_pcm = Vec::new();
    let (cold_total, cold_stats) = renderer
        .render_phrase_stream_with_stats(&input, 4096, |block| {
            cold_pcm.extend_from_slice(block);
            Ok(())
        })
        .expect("60s cold stream");
    assert_eq!(cold_total, offline.len());
    let cold_max_err = pcm_max_error(&offline, &cold_pcm);
    let cold_corr = pcm_correlation(&offline, &cold_pcm);
    assert_eq!(cold_max_err, 0.0);
    assert!(cold_corr > 0.999999);

    // Cache is now warmed for all 120 notes; apply a localized 1-note edit
    // (changing velocity + pitch so note 0 misses PreparedCache while notes 1..119 hit).
    let mut edited = input.clone();
    edited.notes[0].tone += 3;
    edited.phonemes[0].tone += 3;
    edited.phonemes[0].consonant_velocity = 145.0;
    for p in edited.pitches_cents.iter_mut().take(120) {
        *p += 300;
    }
    for p in edited.pitches_cents_f32.iter_mut().take(120) {
        *p += 300.0;
    }

    let mut edit_pcm = Vec::new();
    let (edit_total, edit_stats) = renderer
        .render_phrase_stream_with_stats(&edited, 4096, |block| {
            edit_pcm.extend_from_slice(block);
            Ok(())
        })
        .expect("60s localized warm edit stream");
    std::env::remove_var("WORLDLINE_STREAM_PREP_THREADS");

    let verifier = worldline_plugin::WorldlineRenderer::open(so_path()).expect("verifier");
    let edit_offline = verifier.render_phrase(&edited).expect("60s edit offline");
    assert_eq!(edit_total, edit_offline.len());
    let edit_max_err = pcm_max_error(&edit_offline, &edit_pcm);

    println!(
        "T050 60s (120 notes) stress: audio_ms={:.1} \
         cold_2p(ttfs={:.2}ms total={:.2}ms prep_wait={:.2}ms bp_wait={:.2}ms workers=1synth+{}prep peak_models={} peak_frames={} misses={} max_err={cold_max_err:.3e} corr={cold_corr:.8}) \
         warm_1note_edit(ttfs={:.2}ms total={:.2}ms hits={} misses={} max_err={edit_max_err:.3e})",
        cold_total as f64 * 1000.0 / SAMPLE_RATE as f64,
        cold_stats.first_block_ms,
        cold_stats.total_ms,
        cold_stats.prep_wait_ms,
        cold_stats.backpressure_wait_ms,
        cold_stats.prep_workers,
        cold_stats.peak_buffered_models,
        cold_stats.peak_timeline_frames,
        cold_stats.cache_misses,
        edit_stats.first_block_ms,
        edit_stats.total_ms,
        edit_stats.cache_hits,
        edit_stats.cache_misses,
    );
    assert_eq!(edit_max_err, 0.0);
    assert!(
        edit_stats.first_block_ms < 500.0,
        "warmed 60s localized-edit TTFS {:.2}ms >= 500 ms",
        edit_stats.first_block_ms
    );
}
