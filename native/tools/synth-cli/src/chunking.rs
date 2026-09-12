//! Time-based chunk planning for incremental re-render.
//!
//! A phrase is split into chunks of ~`target_ms` so editing one note only
//! re-renders the chunks that contain it (instead of the whole phrase —
//! the scale demo is a single phrase, ~8s, re-rendered on any edit).
//!
//! Rules (agreed with the user):
//! - **Note = atomic** — a chunk boundary never cuts inside a note, so a
//!   word's phonemes (e.g. "parallel[p A r3 A l e l]") never split.
//! - **Time-based grouping** — accumulate whole notes until ~`target_ms`.
//! - **Long note = its own chunk** (a 2s "parallel" note is one chunk).
//! - **Rest = natural split** — chunks never span a rest (gap).
//! - **Context** — each chunk renders with `context_notes` neighbour notes
//!   on each side so the boundary phonemes keep their crossfade; the
//!   rendered audio is trimmed back to the chunk's own span.
//! - **Hash includes context** — editing a note invalidates chunks that
//!   own it AND chunks that use it as context (safe, slightly more work).

use std::ops::Range;

use feed::render_input::{RenderInput, RenderNote, RenderPhoneme};

/// One renderable chunk: the phoneme range to render (`ctx_range`,
/// includes context) and the audio span to keep (`start_ms..end_ms`).
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Phoneme range to feed the renderer (own + context).
    pub ctx_range: Range<usize>,
    /// Chunk's own phoneme range (context excluded) — used for the
    /// cache key and for the keep-span.
    pub own_range: Range<usize>,
    /// Keep-span in ms (relative to the project start).
    pub start_ms: f64,
    pub end_ms: f64,
}


/// Hash the `window` slice of `values`, clamped to the array.
///
/// Clamping matters: a chunk's time window can extend past the array the
/// builder produced, and a naive `values[lo..hi]` then yields an EMPTY slice —
/// which would silently make the cache key blind to edits in that curve. For a
/// non-empty array this always hashes at least one element.
fn hash_window<T: std::hash::Hash>(h: &mut impl std::hash::Hasher, values: &[T], window: &Range<usize>) {
    use std::hash::Hash as _;
    (values.len() as u64).hash(h);
    if values.is_empty() {
        return;
    }
    let last = values.len() - 1;
    let lo = window.start.min(last);
    let hi = window.end.clamp(lo + 1, values.len());
    for v in &values[lo..hi] {
        v.hash(h);
    }
}

/// Same, for `f32` (hashed by bit pattern so the key is deterministic).
fn hash_window_f32(h: &mut impl std::hash::Hasher, values: &[f32], window: &Range<usize>) {
    use std::hash::Hash as _;
    (values.len() as u64).hash(h);
    if values.is_empty() {
        return;
    }
    let last = values.len() - 1;
    let lo = window.start.min(last);
    let hi = window.end.clamp(lo + 1, values.len());
    for v in &values[lo..hi] {
        v.to_bits().hash(h);
    }
}

impl Chunk {
    /// Cache key for the rendered audio of this chunk.
    ///
    /// Covers **everything that changes the PCM**: the engine salt, the phrase
    /// frame (curve-sampling origin and interval), every field of the rendered
    /// phonemes (own + context), the oto entries behind them, and the pitch /
    /// expression-curve window the chunk samples.
    ///
    /// The curve/pitch part matters: DYN/GENC/TENC/BREC/VOIC are sampled into
    /// `input.curves` and PITD into `input.pitches_cents`, and neither appears
    /// in the phoneme records. Hashing only the phonemes (as this did before)
    /// serves **stale PCM after any curve or pitch edit** — the edit changes
    /// the audio but not the key. `runtime::feed_hash` already had complete
    /// `HashKey` impls for these types; this is the key actually used by the
    /// render pipeline and by the wasm engine's chunk cache.
    pub fn hash(&self, input: &RenderInput, engine_salt: u64) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        engine_salt.hash(&mut h);

        // Phrase frame: the curve origin is `position_ms - leading_ms` and the
        // grid step derives from `leading_ticks`/`time_axis_hint`.
        input.phrase.position_ms.to_bits().hash(&mut h);
        input.phrase.duration_ms.to_bits().hash(&mut h);
        input.phrase.leading_ms.to_bits().hash(&mut h);
        input.phrase.leading_ticks.hash(&mut h);
        input.phrase.time_axis_hint.hash(&mut h);

        // Rendered phonemes (own + context): every field that reaches the
        // renderer. `leading_ms`/`overlap_ms` were missing before and they
        // shift the timing.
        let phonemes = &input.phonemes[self.ctx_range.clone()];
        (phonemes.len() as u64).hash(&mut h);
        for ph in phonemes {
            ph.phoneme.hash(&mut h);
            ph.position_ms.to_bits().hash(&mut h);
            ph.duration_ms.to_bits().hash(&mut h);
            ph.leading_ms.to_bits().hash(&mut h);
            ph.overlap_ms.to_bits().hash(&mut h);
            ph.tone.hash(&mut h);
            ph.tone_shift.hash(&mut h);
            ph.index.hash(&mut h);
            ph.parent_note.hash(&mut h);
        }

        // The oto entries behind those phonemes (offset/cutoff/envelope/flags
        // all change the rendered audio).
        if let Some(sb) = input.sample_based.as_ref() {
            if let Some(otos) = sb.oto.get(self.ctx_range.clone()) {
                (otos.len() as u64).hash(&mut h);
                for oto in otos {
                    oto.alias.hash(&mut h);
                    oto.wav_path.hash(&mut h);
                    oto.offset.to_bits().hash(&mut h);
                    oto.consonant.to_bits().hash(&mut h);
                    oto.cutoff.to_bits().hash(&mut h);
                    oto.preutter.to_bits().hash(&mut h);
                    oto.overlap.to_bits().hash(&mut h);
                    oto.flags.hash(&mut h);
                    (oto.envelope.len() as u64).hash(&mut h);
                    for p in &oto.envelope {
                        p.x_ms.to_bits().hash(&mut h);
                        p.y.to_bits().hash(&mut h);
                    }
                }
            }
        }

        // Pitch + expression curves over the window this chunk samples.
        let window = self.curve_window(input);
        hash_window(&mut h, &input.pitches_cents, &window);
        hash_window_f32(&mut h, &input.curves.dynamics, &window);
        hash_window_f32(&mut h, &input.curves.gender, &window);
        hash_window_f32(&mut h, &input.curves.breathiness, &window);
        hash_window_f32(&mut h, &input.curves.tension, &window);
        hash_window_f32(&mut h, &input.curves.voicing, &window);
        (input.curves.extra.len() as u64).hash(&mut h);
        for named in &input.curves.extra {
            named.abbr.hash(&mut h);
            hash_window_f32(&mut h, &named.values, &window);
        }

        h.finish()
    }

    /// Grid-index window this chunk samples, in the phrase's curve arrays.
    ///
    /// The arrays are stepped by [`worldline_plugin::ms_per_interval`] from the
    /// phrase origin (`position_ms - leading_ms`). The window spans the
    /// context range's time extent plus one interval of slack on each side,
    /// because the renderer interpolates between neighbouring grid points.
    pub fn curve_window(&self, input: &RenderInput) -> Range<usize> {
        let origin_ms = input.phrase.position_ms - input.phrase.leading_ms;
        let first = &input.phonemes[self.ctx_range.start];
        let last = &input.phonemes[self.ctx_range.end - 1];
        let start_ms = first.position_ms - first.leading_ms;
        let end_ms = last.position_ms + last.duration_ms;
        let interval = worldline_plugin::ms_per_interval(input).max(1e-9);
        let lo = (((start_ms - origin_ms) / interval).floor() as i64 - 1).max(0) as usize;
        let hi = ((((end_ms - origin_ms) / interval).ceil() as i64) + 2).max(0) as usize;
        lo..hi.max(lo)
    }
}

/// Group `phonemes` (already phrase-ordered) into time-based chunks.
///
/// `phonemes` must carry `parent_note` indices into `notes` (as produced
/// by the phonemizer + timing). `target_ms` is the desired chunk duration.
pub fn plan_chunks(
    phonemes: &[RenderPhoneme],
    notes: &[RenderNote],
    target_ms: f64,
    context_notes: usize,
) -> Vec<Chunk> {
    if phonemes.is_empty() {
        return Vec::new();
    }

    // Note boundaries (note index -> first phoneme index).
    let mut note_first: Vec<usize> = Vec::new();
    for (i, ph) in phonemes.iter().enumerate() {
        while note_first.len() <= ph.parent_note {
            note_first.push(i);
        }
    }

    // Split points: ONLY rest boundaries (gap between notes in ms) plus
    // the phrase start/end. Note boundaries are handled inside the
    // grouping loop (note-atomic accumulation) — otherwise a phoneme-level
    // project (1 phoneme per note) would produce one chunk per note.
    let note_count = notes.len();
    let mut split_at: Vec<usize> = vec![0];
    for n in 0..note_count {
        // Rest after note n? (next note starts later than this note ends)
        if let Some(next) = notes.get(n + 1) {
            let end_ms = notes[n].position_ms + notes[n].duration_ms;
            if next.position_ms > end_ms {
                let next_first = note_first.get(n + 1).copied().unwrap_or(phonemes.len());
                if next_first > split_at[split_at.len() - 1] {
                    split_at.push(next_first);
                }
            }
        }
    }
    if split_at[split_at.len() - 1] != phonemes.len() {
        split_at.push(phonemes.len());
    }

    // Group segments (between split points) into time-budgeted chunks.
    // Note-atomic: a chunk boundary only falls between notes; a note that
    // alone exceeds the target becomes its own chunk (never split).
    let mut chunks: Vec<Chunk> = Vec::new();
    for w in split_at.windows(2) {
        let seg = w[0]..w[1];
        if seg.start == seg.end {
            continue;
        }
        let mut chunk_start = seg.start;
        let mut acc_ms = 0.0;
        let mut i = seg.start;
        while i < seg.end {
            let note = phonemes[i].parent_note;
            // Note duration = span of its phonemes (usually notes[n].duration_ms).
            let note_dur: f64 = phonemes[i..seg.end]
                .iter()
                .take_while(|p| p.parent_note == note)
                .map(|p| p.duration_ms)
                .sum();
            let note_is_new = i > seg.start && phonemes[i - 1].parent_note != note;
            let fits = acc_ms + note_dur <= target_ms;
            if acc_ms > 0.0 && (note_is_new && !fits) {
                // Next whole note would exceed the budget — close this chunk.
                chunks.push(make_chunk(phonemes, chunk_start, i, context_notes));
                chunk_start = i;
                acc_ms = 0.0;
            }
            acc_ms += note_dur;
            i += phonemes[i..seg.end]
                .iter()
                .take_while(|p| p.parent_note == note)
                .count();
        }
        if chunk_start < seg.end {
            chunks.push(make_chunk(phonemes, chunk_start, seg.end, context_notes));
        }
    }
    chunks
}

/// Build one chunk for own phoneme range `own_start..own_end` with
/// `context_notes` whole neighbour notes on each side.
fn make_chunk(
    phonemes: &[RenderPhoneme],
    own_start: usize,
    own_end: usize,
    context_notes: usize,
) -> Chunk {
    let first_parent = phonemes[own_start].parent_note;
    let last_parent = phonemes[own_end - 1].parent_note;

    // Context before: walk back over whole neighbour notes.
    let mut ctx_start = own_start;
    let mut seen = 0usize;
    let mut p = own_start;
    while p > 0 && seen < context_notes {
        let parent = phonemes[p - 1].parent_note;
        if parent >= first_parent {
            break; // same note as chunk start — context is BEFORE the note
        }
        let mut q = p - 1;
        while q > 0 && phonemes[q - 1].parent_note == parent {
            q -= 1;
        }
        seen += 1;
        ctx_start = q;
        if seen >= context_notes {
            break;
        }
        p = q;
    }

    // Context after: walk forward over whole neighbour notes.
    let mut ctx_end = own_end;
    let mut seen = 0usize;
    let mut p = own_end;
    while p < phonemes.len() && seen < context_notes {
        let parent = phonemes[p].parent_note;
        if parent <= last_parent {
            break;
        }
        let mut q = p;
        while q < phonemes.len() && phonemes[q].parent_note == parent {
            q += 1;
        }
        seen += 1;
        ctx_end = q;
        if seen >= context_notes {
            break;
        }
        p = q;
    }

    // Keep-span: own range's first phoneme start .. last phoneme end.
    let start_ms = phonemes[own_start].position_ms;
    let last = &phonemes[own_end - 1];
    let end_ms = last.position_ms + last.duration_ms;

    Chunk {
        ctx_range: ctx_start..ctx_end,
        own_range: own_start..own_end,
        start_ms,
        end_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ph(parent: usize, pos_ms: f64, dur_ms: f64) -> RenderPhoneme {
        RenderPhoneme {
            phoneme: "x".into(),
            position_ms: pos_ms,
            duration_ms: dur_ms,
            leading_ms: 0.0,
            overlap_ms: 0.0,
            tone: 60,
            tone_shift: 0,
            index: 0,
            parent_note: parent,
        }
    }

    fn note(position_ms: f64, duration_ms: f64) -> RenderNote {
        RenderNote {
            lyric: "la".into(),
            tone: 60,
            position_ms,
            duration_ms,
        }
    }

    #[test]
    fn long_note_is_own_chunk() {
        // One 2000ms note (parallel) + neighbours — must NOT split mid-note.
        let notes = vec![note(0.0, 500.0), note(500.0, 2000.0), note(2500.0, 500.0)];
        let phonemes = vec![
            ph(0, 0.0, 100.0),
            ph(1, 500.0, 500.0), // parallel: 4 × 500ms
            ph(1, 1000.0, 500.0),
            ph(1, 1500.0, 500.0),
            ph(1, 2000.0, 500.0),
            ph(2, 2500.0, 100.0),
        ];
        let chunks = plan_chunks(&phonemes, &notes, 1500.0, 1);
        // The 2000ms note must be intact in ONE chunk.
        let mut long_seen = false;
        for c in &chunks {
            let parents: Vec<usize> = phonemes[c.own_range.clone()]
                .iter()
                .map(|p| p.parent_note)
                .collect();
            if parents.first() == Some(&1) {
                assert_eq!(parents, vec![1, 1, 1, 1], "long note split: {chunks:?}");
                long_seen = true;
            }
        }
        assert!(long_seen, "long note chunk missing: {chunks:?}");
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn rest_splits_chunks() {
        // Note 1 ends at 500, note 2 starts at 1000 → rest between.
        let notes = vec![note(0.0, 500.0), note(1000.0, 500.0)];
        let phonemes = vec![ph(0, 0.0, 100.0), ph(1, 1000.0, 100.0)];
        let chunks = plan_chunks(&phonemes, &notes, 100000.0, 0);
        assert_eq!(chunks.len(), 2, "rest should split into 2 chunks: {chunks:?}");
    }

    /// Build a minimal RenderInput whose curve/pitch arrays are the only
    /// thing a test varies.
    fn input_with_curves(tension: f32, pitch: i32) -> feed::render_input::RenderInput {
        use feed::render_input::{Curves, PhraseInfo, RenderInput};
        // Grid step here is leading_ms/leading_ticks*5 ≈ 1.04 ms over a
        // ~1.1 s span, so the arrays need ~1100 entries — sized like the real
        // builder produces them.
        let n = 1200;
        RenderInput {
            phrase: PhraseInfo {
                position_ms: 0.0,
                duration_ms: 2000.0,
                leading_ms: 100.0,
                leading_ticks: 480,
                time_axis_hint: None,
            },
            notes: vec![note(0.0, 500.0), note(500.0, 500.0)],
            phonemes: vec![ph(0, 0.0, 500.0), ph(1, 500.0, 500.0)],
            pitches_cents: vec![pitch; n],
            curves: Curves {
                dynamics: vec![1.0; n],
                gender: vec![0.0; n],
                breathiness: vec![0.0; n],
                tension: vec![tension; n],
                voicing: vec![100.0; n],
                extra: Vec::new(),
            },
            sample_based: None,
            neural: None,
        }
    }

    fn one_chunk() -> Chunk {
        Chunk {
            ctx_range: 0..2,
            own_range: 0..2,
            start_ms: 0.0,
            end_ms: 1000.0,
        }
    }

    /// The cache key MUST change when an expression curve changes, otherwise
    /// the render cache (native `res-{hash}.bin` and the wasm chunk cache)
    /// serves stale PCM after a curve edit. Regression: the old key hashed
    /// only the phoneme records, which do not carry the curves.
    #[test]
    fn hash_changes_when_curves_change() {
        let chunk = one_chunk();
        let a = chunk.hash(&input_with_curves(0.0, 6000), 7);
        let b = chunk.hash(&input_with_curves(50.0, 6000), 7);
        assert_ne!(a, b, "tension edit did not change the chunk hash");
    }

    /// Same for the pitch grid (PITD is not in the phoneme records either).
    #[test]
    fn hash_changes_when_pitch_changes() {
        let chunk = one_chunk();
        let a = chunk.hash(&input_with_curves(0.0, 6000), 7);
        let b = chunk.hash(&input_with_curves(0.0, 6100), 7);
        assert_ne!(a, b, "pitch edit did not change the chunk hash");
    }

    /// The engine salt must still discriminate engines.
    #[test]
    fn hash_changes_with_engine_salt() {
        let chunk = one_chunk();
        let input = input_with_curves(0.0, 6000);
        assert_ne!(chunk.hash(&input, 1), chunk.hash(&input, 2));
    }

    /// ...and the same input must be stable (a cache is only useful then).
    #[test]
    fn hash_is_stable_for_identical_input() {
        let chunk = one_chunk();
        assert_eq!(
            chunk.hash(&input_with_curves(10.0, 6000), 7),
            chunk.hash(&input_with_curves(10.0, 6000), 7)
        );
    }

    #[test]
    fn context_included_in_ctx_range() {
        let notes = vec![note(0.0, 500.0), note(500.0, 500.0), note(1000.0, 500.0)];
        let phonemes = vec![
            ph(0, 0.0, 100.0),
            ph(1, 500.0, 100.0),
            ph(2, 1000.0, 100.0),
        ];
        let chunks = plan_chunks(&phonemes, &notes, 50.0, 1);
        // Middle chunk (owns note 1) must include note 0 and note 2 as ctx.
        let mid = chunks
            .iter()
            .find(|c| c.own_range == (1..2))
            .expect("middle chunk");
        assert_eq!(mid.ctx_range, 0..3, "context notes missing: {mid:?}");
    }
}
