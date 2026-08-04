//! Oto mapping: phoneme + tone + subbank → oto entry (feed-data-flow.md §1).
//!
//! Uses the voicebank crate's tone-mapped lookup (`prefix.map` / subbank
//! prefixes and suffixes), mirroring `ClassicSinger.TryGetMappedOto` and
//! the `RenderPhone` oto resolution.

use voicebank::Voicebank;

use crate::render_input::OtoEntry;

/// Maps a phoneme to its oto entry at `tone`, producing the entry data a
/// sample-based renderer needs. Returns `None` when the voicebank has no
/// alias for the phoneme (the phoneme stays verbatim and unmapped).
pub struct OtoMapper;

impl OtoMapper {
    /// Look up `phoneme` at `tone`, optionally restricted to the subbank of
    /// `color` (voice color expression). The returned entry carries the
    /// mapped alias, the wav file name and its absolute path.
    pub fn map(vb: &Voicebank, phoneme: &str, tone: i32, color: Option<&str>) -> Option<OtoEntry> {
        let oto = match color {
            Some(color) if !color.is_empty() => vb.lookup_with_color(phoneme, tone, color),
            _ => vb.lookup(phoneme, tone),
        }?;
        Some(OtoEntry {
            alias: oto.alias.clone(),
            file: oto.wav.clone(),
            wav_path: vb.wav_path(oto).to_string_lossy().into_owned(),
            offset: oto.offset,
            consonant: oto.consonant,
            cutoff: oto.cutoff,
            preutter: oto.preutter,
            overlap: oto.overlap,
            envelope: Vec::new(),
            flags: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn mock_voicebank() -> Voicebank {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-data/mock-voicebank");
        assert!(dir.is_dir(), "mock voicebank not found at {}", dir.display());
        voicebank::load_voicebank(&dir).expect("load mock voicebank")
    }

    #[test]
    fn maps_aliases_present_in_the_mock_bank() {
        let vb = mock_voicebank();
        let e = OtoMapper::map(&vb, "3", 60, None).expect("alias 3");
        assert_eq!(e.alias, "3");
        assert_eq!(e.file, "_3_h3_3-.wav");
        assert!(e.wav_path.ends_with("voice/_3_h3_3-.wav"));
        assert_eq!(e.offset, 943.992);
        assert_eq!(e.consonant, 50.0);
        assert_eq!(e.cutoff, -1227.369);
        assert_eq!(e.preutter, 20.0);
        assert_eq!(e.overlap, 40.0);
    }

    #[test]
    fn maps_multi_phoneme_aliases() {
        let vb = mock_voicebank();
        let e = OtoMapper::map(&vb, "3 h3", 60, None).expect("alias 3 h3");
        assert_eq!(e.alias, "3 h3");
        assert_eq!(e.preutter, 250.0);
        assert_eq!(e.overlap, 83.333);
        assert_eq!(e.file, "_3_h3_3-.wav");
        // Vowel aliases of the other two mock wavs.
        let a = OtoMapper::map(&vb, "A", 60, None).expect("alias A");
        assert_eq!(a.file, "_a+_ha+_a+_a+_a+-.wav");
        let ai = OtoMapper::map(&vb, "aI", 60, None).expect("alias aI");
        assert_eq!(ai.file, "_ai+_hai+_ai+-.wav");
    }

    #[test]
    fn unknown_phoneme_returns_none() {
        let vb = mock_voicebank();
        assert!(OtoMapper::map(&vb, "zz", 60, None).is_none());
        assert!(OtoMapper::map(&vb, "r", 60, None).is_none());
    }

    #[test]
    fn tone_does_not_change_mapping_without_subbanks() {
        // The mock bank has no prefix.map: every tone maps the same alias.
        let vb = mock_voicebank();
        assert_eq!(OtoMapper::map(&vb, "3", 48, None).unwrap().alias, "3");
        assert_eq!(OtoMapper::map(&vb, "3", 96, None).unwrap().alias, "3");
    }
}
