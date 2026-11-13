//! Universal phonemizer (Task 5) — wires L1 (IPA + detect) and L2
//! (adapter + manifest) into the [`Phonemizer`] trait.
//!
//! Per-note path:
//!   * lyric with `[hint]`   → hint tokens (authoritative — identical to
//!     the existing English CVVC path; greedy oto pairing resolves them)
//!   * lyric without hint    → [`detect_language`] → IPA tokens (universal)
//!     → [`IpaAdapter`] maps them to bank aliases through the manifest;
//!     unknown phonemes fall back to the lyric verbatim (one phoneme).
//!   * rest lyrics (`R`, `-`) → single `R` phoneme (same as before).

use domain::{UNote, UPhoneme};
use voicebank::Voicebank;

use crate::adapter::IpaAdapter;
use crate::detect::{detect_language, Language};
use crate::g2p::{parse_hint, G2p};
use crate::ipa::IpaDictionary;
use crate::japanese::JapaneseVcvPhonemizer;
use crate::phonemizer::{
    is_rest, make_phoneme, positions_from_durations, split_duration, Phonemizer,
};

/// Universal phonemizer: IPA-first for unhinted lyrics, hint path kept
/// byte-identical to the English CVVC phonemizer.
pub struct UniversalPhonemizer<'a> {
    adapter: IpaAdapter<'a>,
    dict: IpaDictionary,
    word_dict: crate::g2p::G2pFallbacks,
}

impl<'a> UniversalPhonemizer<'a> {
    pub fn new(adapter: IpaAdapter<'a>) -> Self {
        UniversalPhonemizer {
            dict: IpaDictionary::en_subset(),
            word_dict: crate::rusted_utau_dict::rusted_utau_demo_g2p(),
            adapter,
        }
    }

    /// Can the bank render the lyric as a single oto alias?
    fn can_render_alias(&self, lyric: &str) -> bool {
        self.adapter.manifest().can_render(lyric)
    }

    /// Tokens for a note WITHOUT a phonetic hint.
    fn tokens_without_hint(&self, lyric: &str) -> Vec<String> {
        // Rest lyrics never reach here (handled earlier).
        let lang = detect_language(lyric);
        match lang {
            Language::Japanese => {
                // The English subset dict cannot map kana; keep the
                // lyric verbatim for now (VCV adapter is a later task —
                // the Japanese phonemizer still handles this path).
                vec![lyric.to_string()]
            }
            Language::English => {
                // 0. Direct oto alias: if the whole lyric IS a voicebank
                //    alias ("la" → oto has `la`), use it as ONE phoneme.
                //    Golden (C# OpenUtau) does exactly this — skipping
                //    this check split "la" into l+A and produced a
                //    DIFFERENT phoneme boundary than the reference
                //    (waveform correlation ~0.03).
                if self.can_render_alias(lyric) {
                    return vec![lyric.to_string()];
                }
                // 1. Word dictionary (Rusted UTAU demo vocabulary).
                if let Some(tokens) = self.word_dict.query(lyric) {
                    return tokens;
                }
                // 2. Letter-split fallback (mirrors the app's phoneme
                //    resolver): every letter that is a known ARPABET
                //    symbol becomes a phoneme token — "na" → n, a →
                //    IPA /n/ /a/ → adapter maps to n, A. Unresolvable
                //    letters are dropped; nothing resolvable → verbatim.
                let resolved: Vec<String> = lyric
                    .chars()
                    .filter(|c| c.is_ascii_alphabetic())
                    .map(|c| c.to_ascii_lowercase().to_string())
                    .filter(|t| self.dict.arpabet(t).is_some() || self.is_word_symbol(t))
                    .collect();
                if !resolved.is_empty() {
                    return resolved;
                }
                vec![lyric.to_string()]
            }
        }
    }

    /// A letter is usable when it is a known ARPABET symbol (vowel or
    /// consonant) in the IPA dict.
    fn is_word_symbol(&self, t: &str) -> bool {
        self.dict.arpabet(t).is_some()
    }
}

impl Phonemizer for UniversalPhonemizer<'_> {
    fn process(&self, notes: &[UNote], singer: Option<&Voicebank>) -> Vec<UPhoneme> {
        // Japanese (kana) lyrics → delegate the WHOLE part to the VCV
        // phonemizer: it carries a cross-note tail-vowel state (か → "a き"),
        // so per-note fallback would break the head-alias continuity.
        // This is the "ถ้าไม่ตรงภาษาก็เข้า universal" catch-all — kana on
        // any track (English-declared, unspecified, mixed) still renders.
        if notes
            .iter()
            .any(|n| detect_language(&n.lyric) == Language::Japanese)
        {
            return JapaneseVcvPhonemizer.process(notes, singer);
        }

        let mut out = Vec::new();
        for (nidx, note) in notes.iter().enumerate() {
            let (lyric, hint) = note.phonetic_hint();

            let hinted: Option<Vec<String>> =
                hint.map(|h| parse_hint(&h)).filter(|t| !t.is_empty());

            if hinted.is_none() && is_rest(&lyric) {
                // Rest: single `R` phoneme (same as the English path).
                let mut ph = make_phoneme("R", 0, note.duration, 0, note.tone, nidx);
                let candidates = vec!["R".to_string(), "- R".to_string()];
                resolve(&mut ph, singer, note.tone, &candidates);
                out.push(ph);
                continue;
            }

            let tokens = match hinted {
                Some(ref t) => t.clone(),
                None => self.tokens_without_hint(&lyric),
            };

            // Resolve tokens to aliases via the oto table (hint path) or
            // the IPA adapter (unhinted path).
            let aliases = if hinted.is_some() {
                self.pair_hint(&tokens, singer, note.tone)
            } else {
                self.adapter
                    .map_ipa(&tokens.iter().map(String::as_str).collect::<Vec<_>>())
            };
            let durations = split_duration(note.duration, aliases.len());
            let positions = positions_from_durations(&durations);
            for (i, (alias, (&pos, &dur))) in aliases
                .iter()
                .zip(positions.iter().zip(durations.iter()))
                .enumerate()
            {
                let mut ph = make_phoneme(alias.clone(), pos, dur, i as i32, note.tone, nidx);
                ph.raw_phoneme = alias.clone();
                out.push(ph);
            }
        }
        out
    }
}

impl UniversalPhonemizer<'_> {
    /// Greedy oto pairing for hint tokens (identical to
    /// `EnglishCvvcPhonemizer::pair`).
    fn pair_hint(&self, tokens: &[String], singer: Option<&Voicebank>, tone: i32) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let max = (tokens.len() - i).min(3);
            let mut matched = None;
            if let Some(vb) = singer {
                for w in (2..=max).rev() {
                    let candidate = tokens[i..i + w].join(" ");
                    if vb.lookup(&candidate, tone).is_some() {
                        matched = Some(candidate);
                        break;
                    }
                }
            }
            if let Some(candidate) = matched {
                let n_tokens = candidate.split_whitespace().count();
                out.push(candidate);
                i += n_tokens;
                continue;
            }
            out.push(tokens[i].clone());
            i += 1;
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
