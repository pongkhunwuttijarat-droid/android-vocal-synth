//! End-to-end pipeline test: phoneme-like inputs → chunk → schedule →
//! cache → mix, using only local types (the crate is generic; `feed` is
//! not involved).

mod common;

use runtime::cache::RenderCache;
use runtime::chunker::Chunker;
use runtime::mixer::{AudioChunk, MixInput, TrackSpec, SAMPLE_RATE};
use runtime::scheduler::{Job, Scheduler};
use runtime::HashKey;

/// Stand-in for `feed::RenderInput` — the same shape (phoneme, timing,
/// singer) without the feed dependency.
#[derive(Clone, Debug, PartialEq)]
struct PhoneInput {
    phoneme: String,
    position_ms: f64,
    duration_ms: f64,
    tone: i32,
    singer: String,
}

impl HashKey for PhoneInput {
    fn write_hash(&self, out: &mut dyn std::io::Write) -> std::io::Result<()> {
        self.phoneme.write_hash(out)?;
        (self.position_ms as f32).write_hash(out)?;
        (self.duration_ms as f32).write_hash(out)?;
        self.tone.write_hash(out)?;
        self.singer.write_hash(out)
    }
}

/// Fake renderer: returns a chunk whose samples encode the input hash so
/// cache hits are detectable.
fn fake_render(phone: &PhoneInput) -> Vec<f32> {
    let hash = Chunker::<PhoneInput>::hash_items(std::slice::from_ref(phone));
    vec![(hash & 0xff) as f32 / 255.0; 100]
}

#[test]
fn pipeline_chunk_schedule_cache_mix() {
    let tmp = common::TempDir::new("pipeline");

    // 1. Feed-like input.
    let phones: Vec<PhoneInput> = (0..10)
        .map(|i| PhoneInput {
            phoneme: format!("p{i}"),
            position_ms: i as f64 * 100.0,
            duration_ms: 90.0,
            tone: 60 + i,
            singer: "teto".into(),
        })
        .collect();

    // 2. Chunk.
    let chunker = Chunker::<PhoneInput>::new(4, 1);
    let chunks = chunker.split(&phones);
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].items.len(), 4);
    assert_eq!(chunks[1].items[0], chunks[0].items[3], "overlap context");

    // 3. Schedule + render each chunk through the cache.
    let mut sched: Scheduler<Vec<PhoneInput>> = Scheduler::new();
    let ids: Vec<_> = chunks
        .iter()
        .map(|c| {
            sched.enqueue(
                Job::new(c.items.clone())
                    .with_max_attempts(2)
                    .with_progress(|_| {}),
            )
        })
        .collect();
    assert_eq!(sched.pending(), 4);

    let mut cache = RenderCache::new_in_dir(tmp.path(), 1_000_000);
    let mut rendered: Vec<(u64, Vec<f32>)> = Vec::new();
    let mut compute_count = 0;
    let mut progress_log: Vec<(u64, f32)> = Vec::new();

    while let Some((id, input)) = sched.next_ready() {
        let mut done = 0usize;
        let mut audio: Vec<f32> = Vec::new();
        for phone in &input {
            let key = Chunker::<PhoneInput>::hash_items(std::slice::from_ref(phone));
            let samples = cache.get_or_compute(key, || {
                compute_count += 1;
                fake_render(phone)
            });
            audio.extend_from_slice(&samples);
            done += 1;
            sched.report_progress(id, done as f32 / input.len() as f32);
            progress_log.push((id, sched.progress(id).unwrap()));
        }
        rendered.push((input[0].position_ms as u64, audio));
        sched.complete(id);
    }

    // Re-requesting an already rendered phone must hit the cache (the
    // fake renderer is not invoked again).
    let key = Chunker::<PhoneInput>::hash_items(std::slice::from_ref(&chunks[0].items[0]));
    let _ = cache.get_or_compute(key, || {
        compute_count += 1;
        fake_render(&chunks[0].items[0])
    });

    // 10 distinct phones rendered; the duplicate request was a cache hit.
    assert_eq!(compute_count, 10);
    assert_eq!(rendered.len(), 4);
    assert_eq!(sched.pending(), 0);
    // Progress callbacks fired per job.
    assert!(!progress_log.is_empty());
    for (id, _) in &progress_log {
        assert!(ids.contains(id));
    }

    // 4. Mix: align by position_ms and sum.
    let inputs: Vec<MixInput> = rendered
        .iter()
        .map(|(pos, samples)| MixInput {
            chunk: AudioChunk {
                samples: samples.clone(),
                position_ms: *pos as f64,
                leading_ms: 0.0,
                hash: 0,
            },
            track: TrackSpec::new(0.0, 0.0),
        })
        .collect();
    let final_audio = runtime::mixer::mix(&inputs);
    assert_eq!(final_audio.sample_rate, SAMPLE_RATE);
    // Longest chunk ends at 900 ms + 100 samples.
    assert_eq!(final_audio.samples.len(), runtime::mixer::ms_to_samples(900.0, SAMPLE_RATE) + 100);
    // First chunk's first sample is nonzero.
    assert!(final_audio.samples[0] > 0.0);
    // The 900 ms region contains the last chunk's audio.
    let tail_start = runtime::mixer::ms_to_samples(900.0, SAMPLE_RATE);
    assert!(final_audio.samples[tail_start] > 0.0);
}
