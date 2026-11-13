use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use domain::{UMixFx, UNote, UPart, UProject, UTrack, UVoicePart, UWavePart};
use feed::render_input::RenderInput;
use media::{MediaSource, MemoryMediaSource};
use runtime::mixer::SAMPLE_RATE;
use synth_cli::pipeline::{self, PhraseRenderer};
use wavwriter::write_wav_16;

struct SilentRenderer;

impl PhraseRenderer for SilentRenderer {
    fn render_phrase(&self, _input: &RenderInput) -> Result<Vec<f32>, String> {
        Ok(Vec::new())
    }
}

struct CountingRenderer {
    calls: AtomicUsize,
}

impl PhraseRenderer for CountingRenderer {
    fn render_phrase(&self, _input: &RenderInput) -> Result<Vec<f32>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![0.125; SAMPLE_RATE as usize * 4])
    }

    fn render_phrase_stream(
        &self,
        _input: &RenderInput,
        block_samples: usize,
        callback: &mut dyn FnMut(&[f32]) -> Result<(), String>,
    ) -> Result<usize, String> {
        let samples = vec![0.125; SAMPLE_RATE as usize * 4];
        for block in samples.chunks(block_samples) {
            callback(block)?;
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(samples.len())
    }
}

fn tone(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|frame| {
            (0.35
                * (2.0 * std::f64::consts::PI * 330.0 * frame as f64 / f64::from(SAMPLE_RATE))
                    .sin()) as f32
        })
        .collect()
}

fn wave_project() -> UProject {
    let mut project = UProject::create();
    project.tracks = vec![
        UTrack {
            track_name: "voice".into(),
            ..Default::default()
        },
        UTrack {
            track_name: "audio".into(),
            ..Default::default()
        },
    ];
    project.wave_parts = Some(vec![UWavePart {
        name: "backing.wav".into(),
        track_no: 1,
        position: 0,
        relative_path: Some("asset:backing".into()),
        file_duration_ms: 1000.0,
        ..Default::default()
    }]);
    let yaml = serde_yaml::to_string(&project).expect("serialize fixture");
    let mut project = UProject::from_yaml_text(&yaml).expect("reload fixture");
    project.after_load().expect("initialize time axis");
    project
}

fn voice_and_wave_project() -> UProject {
    let mut project = wave_project();
    let mut voice = UVoicePart::new("voice part");
    // 40 seconds at the project tempo, so the 30-second chunk planner must
    // produce more than one independently renderable chunk.
    voice.duration = 480 * 80;
    voice.notes = (0..80)
        .map(|index| UNote {
            position: index * 480,
            duration: 480,
            tone: 60,
            lyric: "a [a]".into(),
            ..Default::default()
        })
        .collect();
    project.parts.insert(0, UPart::Voice(voice));
    project
}

fn empty_voicebank() -> voicebank::Voicebank {
    voicebank::Voicebank::from_oto_bytes("empty", "empty", "voice", "voice/oto.ini", b"")
}

#[test]
fn streamed_wave_mixdown_matches_offline_pcm_and_emits_contiguous_blocks() {
    let source = tone(SAMPLE_RATE as usize);
    let media = MemoryMediaSource::new();
    media.insert("asset:backing", write_wav_16(&source, SAMPLE_RATE));
    let mut project = wave_project();
    project.tracks[1].mix_fx = Some(UMixFx {
        enabled: true,
        reverb_wet: 0.2,
        ..Default::default()
    });
    let wave = project
        .parts
        .iter_mut()
        .find_map(|part| match part {
            UPart::Wave(wave) => Some(wave),
            _ => None,
        })
        .expect("wave part");
    wave.skip = 5;
    wave.trim = 7;
    wave.fadein = 11;
    wave.fadeout = 13;
    let bank = empty_voicebank();

    let mut offline_cache = None;
    let offline = pipeline::render_mixdown_with_pronunciations(
        &project,
        &bank,
        &SilentRenderer,
        None,
        false,
        &mut offline_cache,
        &media,
        None,
        None,
        None,
        None,
    )
    .expect("offline reference");

    let mut streamed_cache = None;
    let mut streamed = Vec::new();
    let mut starts = Vec::new();
    let mut lengths = Vec::new();
    let report = pipeline::render_mixdown_stream_with_pronunciations(
        &project,
        &bank,
        &SilentRenderer,
        None,
        &mut streamed_cache,
        &media,
        None::<&AtomicBool>,
        None,
        None,
        257,
        |start_frame, samples| {
            starts.push(start_frame);
            lengths.push(samples.len() / 2);
            streamed.extend_from_slice(samples);
            Ok(())
        },
    )
    .expect("stream reference");

    assert_eq!(report.channels, 2);
    assert_eq!(report.sample_rate, SAMPLE_RATE);
    assert_eq!(report.frames as usize * 2, offline.samples.len());
    assert_eq!(report.tracks[1].wave_parts_mixed, 1);
    assert_eq!(streamed, offline.samples);
    assert!(
        starts.len() > 1,
        "the stream should contain multiple blocks"
    );
    let mut expected = 0;
    for (start, length) in starts.iter().zip(lengths) {
        assert_eq!(*start, expected, "block starts must be contiguous");
        expected += length as u64;
    }
}

#[test]
fn final_mix_streams_before_all_voice_chunks_finish_and_matches_offline_pcm() {
    let source = tone(SAMPLE_RATE as usize * 10);
    let media = MemoryMediaSource::new();
    media.insert("asset:backing", write_wav_16(&source, SAMPLE_RATE));
    let project = voice_and_wave_project();
    let bank = voicebank::Voicebank::from_oto_bytes(
        "fixture",
        "fixture",
        "voice",
        "voice/oto.ini",
        b"a.wav=a,0,100,0,0,0\n",
    );

    let mut offline_cache = None;
    let offline = pipeline::render_mixdown_with_pronunciations(
        &project,
        &bank,
        &CountingRenderer {
            calls: AtomicUsize::new(0),
        },
        None,
        false,
        &mut offline_cache,
        &media,
        None,
        None,
        None,
        None,
    )
    .expect("offline reference");

    let renderer = CountingRenderer {
        calls: AtomicUsize::new(0),
    };
    let mut streamed_cache = None;
    let mut streamed = Vec::new();
    let mut calls_at_first_block = None;
    let report = pipeline::render_mixdown_stream_with_pronunciations(
        &project,
        &bank,
        &renderer,
        None,
        &mut streamed_cache,
        &media,
        None::<&AtomicBool>,
        None,
        None,
        1024,
        |_, samples| {
            calls_at_first_block.get_or_insert_with(|| renderer.calls.load(Ordering::SeqCst));
            streamed.extend_from_slice(samples);
            Ok(())
        },
    )
    .expect("stream reference");

    assert!(
        renderer.calls.load(Ordering::SeqCst) > 1,
        "test needs multiple chunks"
    );
    assert!(
        calls_at_first_block.expect("first block") < renderer.calls.load(Ordering::SeqCst),
        "first final-mix PCM must be published before all voice chunks finish"
    );
    assert_eq!(
        calls_at_first_block,
        Some(0),
        "the first mix block should be delivered from inside the first phrase callback"
    );
    assert_eq!(report.frames as usize * 2, offline.samples.len());
    assert_eq!(streamed, offline.samples);
}

#[test]
fn streamed_mixdown_reports_missing_wave_assets_before_claiming_audio() {
    let media = MemoryMediaSource::new();
    let project = wave_project();
    let bank = empty_voicebank();
    let mut cache = None;
    let mut blocks = 0usize;
    let error = pipeline::render_mixdown_stream_with_pronunciations(
        &project,
        &bank,
        &SilentRenderer,
        None,
        &mut cache,
        &media,
        None::<&AtomicBool>,
        None,
        None,
        512,
        |_, _| {
            blocks += 1;
            Ok(())
        },
    )
    .expect_err("missing media must prevent a successful stream result");
    assert!(error.contains("backing.wav"), "{error}");
    assert_eq!(
        blocks, 0,
        "no audio is published for a project with no source"
    );
}

#[test]
fn streamed_mixdown_fails_on_undecodable_wave_instead_of_silently_skipping_it() {
    let media = MemoryMediaSource::new();
    media.insert("asset:backing", b"not an audio container".to_vec());
    let project = wave_project();
    let bank = empty_voicebank();
    let mut cache = None;
    let mut blocks = 0usize;
    let error = pipeline::render_mixdown_stream_with_pronunciations(
        &project,
        &bank,
        &SilentRenderer,
        None,
        &mut cache,
        &media,
        None::<&AtomicBool>,
        None,
        None,
        512,
        |_, _| {
            blocks += 1;
            Ok(())
        },
    )
    .expect_err("invalid wave data must fail the stream");
    assert!(error.contains("wave part 'backing.wav'"), "{error}");
    assert_eq!(
        blocks, 0,
        "no PCM may be published for a corrupt wave-only project"
    );
}

#[test]
fn cancellation_during_pcm_delivery_stops_later_blocks() {
    let source = tone(SAMPLE_RATE as usize * 10);
    let media = MemoryMediaSource::new();
    media.insert("asset:backing", write_wav_16(&source, SAMPLE_RATE));
    let project = voice_and_wave_project();
    let bank = voicebank::Voicebank::from_oto_bytes(
        "fixture",
        "fixture",
        "voice",
        "voice/oto.ini",
        b"a.wav=a,0,100,0,0,0\n",
    );
    let cancel = AtomicBool::new(false);
    let mut cache = None;
    let mut blocks = 0usize;

    let error = pipeline::render_mixdown_stream_with_pronunciations(
        &project,
        &bank,
        &CountingRenderer {
            calls: AtomicUsize::new(0),
        },
        None,
        &mut cache,
        &media,
        Some(&cancel),
        None,
        None,
        512,
        |_, _| {
            blocks += 1;
            cancel.store(true, Ordering::Relaxed);
            Ok(())
        },
    )
    .expect_err("cancel after the first delivered block");

    assert!(error.contains("cancelled"), "{error}");
    assert_eq!(blocks, 1, "no PCM block may follow cancellation");
}

#[test]
#[ignore = "decodes and mixes the 288 s external MP3 fixture twice"]
fn real_mp3_wave_stream_matches_offline_and_emits_before_full_decode() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../");
    let source_path = repo.join("test-ustx-example/golden-the-end-of-hutsune-miku.ustx");
    let yaml = std::fs::read_to_string(&source_path).expect("fixture USTX");
    let reference = yaml
        .lines()
        .find_map(|line| line.trim().strip_prefix("relative_path:"))
        .expect("fixture has a wave source")
        .trim();
    let source = media::FileMediaSource::new(source_path.parent().expect("fixture directory"));
    let mp3 = source.load(reference).expect("fixture off-vocal MP3");
    let info = media::probe(&mp3).expect("probe fixture MP3");
    let media = MemoryMediaSource::new();
    media.insert("asset:backing", mp3);
    let mut project = wave_project();
    let wave = project
        .parts
        .iter_mut()
        .find_map(|part| match part {
            UPart::Wave(wave) => Some(wave),
            _ => None,
        })
        .expect("loaded wave part");
    wave.file_duration_ms = info.duration_ms;

    let mut offline_cache = None;
    let offline = pipeline::render_mixdown_with_pronunciations(
        &project,
        &empty_voicebank(),
        &SilentRenderer,
        None,
        false,
        &mut offline_cache,
        &media,
        None,
        None,
        None,
        None,
    )
    .expect("offline MP3 reference");

    let started = std::time::Instant::now();
    let mut first_block_ms = None;
    let mut streamed = Vec::new();
    let mut stream_cache = None;
    let report = pipeline::render_mixdown_stream_with_pronunciations(
        &project,
        &empty_voicebank(),
        &SilentRenderer,
        None,
        &mut stream_cache,
        &media,
        None,
        None,
        None,
        2048,
        |_, block| {
            first_block_ms.get_or_insert_with(|| started.elapsed().as_secs_f64() * 1000.0);
            streamed.extend_from_slice(block);
            Ok(())
        },
    )
    .expect("stream MP3 reference");
    assert_eq!(
        streamed, offline.samples,
        "stream/offline PCM must be exact"
    );
    assert_eq!(report.frames as usize * 2, streamed.len());
    assert!(first_block_ms.is_some_and(|ms| ms < started.elapsed().as_secs_f64() * 1000.0));
    eprintln!(
        "real MP3 stream: first block {:.3} ms, total {:.3} ms, {} frames, exact PCM",
        first_block_ms.unwrap(),
        started.elapsed().as_secs_f64() * 1000.0,
        report.frames,
    );
}

#[test]
fn stream_uses_the_voicebank_bound_to_each_track() {
    let mut project = voice_and_wave_project();
    let mut second_voice = match &project.parts[0] {
        UPart::Voice(part) => part.clone(),
        _ => panic!("voice fixture starts with a voice part"),
    };
    second_voice.name = "second bank voice".into();
    second_voice.track_no = 1;
    project.parts.push(UPart::Voice(second_voice));

    let bank_a = voicebank::Voicebank::from_oto_bytes(
        "bank-a",
        "bank-a",
        "voice",
        "voice/oto.ini",
        b"a.wav=a,0,100,0,0,0\n",
    );
    let bank_b = empty_voicebank();
    let voicebanks = [Some(&bank_a), Some(&bank_b)];
    let media = MemoryMediaSource::new();
    media.insert(
        "asset:backing",
        write_wav_16(&tone(SAMPLE_RATE as usize), SAMPLE_RATE),
    );
    let mut cache = None;

    let report = pipeline::render_mixdown_stream_by_track_with_pronunciations(
        &project,
        &voicebanks,
        &SilentRenderer,
        None,
        &mut cache,
        &media,
        None::<&AtomicBool>,
        None,
        None,
        512,
        |_, _| Ok(()),
    )
    .expect("wave asset keeps the mix non-empty");

    assert!(report.tracks[0].phrases_rendered > 0);
    assert_eq!(report.tracks[1].phrases_rendered, 0);
    assert!(report.tracks[1]
        .skipped
        .iter()
        .any(|reason| reason.contains("no phoneme maps")));
}

#[test]
fn stream_from_start_frame_matches_offline_tail_with_waves_and_voice() {
    let source = tone(SAMPLE_RATE as usize * 4);
    let media = MemoryMediaSource::new();
    media.insert("asset:backing", write_wav_16(&source, SAMPLE_RATE));
    let mut project = voice_and_wave_project();
    if let Some(UPart::Wave(wave)) = project
        .parts
        .iter_mut()
        .find(|part| matches!(part, UPart::Wave(_)))
    {
        wave.file_duration_ms = 4000.0;
        wave.skip = 10;
        wave.trim = 12;
        wave.fadein = 200;
        wave.fadeout = 200;
    }
    let bank = voicebank::Voicebank::from_oto_bytes(
        "test",
        "test",
        "voice",
        "voice/oto.ini",
        b"a.wav=a,0,100,0,0,0\n",
    );

    let mut offline_cache = None;
    let offline = pipeline::render_mixdown_with_pronunciations(
        &project,
        &bank,
        &CountingRenderer {
            calls: AtomicUsize::new(0),
        },
        None,
        false,
        &mut offline_cache,
        &media,
        None,
        None,
        None,
        None,
    )
    .expect("offline full render");

    // Mid-wave + mid-voice-chunk playhead seek
    let seek_frame = (SAMPLE_RATE as i64 * 3) / 2;
    let counter_early = CountingRenderer {
        calls: AtomicUsize::new(0),
    };
    let mut stream_cache = None;
    let mut streamed = Vec::new();
    let mut expected_cursor = seek_frame as u64;
    let report = pipeline::render_mixdown_stream_with_pronunciations_from_frame(
        &project,
        &bank,
        &counter_early,
        None,
        &mut stream_cache,
        &media,
        None,
        None,
        None,
        1024,
        seek_frame,
        |frame_start, block| {
            assert_eq!(frame_start, expected_cursor);
            expected_cursor += (block.len() / 2) as u64;
            streamed.extend_from_slice(block);
            Ok(())
        },
    )
    .expect("seeked stream render");

    let tail_start = seek_frame as usize * 2;
    assert_eq!(streamed, offline.samples[tail_start..]);
    assert_eq!(report.frames as usize * 2, streamed.len());

    // Seek near the tail of the voice part (last 2 seconds) must skip the opening chunk.
    let total_frames = (offline.samples.len() / 2) as i64;
    let late_seek_frame = (total_frames - SAMPLE_RATE as i64 * 2).max(seek_frame);
    let counter_late = CountingRenderer {
        calls: AtomicUsize::new(0),
    };
    let mut late_cache = None;
    let mut late_pcm = Vec::new();
    let late_report = pipeline::render_mixdown_stream_with_pronunciations_from_frame(
        &project,
        &bank,
        &counter_late,
        None,
        &mut late_cache,
        &media,
        None,
        None,
        None,
        1024,
        late_seek_frame,
        |_, block| {
            late_pcm.extend_from_slice(block);
            Ok(())
        },
    )
    .expect("late seek stream");
    assert!(
        counter_late.calls.load(Ordering::SeqCst) < counter_early.calls.load(Ordering::SeqCst),
        "late seek should skip earlier voice chunks ({} vs {})",
        counter_late.calls.load(Ordering::SeqCst),
        counter_early.calls.load(Ordering::SeqCst),
    );
    assert_eq!(late_pcm, offline.samples[late_seek_frame as usize * 2..]);
    assert_eq!(late_report.frames as usize * 2, late_pcm.len());
}

#[test]
fn worldline_rs_stream_matches_offline_mixdown_bit_for_bit() {
    let dir = std::env::temp_dir().join(format!("t050-rs-mix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp voicebank dir");
    let wav: Vec<f32> = (0..(SAMPLE_RATE as usize * 8 / 10))
        .map(|i| {
            let t = i as f64 / f64::from(SAMPLE_RATE);
            let w = 2.0 * std::f64::consts::PI * 220.0 * t;
            (0.3 * w.sin() + 0.15 * (2.0 * w).sin()) as f32
        })
        .collect();
    std::fs::write(dir.join("character.txt"), "name=temp\n").expect("write character.txt");
    std::fs::write(dir.join("a.wav"), write_wav_16(&wav, SAMPLE_RATE)).expect("write wav");
    std::fs::write(dir.join("oto.ini"), "a.wav=a,30,80,-350,20,10\n").expect("write oto.ini");
    let bank = voicebank::load_voicebank(&dir).expect("load temp voicebank");

    let mut project = UProject::create();
    project.tracks = vec![UTrack {
        track_name: "voice".into(),
        ..Default::default()
    }];
    let mut voice = UVoicePart::new("voice");
    voice.notes = (0..4)
        .map(|i| UNote {
            position: i * 480,
            duration: 480,
            tone: 60 + i,
            lyric: "a".into(),
            ..Default::default()
        })
        .collect();
    voice.duration = 4 * 480;
    project.voice_parts = Some(vec![voice]);
    let yaml = serde_yaml::to_string(&project).expect("yaml");
    let mut project = UProject::from_yaml_text(&yaml).expect("reload");
    project.after_load().expect("after_load");

    let engine = synth_cli::engine::WorldlineRsEngine::open(false).expect("worldline-rs engine");
    let media = MemoryMediaSource::new();
    let mut off_cache = None;
    let offline = pipeline::render_mixdown_with_pronunciations(
        &project,
        &bank,
        &engine,
        None,
        false,
        &mut off_cache,
        &media,
        None,
        None,
        None,
        None,
    )
    .expect("offline rs mixdown");

    engine.clear_caches();
    let mut st_cache = None;
    let mut streamed = Vec::new();
    let report = pipeline::render_mixdown_stream_with_pronunciations(
        &project,
        &bank,
        &engine,
        None,
        &mut st_cache,
        &media,
        None,
        None,
        None,
        1024,
        |_, block| {
            streamed.extend_from_slice(block);
            Ok(())
        },
    )
    .expect("stream rs mixdown");

    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(streamed.len(), offline.samples.len());
    assert_eq!(report.frames as usize * 2, streamed.len());
    let max_err = streamed
        .iter()
        .zip(&offline.samples)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert_eq!(max_err, 0.0, "worldline-rs mixdown stream max_err={max_err}");
}

#[cfg(feature = "ffi")]
#[test]
#[ignore = "requires libworldline.so, miku-no-shoushitsu.rustx, and full Teto bank; pass --ignored"]
fn t050_real_miku_shoushitsu_track2_and_warm_edit_seek_under_500ms() {
    use std::process::Command;
    use std::time::Instant;

    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../");
    let rustx_path = repo.join("miku-no-shoushitsu.rustx");
    let bank_path =
        repo.join("test-ustx-example/voicebank/extracted/重音テト OU用日本語統合ライブラリー");
    let so_path = std::env::var("WORLDLINE_SO")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| repo.join("native/build/build-linux/libworldline.so"));
    assert!(rustx_path.exists(), "missing {}", rustx_path.display());
    assert!(bank_path.exists(), "missing {}", bank_path.display());
    assert!(so_path.exists(), "missing {}", so_path.display());

    let out = Command::new("unzip")
        .args(["-p", &rustx_path.to_string_lossy(), "project.ustx"])
        .output()
        .expect("unzip project.ustx");
    assert!(out.status.success(), "unzip failed");
    let yaml = String::from_utf8(out.stdout).expect("utf8 project.ustx");
    let mut full_project = UProject::from_yaml_text(&yaml).expect("parse miku-no-shoushitsu");
    full_project.after_load().expect("after_load");

    let bank = voicebank::load_voicebank(&bank_path).expect("load Teto voicebank");
    let mut track2 = full_project.tracks[2].clone();
    track2.phonemizer = None;
    let main_part = full_project
        .parts
        .iter()
        .find_map(|p| match p {
            UPart::Voice(v) if v.track_no == 2 && v.name == "main" => Some(v.clone()),
            _ => None,
        })
        .expect("Track 2 'main' voice part");

    let phrases = pipeline::build_phrase_inputs_with_choices(
        &full_project,
        &track2,
        &main_part,
        &bank,
        phonemizer::PhonemizerKind::Default,
        None,
    )
    .expect("build Track 2 phrases");

    let mapped_phrases: Vec<_> = phrases
        .iter()
        .enumerate()
        .filter(|(_, p)| p.input.sample_based.is_some())
        .collect();
    for (idx, p) in &mapped_phrases {
        if p.input.notes.len() >= 10 {
            eprintln!(
                "Track2 main phrase[{idx}]: notes={} phonemes={} pos_ms={:.1} dur_ms={:.1}",
                p.input.notes.len(),
                p.input.phonemes.len(),
                p.group.position_ms,
                p.input.phrase.duration_ms
            );
        }
    }

    let dense_phrase = mapped_phrases
        .iter()
        .map(|(_, p)| *p)
        .find(|p| p.input.notes.len() == 25)
        .unwrap_or_else(|| {
            mapped_phrases
                .iter()
                .map(|(_, p)| *p)
                .filter(|p| p.input.notes.len() >= 20 && p.input.notes.len() <= 40)
                .max_by(|a, b| a.input.phrase.duration_ms.total_cmp(&b.input.phrase.duration_ms))
                .expect("dense ~25-note phrase on Track 2")
        });
    let long_phrase = mapped_phrases
        .iter()
        .map(|(_, p)| *p)
        .max_by_key(|p| p.input.notes.len())
        .expect("long phrase on Track 2");

    let renderer = worldline_plugin::WorldlineRenderer::open(&so_path).expect("open renderer");

    // 1. Real phrases via C++ WorldlineRenderer (cold & warm, strict bit-exact parity)
    for (label, phrase) in [
        ("miku_track2_dense", dense_phrase),
        ("miku_track2_long", long_phrase),
    ] {
        renderer.clear_caches();
        let offline = renderer
            .render_phrase(&phrase.input)
            .expect("cpp offline phrase");

        renderer.clear_caches();
        let mut cold_pcm = Vec::new();
        let (cold_len, cold_stats) = renderer
            .render_phrase_stream_with_stats(&phrase.input, 4096, |block| {
                cold_pcm.extend_from_slice(block);
                Ok(())
            })
            .expect("cpp cold stream phrase");
        assert_eq!(cold_len, offline.len());
        let max_err = cold_pcm
            .iter()
            .zip(&offline)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        let dot: f64 = cold_pcm
            .iter()
            .zip(&offline)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum();
        let na: f64 = cold_pcm.iter().map(|x| f64::from(*x).powi(2)).sum();
        let nb: f64 = offline.iter().map(|x| f64::from(*x).powi(2)).sum();
        let corr = dot / (na * nb).sqrt().max(f64::MIN_POSITIVE);

        let mut warm_pcm = Vec::new();
        let (warm_len, warm_stats) = renderer
            .render_phrase_stream_with_stats(&phrase.input, 4096, |block| {
                warm_pcm.extend_from_slice(block);
                Ok(())
            })
            .expect("cpp warm stream phrase");
        assert_eq!(warm_len, offline.len());
        let warm_err = warm_pcm
            .iter()
            .zip(&offline)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);

        eprintln!(
            "T050 REAL {label}: notes={} dur_s={:.2} | cold(ttfs={:.2}ms total={:.2}ms workers=1+{} prep_wait={:.2}ms bp_wait={:.2}ms peak_models={} peak_frames={} misses={} max_err={max_err:.3e} corr={corr:.8}) | warm(ttfs={:.2}ms total={:.2}ms hits={} max_err={warm_err:.3e})",
            phrase.input.notes.len(),
            phrase.input.phrase.duration_ms / 1000.0,
            cold_stats.first_block_ms,
            cold_stats.total_ms,
            cold_stats.prep_workers,
            cold_stats.prep_wait_ms,
            cold_stats.backpressure_wait_ms,
            cold_stats.peak_buffered_models,
            cold_stats.peak_timeline_frames,
            cold_stats.cache_misses,
            warm_stats.first_block_ms,
            warm_stats.total_ms,
            warm_stats.cache_hits,
        );
        assert_eq!(max_err, 0.0);
        assert_eq!(warm_err, 0.0);
        assert!(corr > 0.9999999);
    }

    // 2. Real dense phrase via Pure-Rust WorldlineRsEngine (bit-exact + cache)
    let rs_engine = synth_cli::engine::WorldlineRsEngine::open(false).expect("rs engine");
    rs_engine.clear_caches();
    let rs_offline = rs_engine
        .render_phrase(&dense_phrase.input)
        .expect("rs dense offline");
    rs_engine.clear_caches();
    let mut rs_cold = Vec::new();
    let (rs_cold_len, rs_cold_stats) = rs_engine
        .render_phrase_stream_with_stats(&dense_phrase.input, 4096, |b| {
            rs_cold.extend_from_slice(b);
            Ok(())
        })
        .expect("rs dense cold stream");
    assert_eq!(rs_cold_len, rs_offline.len());
    let rs_cold_err = rs_cold
        .iter()
        .zip(&rs_offline)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert_eq!(rs_cold_err, 0.0);

    let mut rs_warm = Vec::new();
    let (_, rs_warm_stats) = rs_engine
        .render_phrase_stream_with_stats(&dense_phrase.input, 4096, |b| {
            rs_warm.extend_from_slice(b);
            Ok(())
        })
        .expect("rs dense warm stream");
    let rs_warm_err = rs_warm
        .iter()
        .zip(&rs_offline)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    eprintln!(
        "T050 PURE-RUST miku_track2_dense: notes={} | cold(ttfs={:.2}ms total={:.2}ms workers=1+{} prep_wait={:.2}ms bp_wait={:.2}ms peak_models={} peak_frames={} max_err={rs_cold_err:.3e}) | warm(ttfs={:.2}ms total={:.2}ms hits={} max_err={rs_warm_err:.3e})",
        dense_phrase.input.notes.len(),
        rs_cold_stats.first_block_ms,
        rs_cold_stats.total_ms,
        rs_cold_stats.prep_workers,
        rs_cold_stats.prep_wait_ms,
        rs_cold_stats.backpressure_wait_ms,
        rs_cold_stats.peak_buffered_models,
        rs_cold_stats.peak_timeline_frames,
        rs_warm_stats.first_block_ms,
        rs_warm_stats.total_ms,
        rs_warm_stats.cache_hits,
    );
    assert_eq!(rs_warm_err, 0.0);
    assert_eq!(rs_warm_stats.cache_misses, 0);

    // 3. Warmed real Track 2 project + localized 1-note edit + playhead seek < 500 ms
    let mut track2_proj = full_project.clone();
    let mut t2 = track2.clone();
    t2.phonemizer = None;
    track2_proj.tracks = vec![t2];
    let mut track2_main = main_part.clone();
    track2_main.track_no = 0;
    track2_proj.wave_parts = None;
    track2_proj.voice_parts = Some(vec![track2_main]);
    let t2_yaml = serde_yaml::to_string(&track2_proj).expect("serialize track2 proj");
    let mut track2_proj = UProject::from_yaml_text(&t2_yaml).expect("reload track2 proj");
    track2_proj.after_load().expect("after_load track2 proj");

    let media = MemoryMediaSource::new();
    let cache_dir = std::env::temp_dir().join(format!("t050-rcache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache_dir);
    std::fs::create_dir_all(&cache_dir).expect("cache dir");
    let mut render_cache = Some(runtime::RenderCache::new_in_dir(&cache_dir, 128 * 1024 * 1024));

    // Warm full Track 2 project with C++ WorldlineRenderer
    renderer.clear_caches();
    let _ = pipeline::render_mixdown_stream_with_pronunciations(
        &track2_proj,
        &bank,
        &renderer,
        Some(phonemizer::PhonemizerKind::Default),
        &mut render_cache,
        &media,
        None,
        None,
        None,
        2048,
        |_, _| Ok(()),
    )
    .expect("warm Track 2 mixdown");

    // Pick a note inside the long passage around its middle, edit its pitch (+2 semitones),
    // and immediately hit Play from that note's playhead frame!
    let long_first_ms = long_phrase.group.position_ms;
    let target_note_pos = track2_proj.parts.iter().find_map(|p| match p {
        UPart::Voice(v) => v
            .notes
            .iter()
            .find(|n| track2_proj.time_axis.tick_to_ms(f64::from(n.position)) >= long_first_ms)
            .map(|n| n.position),
        _ => None,
    })
    .expect("note in long passage");
    if let Some(UPart::Voice(v)) = track2_proj.parts.iter_mut().find(|p| matches!(p, UPart::Voice(_))) {
        let note = v
            .notes
            .iter_mut()
            .find(|n| n.position == target_note_pos)
            .expect("target note mut");
        note.tone += 2;
    }
    let seek_ms = track2_proj.time_axis.tick_to_ms(f64::from(target_note_pos));
    let seek_frame = ((seek_ms * f64::from(SAMPLE_RATE)) / 1000.0) as i64;

    let play_start = Instant::now();
    let mut ttfs_ms = None;
    let mut edited_seek_pcm = Vec::new();
    let seek_report = pipeline::render_mixdown_stream_with_pronunciations_from_frame(
        &track2_proj,
        &bank,
        &renderer,
        Some(phonemizer::PhonemizerKind::Default),
        &mut render_cache,
        &media,
        None,
        None,
        None,
        2048,
        seek_frame,
        |_, block| {
            ttfs_ms.get_or_insert_with(|| play_start.elapsed().as_secs_f64() * 1000.0);
            edited_seek_pcm.extend_from_slice(block);
            Ok(())
        },
    )
    .expect("warmed localized-edit playhead seek stream");
    let seek_total_ms = play_start.elapsed().as_secs_f64() * 1000.0;
    let ttfs_ms = ttfs_ms.expect("at least one block");

    let verifier_renderer = worldline_plugin::WorldlineRenderer::open(&so_path).expect("verifier");
    let mut off_cache = None;
    let edited_offline = pipeline::render_mixdown_with_pronunciations(
        &track2_proj,
        &bank,
        &verifier_renderer,
        Some(phonemizer::PhonemizerKind::Default),
        false,
        &mut off_cache,
        &media,
        None,
        None,
        None,
        None,
    )
    .expect("edited offline reference");

    let tail_err = edited_seek_pcm
        .iter()
        .zip(&edited_offline.samples[seek_frame as usize * 2..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let _ = std::fs::remove_dir_all(&cache_dir);

    eprintln!(
        "T050 REAL WARMED EDIT + PLAYHEAD SEEK ({seek_ms:.1} ms / frame {seek_frame}): \
         first_block_ms={ttfs_ms:.2} total_ms={seek_total_ms:.2} emitted_frames={} tail_max_err={tail_err:.3e}",
        seek_report.frames,
    );
    assert_eq!(edited_seek_pcm.len(), edited_offline.samples.len() - seek_frame as usize * 2);
    assert_eq!(tail_err, 0.0, "seeked localized-edit tail max_err={tail_err}");
    assert!(
        ttfs_ms < 500.0,
        "warmed project localized-edit playhead seek TTFS {ttfs_ms:.2} ms >= 500 ms"
    );
}
