//! OpenUtau `DefaultPhonemizer` parity: **one note -> one phoneme**.
//!
//! Mirrors `OpenUtau.Core/DefaultPhonemizer.cs` (the phonemizer OpenUtau picks
//! when a track has no phonemizer, i.e. `track.phonemizer == ''`):
//!
//! ```text
//! string alias = notes[0].lyric;
//! if (singer.TryGetMappedOto(notes[0].lyric + alt, notes[0].tone + shift, color, out var oto))
//!     alias = oto.Alias;
//! return new Result { phonemes = [ new Phoneme { phoneme = alias } ] };
//! ```
//!
//! So: the lyric is used **verbatim** (no G2P, no mora splitting, no VCV
//! bridging) as the single oto alias for that note, and the phoneme takes the
//! alias string the oto table reports.
//!
//! One deliberate difference, forced by our pipeline having no per-phoneme
//! error flag: OpenUtau marks a phoneme whose oto is missing as `Error` and
//! `RenderPhrase.FromPart` drops it *before* grouping, so the note's whole slot
//! becomes a gap while the other notes still render. If we emitted that phoneme,
//! our phrase-level gate (`sample_based.oto.len() != phonemes.len()`) would skip
//! the **entire phrase** instead. This phonemizer therefore does not emit a
//! phoneme for a note whose lyric has no oto alias, which reproduces the
//! reference's phrase boundaries and keeps the remaining notes renderable.
//! `DefaultPhonemizer::process_report` returns the dropped notes so hosts can
//! surface them instead of hiding the loss.
//!
//! Extender notes (lyric `+` or `+N`) belong to the preceding note in OpenUtau
//! (they arrive as `notes[1..]` of the same call and share its one phoneme), so
//! they extend the previous phoneme's duration instead of producing their own.

use domain::{UNote, UPhoneme};
use voicebank::Voicebank;

use crate::phonemizer::{make_phoneme, Phonemizer};

/// The OpenUtau default phonemizer (see the module docs).
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPhonemizer;

impl DefaultPhonemizer {
    /// Phonemes plus a human-readable reason for every note that was dropped
    /// because its lyric has no oto alias in the singer's table.
    pub fn process_report(
        &self,
        notes: &[UNote],
        singer: Option<&Voicebank>,
    ) -> (Vec<UPhoneme>, Vec<String>) {
        let mut out: Vec<UPhoneme> = Vec::with_capacity(notes.len());
        let mut dropped: Vec<String> = Vec::new();
        for (nidx, note) in notes.iter().enumerate() {
            // OpenUtau's UNote parses "[hint]" out of the lyric into a separate
            // phoneticHint field (UNote.cs:120-158), so notes[0].lyric in
            // DefaultPhonemizer is the lyric WITHOUT the hint. This phonemizer
            // does not consume the hint either (only the hint-aware English and
            // Universal paths do).
            let (lyric, _hint) = note.phonetic_hint();
            let lyric = lyric.trim().to_string();

            // Extender: same phoneme as the leading note, longer duration.
            if lyric.starts_with('+') {
                if let Some(prev) = out.last_mut() {
                    prev.duration += note.duration;
                }
                continue;
            }

            if lyric.is_empty() {
                dropped.push(format!("note {nidx} lyric {:?}: empty lyric", note.lyric));
                continue;
            }

            let alias = match singer {
                Some(vb) => match vb.lookup(&lyric, note.tone) {
                    Some(oto) => oto.alias.clone(),
                    None => {
                        dropped.push(format!("note {nidx} lyric {lyric:?}: no oto alias"));
                        continue;
                    }
                },
                // No singer: emit unresolved, like the other phonemizers do.
                None => lyric.clone(),
            };
            out.push(make_phoneme(alias, 0, note.duration, 0, note.tone, nidx));
        }
        (out, dropped)
    }
}

impl Phonemizer for DefaultPhonemizer {
    fn process(&self, notes: &[UNote], singer: Option<&Voicebank>) -> Vec<UPhoneme> {
        self.process_report(notes, singer).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(lyric: &str, duration: i32) -> UNote {
        let mut n = UNote::default();
        n.lyric = lyric.to_string();
        n.tone = 69;
        n.duration = duration;
        n
    }

    #[test]
    fn one_note_one_phoneme_without_a_singer() {
        let notes = vec![note("sa", 240), note("ku", 240)];
        let (phs, dropped) = DefaultPhonemizer.process_report(&notes, None);
        assert!(dropped.is_empty());
        assert_eq!(phs.len(), 2, "one phoneme per note");
        assert_eq!(phs[0].phoneme, "sa");
        assert_eq!(phs[0].raw_phoneme, "sa");
        assert_eq!(phs[0].position, 0);
        assert_eq!(phs[0].duration, 240);
        assert_eq!(phs[0].parent, Some(0));
        assert_eq!(phs[1].phoneme, "ku");
        assert_eq!(phs[1].parent, Some(1));
    }

    #[test]
    fn extender_notes_extend_the_previous_phoneme() {
        let notes = vec![note("sa", 240), note("+", 240), note("+1", 480)];
        let (phs, dropped) = DefaultPhonemizer.process_report(&notes, None);
        assert!(dropped.is_empty());
        assert_eq!(phs.len(), 1, "extenders belong to the leading note");
        assert_eq!(phs[0].duration, 240 + 240 + 480);
    }

    #[test]
    fn phonetic_hint_is_stripped_from_the_lyric() {
        // "sa [ku]" -> lyric "sa" (OpenUtau keeps the hint in a separate field).
        let notes = vec![note("sa [ku]", 240)];
        let (phs, dropped) = DefaultPhonemizer.process_report(&notes, None);
        assert!(dropped.is_empty());
        assert_eq!(phs.len(), 1);
        assert_eq!(
            phs[0].phoneme, "sa",
            "the [hint] must not be part of the alias"
        );
    }

    #[test]
    fn empty_lyric_is_dropped_not_emitted() {
        let notes = vec![note("", 240)];
        let (phs, dropped) = DefaultPhonemizer.process_report(&notes, None);
        assert!(phs.is_empty());
        assert_eq!(dropped.len(), 1);
    }
}
