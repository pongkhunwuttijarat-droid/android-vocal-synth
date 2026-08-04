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

use feed::render_input::{RenderNote, RenderPhoneme};

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

impl Chunk {
    /// Cache key: hash of the phonemes in the RENDERED range (own +
    /// context). Editing any note changes the chunks that own it and the
    /// chunks that use it as context.
    pub fn hash(&self, phonemes: &[RenderPhoneme]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.ctx_range.len() as u64).hash(&mut h);
        for ph in &phonemes[self.ctx_range.clone()] {
            ph.phoneme.hash(&mut h);
            ph.position_ms.to_bits().hash(&mut h);
            ph.duration_ms.to_bits().hash(&mut h);
            ph.tone.hash(&mut h);
            ph.tone_shift.hash(&mut h);
        }
        h.finish()
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
