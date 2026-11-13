//! Wave (audio) parts must reach the mix, and missing media must be reported.
//!
//! Wave parts were modelled and round-tripped through `.ustx` for a long time
//! but never mixed: `grep 'UPart::Wave'` in `phrase/` and `pipeline.rs` was
//! empty, so a project with an audio track exported vocals only. These tests
//! drive the real [`pipeline::render_mixdown`] with a stub renderer so the
//! assertions are about the mixdown, not about synthesis quality.
//!
//! No `.so`, no voicebank samples: the voice track is empty and every sample in
//! the output must come from the media.

use domain::{UPart, UProject, UTrack, UWavePart};
use feed::render_input::RenderInput;
use media::{FileMediaSource, MediaSource, MemoryMediaSource};
use runtime::mixer::SAMPLE_RATE;
use synth_cli::pipeline::{self, MixdownReport, PhraseRenderer};
use wavwriter::write_wav_16;

/// Emits nothing: the mixdown then contains only what the media contributed,
/// which makes every assertion below exact.
struct SilentRenderer;

impl PhraseRenderer for SilentRenderer {
    fn render_phrase(&self, _input: &RenderInput) -> Result<Vec<f32>, String> {
        Ok(Vec::new())
    }
}

fn media_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rusted-utau-wave-mix-{}", std::process::id()))
}

fn tone(n: usize, hz: f64) -> Vec<f32> {
    (0..n)
        .map(|i| {
            (0.4 * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(SAMPLE_RATE)).sin())
                as f32
        })
        .collect()
}

/// A project with one (silent) voice track and one wave part on track 1.
///
/// 120 bpm, 4/4 → 480 ticks per beat → 500 ms per beat, i.e. **1.0417 ms per
/// tick** (a common trap: 480 ticks/beat is not 2 ms/tick at 120 bpm). The wave
/// part starts at tick 0 with a 1000 ms file, so it lands on exactly the first
/// second.
fn project_with_wave_part(reference: &str, file_duration_ms: f64) -> UProject {
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
        name: "bg.wav".into(),
        track_no: 1,
        position: 0,
        relative_path: Some(reference.into()),
        file_duration_ms,
        skip: 0,
        trim: 0,
        fadein: 0,
        fadeout: 0,
        ..Default::default()
    }]);
    let text = serde_yaml::to_string(&project).expect("serialize");
    let mut reloaded = UProject::from_yaml_text(&text).expect("reload");
    // `create()` builds a default axis; reloading through `from_yaml_text` does
    // not run `after_load`, so build the axis the way the loader does.
    reloaded.after_load().expect("after_load");
    reloaded
}

/// A bank with no samples: the mixdown never consults it because the voice
/// track is empty, but the API requires one.
fn empty_voicebank() -> voicebank::Voicebank {
    voicebank::Voicebank::from_oto_bytes("empty", "empty", "voice", "voice/oto.ini", b"")
}

fn mixdown(project: &UProject, media: &dyn MediaSource) -> Result<MixdownReport, String> {
    let mut cache = None;
    pipeline::render_mixdown(
        project,
        &empty_voicebank(),
        &SilentRenderer,
        None,
        false,
        &mut cache,
        media,
        None,
        None,
        None,
    )
}

#[test]
fn a_wave_part_is_mixed_at_its_timeline_position() {
    let root = media_root();
    std::fs::create_dir_all(&root).unwrap();
    let samples = tone(SAMPLE_RATE as usize, 440.0);
    std::fs::write(root.join("bg.wav"), write_wav_16(&samples, SAMPLE_RATE)).unwrap();

    let project = project_with_wave_part("bg.wav", 1000.0);
    let media = FileMediaSource::new(&root);
    let report = mixdown(&project, &media).expect("mixdown");

    assert_eq!(report.sample_rate, SAMPLE_RATE);
    assert_eq!(report.channels, 2);
    assert_eq!(
        report.tracks[1].wave_parts_mixed, 1,
        "wave part was not mixed: {:?}",
        report.tracks[1].skipped
    );
    assert_eq!(report.samples.len(), samples.len() * 2);
    // Mono media is copied to both output channels, then the track's centered
    // equal-power fader applies sqrt(1/2) to each channel, as OpenUtau does.
    let center_gain = std::f32::consts::FRAC_1_SQRT_2;
    let worst = report
        .samples
        .chunks_exact(2)
        .zip(&samples)
        .map(|(frame, source)| {
            (frame[0] - source * center_gain)
                .abs()
                .max((frame[1] - source * center_gain).abs())
        })
        .fold(0f32, f32::max);
    assert!(worst <= 1.0 / 32768.0 + 1e-6, "worst |diff| = {worst}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_track_holding_only_a_wave_part_is_not_an_error() {
    // The per-track render path errors with "no voice part found on track N";
    // the mixdown must not, because an audio-only track is legal.
    let mut src = MemoryMediaSource::new();
    src.insert("bg.wav", write_wav_16(&tone(2205, 440.0), SAMPLE_RATE));
    let project = project_with_wave_part("bg.wav", 50.0);
    let report = mixdown(&project, &src).expect("mixdown");
    assert_eq!(report.tracks[0].phrases_rendered, 0);
    assert_eq!(report.tracks[1].wave_parts_mixed, 1);
    assert!(!report.samples.is_empty());
}

#[test]
fn missing_media_is_reported_instead_of_silently_dropped() {
    let project = project_with_wave_part("nope.wav", 1000.0);
    let media = MemoryMediaSource::new();
    // Nothing else in the project can produce audio, so this must fail — and
    // the failure has to name the missing reference rather than just saying
    // "no audio".
    let err = mixdown(&project, &media).expect_err("unresolvable media must fail");
    assert!(err.contains("nothing to mix"), "{err}");
    assert!(
        err.contains("nope.wav"),
        "error should name the reference: {err}"
    );
}

#[test]
fn a_project_with_no_parts_at_all_is_an_error_not_an_empty_wav() {
    let mut project = UProject::create();
    project.tracks = vec![UTrack::default()];
    let media = MemoryMediaSource::new();
    let err = mixdown(&project, &media).expect_err("nothing to mix must fail");
    assert!(err.contains("nothing to mix"), "{err}");
}

#[test]
fn skip_and_trim_shorten_the_media_that_reaches_the_mix() {
    let root = media_root();
    std::fs::create_dir_all(&root).unwrap();
    // 1000 ms of 44.1 kHz. skip = 100 ticks = 200 ms, trim = 200 ticks = 400 ms.
    let samples = tone(SAMPLE_RATE as usize, 440.0);
    std::fs::write(root.join("bg.wav"), write_wav_16(&samples, SAMPLE_RATE)).unwrap();

    let mut project = project_with_wave_part("bg.wav", 1000.0);
    // `after_load` has already folded `wave_parts` into `parts`, so the part to
    // edit is the one in `parts`.
    for part in project.parts.iter_mut() {
        if let UPart::Wave(w) = part {
            w.skip = 100;
            w.trim = 200;
        }
    }

    let media = FileMediaSource::new(&root);
    let report = mixdown(&project, &media).expect("mixdown");
    assert_eq!(report.tracks[1].wave_parts_mixed, 1);
    // 1000 ms of file, minus `skip` (100 ticks) and `trim` (200 ticks) worth of
    // audio. At 120 bpm a tick is 60_000/(480*120) = 1.0417 ms, so 300 ticks
    // (312.5 ms) are cut and 687.5 ms survive. The per-part frame counts truncate
    // like the C# casts, hence the small tolerance.
    let ms_per_tick = 60_000.0 / (480.0 * 120.0);
    let expected = (1000.0 - 300.0 * ms_per_tick) * f64::from(SAMPLE_RATE) / 1000.0;
    assert!(
        ((report.samples.len() / report.channels as usize) as f64 - expected).abs() <= 4.0,
        "expected ~{expected} samples, got {}",
        report.samples.len() / report.channels as usize
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_muted_track_still_reports_its_wave_part_but_contributes_no_audio() {
    // OpenUtau mutes a track by scaling the fader to `-24 dB`, which
    // `DecibelToVolume` maps to 0 — not by dropping the track.
    let mut src = MemoryMediaSource::new();
    src.insert("bg.wav", write_wav_16(&tone(2205, 440.0), SAMPLE_RATE));
    let mut project = project_with_wave_part("bg.wav", 50.0);
    project.tracks[1].mute = true;
    project.after_load().expect("after_load");
    let report = mixdown(&project, &src).expect("mixdown");
    assert_eq!(report.tracks[1].wave_parts_mixed, 1);
    let peak = report.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    assert_eq!(peak, 0.0, "a muted track must be silent, peak was {peak}");
}
// ---------------------------------------------------------------------------
// The per-track render path (`render_track_with_media`) — what the editor's
// §6.13 render job calls.
//
// Before this existed, `render_project_with_progress_and_pronunciations`
// answered "no voice part found on track N" for an audio-only track and
// silently ignored wave parts on a track that also had voice, so an audio
// track could never be heard or drawn.
// ---------------------------------------------------------------------------

fn track_render(
    project: &UProject,
    track_no: i32,
    media: &dyn MediaSource,
) -> Result<synth_cli::pipeline::RenderReport, String> {
    let mut cache = None;
    let kind = pipeline::PhonemizerKind::from_track(
        project
            .tracks
            .get(track_no as usize)
            .expect("track exists in the fixture"),
    );
    pipeline::render_track_with_media(
        project,
        &empty_voicebank(),
        &SilentRenderer,
        track_no,
        kind,
        false,
        &mut cache,
        media,
        None,
        None,
        None,
        None,
        None,
    )
}

#[test]
fn a_wave_only_track_renders_through_the_per_track_path() {
    let mut src = MemoryMediaSource::new();
    let original = tone(2205, 440.0); // 50 ms
    src.insert("bg.wav", write_wav_16(&original, SAMPLE_RATE));
    let project = project_with_wave_part("bg.wav", 50.0);

    let report = track_render(&project, 1, &src).expect("render track 1");
    assert_eq!(report.phrases_rendered, 0);
    assert_eq!(report.wave_parts_mixed, 1, "skipped: {:?}", report.skipped);
    assert_eq!(report.channels, 2, "track renders use interleaved stereo");
    assert_eq!(
        report.samples.len(),
        original.len() * report.channels as usize,
        "the decoded media must reach both track channels at its own frame length"
    );
    let center_gain = std::f32::consts::FRAC_1_SQRT_2;
    let worst = report
        .samples
        .chunks_exact(2)
        .zip(&original)
        .flat_map(|(stereo, mono)| {
            [
                (stereo[0] - mono * center_gain).abs(),
                (stereo[1] - mono * center_gain).abs(),
            ]
        })
        .fold(0f32, f32::max);
    assert!(worst <= 1.0 / 32768.0 + 1e-6, "worst |diff| = {worst}");
}

#[test]
fn an_unresolvable_wave_part_is_reported_on_the_track_render() {
    let project = project_with_wave_part("gone.wav", 1000.0);
    let src = MemoryMediaSource::new();
    let err = track_render(&project, 1, &src).expect_err("nothing to render");
    assert!(
        err.contains("gone.wav"),
        "error must name the reference: {err}"
    );
}

#[test]
fn a_track_with_neither_voice_nor_wave_says_so() {
    let mut project = UProject::create();
    project.tracks = vec![UTrack::default()];
    project.after_load().expect("after_load");
    let src = MemoryMediaSource::new();
    let err = track_render(&project, 0, &src).expect_err("empty track");
    assert!(err.contains("no voice part and no wave part"), "{err}");
}

#[test]
fn an_untouched_voice_track_renders_exactly_as_before() {
    // Regression guard for the new entry point: with no wave part on the track
    // its output must be byte-identical to the export path it wraps, or every
    // golden per-track WAV comparison would silently drift.
    let mut src = MemoryMediaSource::new();
    src.insert("bg.wav", write_wav_16(&tone(2205, 440.0), SAMPLE_RATE));
    let mut project = project_with_wave_part("bg.wav", 50.0);
    // Give track 0 a voice part so the voice path actually runs.
    let mut voice = domain::UVoicePart {
        position: 0,
        duration: 480 * 2,
        ..Default::default()
    };
    voice.notes = vec![project.create_note_at(66, 0, 480)];
    project.parts.push(UPart::Voice(voice));
    project.after_load().expect("after_load");

    let mut cache = None;
    let export = pipeline::render_project_with_progress_and_pronunciations(
        &project,
        &empty_voicebank(),
        &SilentRenderer,
        0,
        pipeline::PhonemizerKind::from_track(&project.tracks[0]),
        false,
        &mut cache,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("export path");

    let mut cache = None;
    let preview = pipeline::render_track_with_media(
        &project,
        &empty_voicebank(),
        &SilentRenderer,
        0,
        pipeline::PhonemizerKind::from_track(&project.tracks[0]),
        false,
        &mut cache,
        &src,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("preview path");

    assert_eq!(preview.wave_parts_mixed, 0);
    assert_eq!(preview.samples, export.samples, "voice-only track changed");
}
