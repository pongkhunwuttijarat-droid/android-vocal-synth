//! English CVVC phonemizer.
//!
//! Converts a phonetic hint (or a G2p dictionary lookup) into CV/CVVC
//! aliases by greedy pairing against the singer's oto table:
//!
//! * multi-phoneme aliases are matched first, longest first — the en_vccv
//!   bank provides pairs like `a t`, `i d`, `@ r`, `a l` and the 3-token
//!   `9 l l`;
//! * a word-final vowel falls back to the CVVC coda alias (`a-`), which
//!   carries the vowel into the next note;
//! * anything unmatched is kept verbatim (hint as-is).
//!
//! This mirrors the alias style of OpenUtau's `EnglishVCCVPhonemizer`
//! while staying a simple greedy table walk like
//! [`crate::JapaneseVcvPhonemizer`].

use domain::{UNote, UPhoneme};
use voicebank::Voicebank;

use crate::g2p::{parse_hint, G2p};
use crate::phonemizer::{
    is_rest, make_phoneme, positions_from_durations, split_duration, Phonemizer,
};

/// Vowels of the en_vccv symbol set (mirrors
/// `EnglishVCCVPhonemizer.vowels`), used for the CVVC coda rule when no
/// G2p dictionary is configured.
const VOWELS: &[&str] = &[
    "a", "@", "u", "0", "8", "I", "e", "3", "A", "i", "E", "O", "Q", "6", "o", "1ng", "9", "&",
    "x", "1", "Y", "L", "W", "8n", "Ang", "9l",
];

/// Longest multi-phoneme alias to attempt when pairing. 3 tokens covers
/// the en_vccv `9 l l` alias; CVVC banks rarely pair longer runs.
const MAX_ALIAS_TOKENS: usize = 3;

/// English CVVC phonemizer with greedy oto pairing.
pub struct EnglishCvvcPhonemizer {
    g2p: Option<Box<dyn G2p>>,
}

impl EnglishCvvcPhonemizer {
    /// A phonemizer that only uses phonetic hints (no dictionary).
    pub fn new() -> Self {
        EnglishCvvcPhonemizer { g2p: None }
    }

    /// A phonemizer that additionally queries `g2p` for lyrics without a
    /// phonetic hint.
    pub fn with_g2p(g2p: impl G2p + 'static) -> Self {
        EnglishCvvcPhonemizer {
            g2p: Some(Box::new(g2p)),
        }
    }

    /// Whether `symbol` is a vowel, per the configured g2p (falling back
    /// to the built-in en_vccv vowel set when no g2p is configured).
    fn is_vowel(&self, symbol: &str) -> bool {
        match &self.g2p {
            Some(g2p) => g2p.is_vowel(symbol),
            None => VOWELS.contains(&symbol),
        }
    }

    /// Greedy pairing: walk `tokens` left to right, consuming the longest
    /// multi-token alias found in the oto table at each position.
    ///
    /// Returns `(raw, alias)` pairs: `raw` is the pre-resolution token
    /// sequence, `alias` the resolved oto alias (equal to `raw` when no
    /// alias matched).
    fn pair(
        &self,
        tokens: &[String],
        singer: Option<&Voicebank>,
        tone: i32,
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let max = (tokens.len() - i).min(MAX_ALIAS_TOKENS);
            let mut matched = None;
            if let Some(vb) = singer {
                for w in (2..=max).rev() {
                    let candidate = tokens[i..i + w].join(" ");
                    if vb.lookup(&candidate, tone).is_some() {
                        matched = Some((candidate, w));
                        break;
                    }
                }
            }
            if let Some((candidate, w)) = matched {
                out.push((candidate.clone(), candidate));
                i += w;
                continue;
            }
            // Single token: a word-final vowel uses the CVVC coda alias
            // `v-` (it carries into the next note) when present.
            let token = tokens[i].clone();
            let mut alias = token.clone();
            if i == tokens.len() - 1 && self.is_vowel(&token) {
                if let Some(vb) = singer {
                    let coda = format!("{token}-");
                    if vb.lookup(&coda, tone).is_some() {
                        alias = coda;
                    }
                }
            }
            out.push((token, alias));
            i += 1;
        }
        out
    }
}

impl Default for EnglishCvvcPhonemizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Tokens for a note without a phonetic hint: the G2p query result, or
/// the lyric verbatim when the dictionary does not know it.
fn tokens_without_hint(g2p: Option<&dyn G2p>, lyric: &str) -> Vec<String> {
    if let Some(g2p) = g2p {
        if let Some(tokens) = g2p.query(lyric) {
            return tokens;
        }
    }
    vec![lyric.to_string()]
}

impl Phonemizer for EnglishCvvcPhonemizer {
    fn process(&self, notes: &[UNote], singer: Option<&Voicebank>) -> Vec<UPhoneme> {
        let mut out = Vec::new();
        for (nidx, note) in notes.iter().enumerate() {
            let (lyric, hint) = note.phonetic_hint();

            // A phonetic hint is authoritative: parse its tokens.
            let hinted: Option<Vec<String>> =
                hint.map(|h| parse_hint(&h)).filter(|t| !t.is_empty());

            if hinted.is_none() && is_rest(&lyric) {
                // Rest: single `R` phoneme (same as the Japanese phonemizer).
                let candidates = vec!["R".to_string(), "- R".to_string()];
                let mut ph = make_phoneme("R", 0, note.duration, 0, note.tone, nidx);
                resolve(&mut ph, singer, note.tone, &candidates);
                out.push(ph);
                continue;
            }

            let tokens = match hinted {
                Some(tokens) => tokens,
                None => tokens_without_hint(self.g2p.as_deref(), &lyric),
            };
            let aliases = self.pair(&tokens, singer, note.tone);
            let durations = split_duration(note.duration, aliases.len());
            let positions = positions_from_durations(&durations);
            for (i, ((raw, alias), (&pos, &dur))) in aliases
                .iter()
                .zip(positions.iter().zip(durations.iter()))
                .enumerate()
            {
                let mut ph = make_phoneme(alias.clone(), pos, dur, i as i32, note.tone, nidx);
                ph.raw_phoneme = raw.clone();
                out.push(ph);
            }
        }
        out
    }
}

/// Try each candidate alias against the singer's oto table (tone-mapped,
/// with plain fallback) and keep the first match in `ph.phoneme`.
fn resolve(ph: &mut UPhoneme, singer: Option<&Voicebank>, tone: i32, candidates: &[String]) {
    if let Some(vb) = singer {
        for candidate in candidates {
            if vb.lookup(candidate, tone).is_some() {
                ph.phoneme = candidate.clone();
                return;
            }
        }
    }
    ph.phoneme = ph.raw_phoneme.clone();
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Shared test fixtures: the real en_vccv bank built from its oto.ini
    //! files (parsed directly, no wav files involved).

    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use voicebank::{parse_character_yaml, parse_oto_ini, Subbank, Voicebank};

    /// Directory of the real en_vccv test bank (checked into the repo).
    pub(crate) fn en_vccv_dir() -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("ref(openutau+openutau mobile)")
            .join("desktop-ref/OpenUtau.Test/Files/en_vccv");
        assert!(dir.is_dir(), "en_vccv bank not found at {}", dir.display());
        dir
    }

    /// A `Voicebank` built from every oto.ini under the en_vccv bank with
    /// the subbanks of its character.yaml.
    pub(crate) fn en_vccv_voicebank() -> Voicebank {
        let dir = en_vccv_dir();
        let mut oto_map: HashMap<String, _> = HashMap::new();
        // Each subdirectory (CV, CVC_CV, ..., high) holds one oto.ini.
        let mut subdirs: Vec<PathBuf> = std::fs::read_dir(&dir)
            .expect("read en_vccv dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            let ini = sub.join("oto.ini");
            if !ini.is_file() {
                continue;
            }
            let bytes = std::fs::read(&ini).expect("read oto.ini");
            let parsed = parse_oto_ini(&bytes, None);
            for oto in parsed.otos {
                oto_map.entry(oto.alias.clone()).or_insert(oto);
            }
        }
        // character.yaml subbanks: `_H` for G4-B7, `_F3` for C1-F#4.
        let yaml_bytes = std::fs::read(dir.join("character.yaml")).expect("read character.yaml");
        let yaml_bytes = yaml_bytes
            .strip_prefix(&[0xEF, 0xBB, 0xBF])
            .unwrap_or(&yaml_bytes);
        let cfg = parse_character_yaml(yaml_bytes).expect("parse character.yaml");
        let subbanks: Vec<Subbank> = cfg
            .subbanks
            .iter()
            .map(|sb| {
                Subbank::from_ranges(
                    sb.color.clone().unwrap_or_default(),
                    sb.prefix.clone().unwrap_or_default(),
                    sb.suffix.clone().unwrap_or_default(),
                    sb.tone_ranges.clone(),
                )
            })
            .collect();
        Voicebank {
            name: "en_vccv".to_string(),
            base_path: dir.clone(),
            path: dir,
            id: "en_vccv".to_string(),
            image: None,
            portrait: None,
            portrait_opacity: 0.0,
            portrait_height: 0,
            author: None,
            voice: None,
            web: None,
            version: None,
            sample: None,
            default_phonemizer: None,
            other_info: String::new(),
            localized_names: HashMap::new(),
            text_file_encoding: encoding_rs::UTF_8,
            singer_type: String::new(),
            use_filename_as_alias: None,
            subbanks,
            oto_sets: Vec::new(),
            otos: Vec::new(),
            oto_map,
            warnings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g2p::{G2pDictionary, G2pFallbacks};

    fn note(lyric: &str) -> UNote {
        UNote {
            lyric: lyric.to_string(),
            duration: 480,
            tone: 60,
            ..Default::default()
        }
    }

    /// The alias sequence (resolved `phoneme` strings) for one note.
    fn aliases(lyric: &str, singer: Option<&Voicebank>) -> Vec<String> {
        EnglishCvvcPhonemizer::new()
            .process(&[note(lyric)], singer)
            .into_iter()
            .map(|ph| ph.phoneme)
            .collect()
    }

    #[test]
    fn fixture_loads_real_aliases() {
        let vb = fixtures::en_vccv_voicebank();
        for alias in ["a t", "i d", "@ r", "a l", "a-", "E d", "9 l l", "-h@"] {
            assert!(vb.oto_map.contains_key(alias), "missing alias {alias:?}");
        }
        // character.yaml subbanks: C4 falls in _F3 (no alias -> plain),
        // C5 falls in _H (tone-mapped alias exists).
        assert_eq!(vb.lookup("a r", 60).unwrap().alias, "a r");
        assert_eq!(vb.lookup("a r", 72).unwrap().alias, "a r_H");
    }

    #[test]
    fn hint_pairs_into_multi_phoneme_aliases() {
        let vb = fixtures::en_vccv_voicebank();
        assert_eq!(aliases("read[a r]", Some(&vb)), ["a r"]);
        assert_eq!(aliases("id[i d]", Some(&vb)), ["i d"]);
        assert_eq!(aliases("x[@ r]", Some(&vb)), ["@ r"]);
        assert_eq!(aliases("x[a l]", Some(&vb)), ["a l"]);
        assert_eq!(aliases("x[a t]", Some(&vb)), ["a t"]);
    }

    #[test]
    fn greedy_longest_alias_wins() {
        let vb = fixtures::en_vccv_voicebank();
        // `9 l l` exists as a 3-token alias (and `9 l` as a 2-token one):
        // greedy pairing must consume the longest match first.
        assert_eq!(aliases("x[9 l l]", Some(&vb)), ["9 l l"]);
    }

    #[test]
    fn pairing_mixed_with_verbatim_fallback() {
        let vb = fixtures::en_vccv_voicebank();
        // `E d` pairs; leading `r` has no alias in the bank and is kept
        // verbatim.
        assert_eq!(aliases("read[r E d]", Some(&vb)), ["r", "E d"]);
    }

    #[test]
    fn word_final_vowel_uses_coda_alias() {
        let vb = fixtures::en_vccv_voicebank();
        // CVVC coda: a word-final vowel resolves to `v-`.
        assert_eq!(aliases("x[a]", Some(&vb)), ["a-"]);
        assert_eq!(aliases("x[E]", Some(&vb)), ["E-"]);
    }

    #[test]
    fn unmatched_tokens_stay_verbatim() {
        let vb = fixtures::en_vccv_voicebank();
        // `z b` is not an alias and neither symbol is in the bank.
        assert_eq!(aliases("x[z b]", Some(&vb)), ["z", "b"]);
    }

    #[test]
    fn without_singer_tokens_pass_through() {
        // No oto table: the hint is kept verbatim, one phoneme per token.
        assert_eq!(aliases("read[a r]", None), ["a", "r"]);
        assert_eq!(aliases("x[9 l l]", None), ["9", "l", "l"]);
    }

    #[test]
    fn g2p_lookup_supplies_tokens() {
        let mut dict = G2pDictionary::new();
        dict.add_symbol("r", false, true)
            .add_symbol("E", true, false)
            .add_symbol("d", false, false);
        dict.add_entry("read", &["r", "E", "d"]);
        let g2p = G2pFallbacks::new(vec![Box::new(dict)]);

        // With the oto table: `E d` pairs, `r` stays verbatim.
        let vb = fixtures::en_vccv_voicebank();
        let phs = EnglishCvvcPhonemizer::with_g2p(g2p).process(&[note("read")], Some(&vb));
        let names: Vec<&str> = phs.iter().map(|ph| ph.phoneme.as_str()).collect();
        assert_eq!(names, ["r", "E d"]);

        // Unknown lyric falls back to the lyric verbatim.
        let mut dict = G2pDictionary::new();
        dict.add_symbol("r", false, true);
        dict.add_entry("read", &["r"]);
        let phs = EnglishCvvcPhonemizer::with_g2p(G2pFallbacks::new(vec![Box::new(dict)]))
            .process(&[note("xyz")], None);
        let names: Vec<&str> = phs.iter().map(|ph| ph.phoneme.as_str()).collect();
        assert_eq!(names, ["xyz"]);
    }

    #[test]
    fn rest_note_emits_r() {
        assert_eq!(aliases("-", None), ["R"]);
        assert_eq!(aliases("R", None), ["R"]);
    }

    #[test]
    fn note_duration_split_across_aliases() {
        let vb = fixtures::en_vccv_voicebank();
        let phs = EnglishCvvcPhonemizer::new().process(&[note("x[a t d]")], Some(&vb));
        assert_eq!(phs.len(), 2);
        // "a t" at note-relative 0..240, "d" at 240..480.
        assert_eq!((phs[0].position, phs[0].duration), (0, 240));
        assert_eq!((phs[1].position, phs[1].duration), (240, 240));
        assert_eq!(phs[0].raw_phoneme, "a t");
        assert_eq!(phs[0].phoneme, "a t");
        assert_eq!(phs[1].raw_phoneme, "d");
        assert_eq!(phs[1].phoneme, "d");
        assert_eq!(phs[0].parent, Some(0));
        assert_eq!(phs[1].index, 1);
    }

    #[test]
    fn multiple_notes_keep_parent_indices() {
        let vb = fixtures::en_vccv_voicebank();
        let notes = [note("x[a r]"), note("x[i d]")];
        let phs = EnglishCvvcPhonemizer::new().process(&notes, Some(&vb));
        assert_eq!(phs.len(), 2);
        assert_eq!(phs[0].phoneme, "a r");
        assert_eq!(phs[0].parent, Some(0));
        assert_eq!(phs[1].phoneme, "i d");
        assert_eq!(phs[1].parent, Some(1));
    }
}
