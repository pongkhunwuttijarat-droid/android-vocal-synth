//! Decode-level tests for the `media` crate.
//!
//! The WAV case is self-contained (we encode with our own writer and decode with
//! symphonia, so a regression in either shows up as a mismatch). The MP3 case
//! uses the wave part of the real fixture project; that media lives outside the
//! repository, so the test reports a skip when it is absent instead of failing.

use media::{
    decode_mono_44100, decode_stereo_44100, decode_stereo_44100_blocks, probe, MediaSource,
    MemoryMediaSource,
};
use wavwriter::write_wav_16;

fn sine(n: usize, rate: u32, hz: f64) -> Vec<f32> {
    (0..n)
        .map(|i| {
            (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate))).sin() as f32
        })
        .collect()
}

#[test]
fn wav_round_trips_through_the_decoder() {
    let rate = 44100;
    let original = sine(rate as usize / 10, rate, 440.0);
    let bytes = write_wav_16(&original, rate);

    let info = probe(&bytes).expect("probe a wav we just wrote");
    assert_eq!(info.sample_rate, rate);
    assert_eq!(info.channels, 1);
    assert!(
        (info.duration_ms - 100.0).abs() < 1.0,
        "duration was {}",
        info.duration_ms
    );

    let decoded = decode_mono_44100(&bytes).expect("decode a wav we just wrote");
    assert_eq!(decoded.len(), original.len(), "sample count must match");
    // 16-bit quantisation is the only permitted difference.
    let worst = original
        .iter()
        .zip(&decoded)
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(worst <= 1.0 / 32768.0 + 1e-6, "worst |diff| = {worst}");
}

#[test]
fn stereo_stream_emits_bounded_blocks_and_matches_whole_decode() {
    let rate = 44100;
    let original = sine(rate as usize / 5, rate, 440.0);
    let bytes = write_wav_16(&original, rate);
    let whole = decode_stereo_44100(&bytes).expect("whole decode");
    let mut streamed = Vec::new();
    let mut block_count = 0;
    decode_stereo_44100_blocks(&bytes, 997, |block| {
        assert!(!block.is_empty());
        assert_eq!(block.len() % 2, 0);
        assert!(block.len() <= 997 * 2);
        block_count += 1;
        streamed.extend_from_slice(block);
        Ok(())
    })
    .expect("streamed decode");
    assert!(block_count > 1, "decoder should publish before EOF");
    assert_eq!(streamed, whole, "streaming must preserve PCM exactly");
}

#[test]
fn garbage_bytes_are_rejected_rather_than_silently_zeroed() {
    assert!(probe(b"not audio at all").is_err());
    assert!(decode_mono_44100(b"not audio at all").is_err());
    assert!(decode_mono_44100(&[]).is_err());
}

#[test]
fn memory_source_feeds_the_decoder_end_to_end() {
    let rate = 44100;
    let original = sine(4410, rate, 220.0);
    let mut src = MemoryMediaSource::new();
    src.insert("bg.wav", write_wav_16(&original, rate));
    let bytes = src.load("bg.wav").expect("registered blob");
    let decoded = decode_mono_44100(&bytes).expect("decode");
    assert_eq!(decoded.len(), original.len());
}

/// The fixture project's wave part points at an MP3 outside the repository
/// (`../../../../../home/<user>/…`). When it is reachable we decode it for real
/// and check the reported duration against the `file_duration_ms` the `.ustx`
/// recorded, which is the number the part is placed by.
#[test]
fn decodes_the_fixture_projects_mp3_wave_part_when_present() {
    let project_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../test-ustx-example");
    let project = project_dir.join("golden-the-end-of-hutsune-miku.ustx");
    if !project.exists() {
        eprintln!("skip: {} not present", project.display());
        return;
    }
    let yaml = std::fs::read_to_string(&project).expect("read fixture");
    let reference = yaml
        .lines()
        .find_map(|l| l.trim().strip_prefix("relative_path:"))
        .map(|v| v.trim().trim_matches('\'').trim_matches('"'))
        .expect("fixture has a wave part with a relative_path");

    let src = media::FileMediaSource::new(&project_dir);
    let resolved = src.resolve(reference).unwrap_or_else(|| {
        panic!(
            "cannot resolve '{reference}' from {}",
            project_dir.display()
        )
    });
    if !resolved.exists() {
        eprintln!("skip: media {} not present", resolved.display());
        return;
    }

    let bytes = src.load(reference).expect("load media");
    let info = probe(&bytes).expect("probe mp3");
    assert!(info.sample_rate > 0);
    assert!(info.channels >= 1);
    println!(
        "fixture mp3: {} Hz, {} ch, {:.0} ms",
        info.sample_rate, info.channels, info.duration_ms
    );
    // The `.ustx` says 288391.8367 ms; an MP3 frame-count estimate lands within
    // a frame or two of that.
    let declared: f64 = yaml
        .lines()
        .find_map(|l| l.trim().strip_prefix("file_duration_ms:"))
        .and_then(|v| v.trim().parse().ok())
        .expect("fixture declares file_duration_ms");
    assert!(
        (info.duration_ms - declared).abs() < 200.0,
        "probed {} ms vs declared {declared} ms",
        info.duration_ms
    );

    let samples = decode_mono_44100(&bytes).expect("decode mp3");
    assert!(!samples.is_empty());
    let gapless_frames = (info.duration_ms * 44.1).round() as usize;
    // The MP3 demuxer's duration is based on Xing frame metadata and may be
    // off by a few dozen samples; the decoded PCM count is the exact duration.
    // Keep the reported and decoded lengths within one MPEG frame.
    assert!(
        samples.len().abs_diff(gapless_frames) <= 1152,
        "gapless MP3 decode produced {} frames, probe reports {gapless_frames}",
        samples.len()
    );
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.01, "decoded audio is silent (peak {peak})");
    println!(
        "decoded {} samples ({:.1} s), peak {peak:.3}",
        samples.len(),
        samples.len() as f64 / 44100.0
    );
}
