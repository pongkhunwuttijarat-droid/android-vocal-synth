//! Phrase grouping — a faithful port of `RenderPhrase.FromPart`.
//!
//! `OpenUtau.Core/Render/RenderPhrase.cs` splits a voice part's derived
//! phonemes into consecutive phrases: a new phrase starts exactly when
//! `phonemes[i - 1].End != phonemes[i].position` (any gap *or* overlap
//! between two phonemes breaks the phrase). Phonemes with a render error
//! are skipped in the reference (`Where(phoneme => !phoneme.Error)`); the
//! domain `UPhoneme` has no error flag, so grouping operates on whatever
//! phonemes it is given — the caller filters first if needed.

use domain::UPhoneme;

/// One phrase: a consecutive run of phonemes with no gap between them.
///
/// Timing fields mirror `RenderPhrase` (`positionMs` / `durationMs` /
/// `leadingMs`): all three are project-absolute milliseconds, with
/// `leading_ms` being the first phoneme's preutter.
#[derive(Debug, Clone, PartialEq)]
pub struct PhraseGroup {
    /// The phrase's phonemes, in part order. Adjacent phonemes satisfy
    /// `phonemes[i - 1].end() == phonemes[i].position`.
    pub phonemes: Vec<UPhoneme>,
    /// Project-absolute ms of the first phoneme's start.
    pub position_ms: f64,
    /// ms from the first phoneme's start to the last phoneme's end.
    pub duration_ms: f64,
    /// Leading (preutter) ms of the first phoneme.
    pub leading_ms: f64,
}

impl PhraseGroup {
    fn from_phonemes(phonemes: Vec<UPhoneme>) -> Self {
        let first = phonemes.first().expect("phrase group is non-empty");
        let last = phonemes.last().expect("phrase group is non-empty");
        let position_ms = first.position_ms;
        let end_ms = last.position_ms + last.duration_ms;
        PhraseGroup {
            position_ms,
            duration_ms: end_ms - position_ms,
            leading_ms: first.leading_ms,
            phonemes,
        }
    }
}

/// Groups a voice part's phonemes into phrases (`RenderPhrase.FromPart`).
pub struct PhraseGrouping;

impl PhraseGrouping {
    /// Split `phonemes` into phrases. A new phrase starts when
    /// `phonemes[i - 1].end() != phonemes[i].position`.
    ///
    /// The input is a slice of the part's derived phonemes (project
    /// order); the returned groups own clones of their phonemes.
    /// An empty input yields no phrases.
    pub fn group(phonemes: &[UPhoneme]) -> Vec<PhraseGroup> {
        let mut phrases: Vec<PhraseGroup> = Vec::new();
        let mut current: Vec<UPhoneme> = Vec::new();
        for (i, phoneme) in phonemes.iter().enumerate() {
            if i > 0 && phonemes[i - 1].end() != phoneme.position {
                phrases.push(PhraseGroup::from_phonemes(std::mem::take(&mut current)));
            }
            current.push(phoneme.clone());
        }
        if !current.is_empty() {
            phrases.push(PhraseGroup::from_phonemes(current));
        }
        phrases
    }

    /// Convenience over [`group`](Self::group) for a whole voice part.
    /// `phonemes` must be the part's derived phonemes.
    pub fn from_part(part: &domain::UVoicePart, phonemes: &[UPhoneme]) -> Vec<PhraseGroup> {
        let _ = part;
        Self::group(phonemes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phoneme(position: i32, duration: i32, leading_ms: f64) -> UPhoneme {
        UPhoneme {
            position,
            raw_position: position,
            duration,
            position_ms: position as f64 * 100.0 / 480.0 * 500.0 / 100.0,
            duration_ms: duration as f64 * 100.0 / 480.0 * 500.0 / 100.0,
            leading_ms,
            ..Default::default()
        }
    }

    #[test]
    fn empty_input_yields_no_phrases() {
        assert!(PhraseGrouping::group(&[]).is_empty());
    }

    #[test]
    fn adjacent_phonemes_form_one_phrase() {
        let phs = [
            phoneme(0, 480, 0.0),
            phoneme(480, 480, 0.0),
            phoneme(960, 480, 0.0),
        ];
        let phrases = PhraseGrouping::group(&phs);
        assert_eq!(phrases.len(), 1);
        assert_eq!(phrases[0].phonemes.len(), 3);
        assert_eq!(phrases[0].position_ms, 0.0);
        assert_eq!(phrases[0].duration_ms, 1500.0); // 1440 ticks = 1500 ms @120bpm
    }

    #[test]
    fn gap_starts_a_new_phrase() {
        // 0..480, 480..960 adjacent; 1440..1920 leaves a 480-tick gap.
        let phs = [
            phoneme(0, 480, 0.0),
            phoneme(480, 480, 12.0),
            phoneme(1440, 480, 34.0),
            phoneme(1920, 480, 0.0),
        ];
        let phrases = PhraseGrouping::group(&phs);
        assert_eq!(phrases.len(), 2);
        assert_eq!(phrases[0].phonemes.len(), 2);
        assert_eq!(phrases[1].phonemes.len(), 2);
        // Phrase-level ms mirror RenderPhrase: first phoneme start, span,
        // and first phoneme leading.
        assert_eq!(phrases[0].position_ms, 0.0);
        assert_eq!(phrases[0].duration_ms, 1000.0);
        assert_eq!(phrases[0].leading_ms, 0.0);
        assert_eq!(phrases[1].position_ms, 1500.0);
        assert_eq!(phrases[1].duration_ms, 1000.0);
        assert_eq!(phrases[1].leading_ms, 34.0);
    }

    #[test]
    fn overlap_also_starts_a_new_phrase() {
        // RenderPhrase breaks on any End != position, including overlap.
        let phs = [phoneme(0, 480, 0.0), phoneme(460, 480, 0.0)];
        let phrases = PhraseGrouping::group(&phs);
        assert_eq!(phrases.len(), 2);
    }

    #[test]
    fn single_phoneme_is_a_phrase() {
        let phs = [phoneme(0, 480, 5.0)];
        let phrases = PhraseGrouping::group(&phs);
        assert_eq!(phrases.len(), 1);
        assert_eq!(phrases[0].leading_ms, 5.0);
    }
}
