//! T26: `PlannedChunk.position_ms` is the chunk's NOTE start, never the buffer
//! origin.
//!
//! `runtime::mixer::mix` aligns every chunk at `position_ms - leading_ms` — it
//! subtracts the leading itself. `plan_track_chunks` used to report
//! `chunk.start_ms - leading_ms` (the buffer origin), so the pool/worker path
//! subtracted the leading a SECOND time and every chunk landed `leading_ms`
//! early (T24: pool 6 samples short of sequential; DIAG-FIX `+leading_ms` was
//! bit-identical). The single-threaded path was unaffected because
//! `trim_chunk_audio` returns `chunk.start_ms` (T9).
//!
//! ```sh
//! cargo test -p synth-cli --test plan_convention
//! cargo test -p synth-cli --test plan_convention -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use runtime::mixer::{mix, AudioChunk, MixInput, TrackSpec};
use synth_cli::pipeline::{self, PhonemizerKind};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[test]
fn planned_position_ms_is_the_note_start_not_the_buffer_origin() {
    let root = repo_root();
    let project = pipeline::load_project(&root.join("test/golden/one-note.ustx"))
        .expect("load one-note.ustx");
    let voicebank = pipeline::load_voicebank(&root.join("test/golden/teto-english/library"))
        .expect("load teto bank");
    let (phrases, planned, skipped) =
        pipeline::plan_track_chunks(&project, &voicebank, 0, PhonemizerKind::Universal)
            .expect("plan chunks");
    assert!(skipped.is_empty(), "unexpected skips: {skipped:?}");
    assert!(
        !planned.is_empty(),
        "the fixture must plan at least one chunk"
    );

    let mut with_leading = 0;
    for p in &planned {
        let phrase = &phrases[p.phrase_index];
        let own_first = &phrase.input.phonemes[p.chunk.own_range.start];
        assert!(
            (p.leading_ms - own_first.leading_ms).abs() < 1e-9,
            "leading_ms must be the own-span first phoneme's leading ({} vs {})",
            p.leading_ms,
            own_first.leading_ms
        );
        assert!(
            (p.position_ms - p.chunk.start_ms).abs() < 1e-9,
            "position_ms must be chunk.start_ms (the note start): {} vs {}",
            p.position_ms,
            p.chunk.start_ms
        );
        if p.leading_ms > 0.0 {
            with_leading += 1;
            // The old (buggy) convention was the buffer origin. This guards it.
            assert!(
                (p.position_ms - (p.chunk.start_ms - p.leading_ms)).abs() > 1e-9,
                "position_ms must NOT be the buffer origin (start_ms - leading)"
            );
        }
    }
    assert!(
        with_leading > 0,
        "the fixture must exercise a non-zero leading"
    );
}

/// The Rust side of `web/run-pool.sh`: plan -> render_planned_chunk -> mixer
/// must be bit-identical to the single-threaded `render_project`.
#[test]
#[ignore = "needs libworldline.so (run with --ignored)"]
fn planned_chunks_mix_bit_identically_to_the_sequential_render() {
    let root = repo_root();
    let so = std::env::var("WORLDLINE_SO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("native/build/build-linux/libworldline.so"));
    if !so.is_file() {
        eprintln!("libworldline.so not found at {}; skipping", so.display());
        return;
    }
    let project = pipeline::load_project(&root.join("test/golden/one-note.ustx")).expect("project");
    let voicebank =
        pipeline::load_voicebank(&root.join("test/golden/teto-english/library")).expect("bank");
    let renderer = worldline_plugin::WorldlineRenderer::open(&so).expect("open libworldline.so");

    let seq = pipeline::render_project(
        &project,
        &voicebank,
        &renderer,
        0,
        PhonemizerKind::Universal,
        false,
        &mut None,
        None,
        None,
    )
    .expect("sequential render")
    .samples;

    let (phrases, planned, _) =
        pipeline::plan_track_chunks(&project, &voicebank, 0, PhonemizerKind::Universal)
            .expect("plan");
    let inputs: Vec<MixInput> = planned
        .iter()
        .map(|p| {
            let samples =
                pipeline::render_planned_chunk(&phrases, p, &renderer).expect("render chunk");
            MixInput {
                chunk: AudioChunk {
                    samples,
                    position_ms: p.position_ms,
                    leading_ms: p.leading_ms,
                    hash: 0,
                },
                track: TrackSpec::default(),
            }
        })
        .collect();
    let pooled = mix(&inputs);

    println!(
        "pool {} samples · sequential {} samples",
        pooled.samples.len(),
        seq.len()
    );
    assert_eq!(
        pooled.samples.len(),
        seq.len(),
        "pool length must equal the sequential render"
    );
    let max = pooled
        .samples
        .iter()
        .zip(seq.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert_eq!(
        max, 0.0,
        "pool must be bit-identical to the sequential render (max|diff| = {max})"
    );
}
