//! Layered phoneme composition (T013) — the IME-style candidate engine.
//!
//! Turns a typed lyric/sound hint into a bounded, deterministic list of
//! pronunciation candidates, mirroring the layered design in
//! `docs/planning/phoneme-input-composition.md`:
//!
//! ```text
//! typed text / phonetic hint
//!        │ language-specific conversion (G2p / mora split)
//!        ▼
//! language phone sequence
//!        │ normalize through the universal (IPA) inventory
//!        ▼
//! universal phone sequence
//!        │ resolve against the selected voicebank's aliases
//!        ▼
//! voicebank-native alias sequence, tagged with the style it was built for
//! ```
//!
//! Candidates are produced per strategy in a fixed order (direct alias,
//! phonetic hint, language strategies, universal mapping) and sorted
//! resolved-first; every candidate records which layer produced each
//! resolved alias and lists phones the selected bank cannot render. A
//! committed choice is stored as [`NotePronunciation`] — the universal
//! phone sequence plus language, mode and style — so changing the selected
//! voicebank recomputes the derived aliases without changing the choice.
//!
//! This module is host-neutral: no HTTP, no session types. Wiring it into
//! `project-session` / `engine-wasm` / `synth-server` goes through a new
//! contract revision (CONTRACT-v1 §6 is frozen).

use serde::{Deserialize, Serialize};

use voicebank::manifest::PhonemeManifest;
use voicebank::Voicebank;

use crate::adapter::NearestPhoneme;
use crate::g2p::{parse_hint, G2p};
use crate::ipa::IpaDictionary;
use crate::japanese::{is_pure_vowel_mora, split_morae, tail_vowel};
use crate::phonemizer::is_rest;
use crate::translit::mora_to_english;

/// Maximum accepted composition input length (characters). Longer input is
/// truncated with a diagnostic so a single request stays bounded.
pub const MAX_INPUT_CHARS: usize = 256;

/// Candidate limit applied when the request omits `limit`.
pub const DEFAULT_LIMIT: usize = 8;

/// Hard cap for the candidate limit.
pub const MAX_LIMIT: usize = 16;

/// Language of the composition input. Extensible by adding a variant plus
/// a converter; the wire value is the lowercase id (`"ja"`, `"en"`, `"th"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "th")]
    Thai,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Japanese => "ja",
            Language::English => "en",
            Language::Thai => "th",
        }
    }

    /// Parse a language id; `None` for unknown ids (reported, never guessed).
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "ja" => Some(Language::Japanese),
            "en" => Some(Language::English),
            "th" => Some(Language::Thai),
            _ => None,
        }
    }
}

/// Output view the user edits: the universal phone sequence or the bank's
/// native alias sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MappingMode {
    #[default]
    #[serde(rename = "universal")]
    Universal,
    #[serde(rename = "voicebank_native")]
    VoicebankNative,
}

impl MappingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            MappingMode::Universal => "universal",
            MappingMode::VoicebankNative => "voicebank_native",
        }
    }
}

/// Bank-native sequence style. `Auto` lets the composer pick; any other
/// value restricts candidates to strategies built for that style. Styles
/// describe the alias sequence a strategy builds — they are never guessed
/// from a bank's display name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Style {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "cv")]
    Cv,
    #[serde(rename = "vcv")]
    Vcv,
    #[serde(rename = "cvvc")]
    Cvvc,
    #[serde(rename = "vccv")]
    Vccv,
}

impl Style {
    pub fn as_str(self) -> &'static str {
        match self {
            Style::Auto => "auto",
            Style::Cv => "cv",
            Style::Vcv => "vcv",
            Style::Cvvc => "cvvc",
            Style::Vccv => "vccv",
        }
    }
}

/// Which layer produced a candidate's resolved aliases (provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    /// The whole input is already a bank alias (`la`, `- sa`).
    #[serde(rename = "direct_alias")]
    DirectAlias,
    /// Tokens from a `[...]` phonetic hint, greedily paired.
    #[serde(rename = "phonetic_hint")]
    PhoneticHint,
    /// Japanese whole-lyric VCV alias (one alias per note).
    #[serde(rename = "japanese_vcv")]
    JapaneseVcv,
    /// Japanese mora split (CV heads + tail vowels).
    #[serde(rename = "mora_cv")]
    MoraCv,
    /// English G2p dictionary tokens, greedily paired (CVVC-style).
    #[serde(rename = "g2p_cvvc")]
    G2pCvvc,
    /// Thai orthographic conversion followed by Thai VCCV alias mapping.
    #[serde(rename = "thai_vccv")]
    ThaiVccv,
    /// Letter-split fallback (letters that are known symbols).
    #[serde(rename = "letter_fallback")]
    LetterFallback,
    /// Universal (IPA) mapping through the bank manifest.
    #[serde(rename = "universal_ipa")]
    UniversalIpa,
}

impl Strategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Strategy::DirectAlias => "direct_alias",
            Strategy::PhoneticHint => "phonetic_hint",
            Strategy::JapaneseVcv => "japanese_vcv",
            Strategy::MoraCv => "mora_cv",
            Strategy::G2pCvvc => "g2p_cvvc",
            Strategy::ThaiVccv => "thai_vccv",
            Strategy::LetterFallback => "letter_fallback",
            Strategy::UniversalIpa => "universal_ipa",
        }
    }
}

/// One pronunciation choice the user can pick from the candidate list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionCandidate {
    /// Deterministic for the request context; transient (never persisted).
    pub candidate_id: String,
    /// The spelling shown in the IME-style list.
    pub display_text: String,
    /// Language this candidate was converted from.
    pub language: Language,
    /// Resolved style — never [`Style::Auto`].
    pub style: Style,
    /// Which layer produced the resolved aliases.
    pub strategy: Strategy,
    /// Language-level phone sequence (morae, G2p tokens, hint tokens...).
    pub language_phones: Vec<String>,
    /// Universal (IPA where known) sequence derived from `language_phones`.
    /// Symbols the inventory does not know pass through verbatim.
    pub universal_phones: Vec<String>,
    /// Bank-native alias sequence (verbatim symbols when unresolved).
    pub voicebank_aliases: Vec<String>,
    /// Phones the selected voicebank cannot render, in order. Empty when
    /// everything resolved; with no voicebank attached every phone is
    /// listed (nothing could be verified).
    pub unresolved_phones: Vec<String>,
}

/// Result of one composition request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionResult {
    /// Deterministic id for the request context; transient.
    pub composition_id: String,
    /// The input the composition ran on (post-truncation).
    pub input: String,
    /// Resolved language (explicit request language or detection).
    pub language: Language,
    pub mode: MappingMode,
    /// The style restriction from the request (`Auto` when unrestricted).
    pub requested_style: Style,
    /// Voicebank id echoed from the request (host-assigned, informational).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voicebank_id: Option<String>,
    pub candidates: Vec<CompositionCandidate>,
    /// Explicit diagnostics: unsupported languages/styles, truncation,
    /// forced-language mismatch. Never a silent fallback.
    pub diagnostics: Vec<String>,
}

/// Request for [`PhonemeComposer::compose`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CompositionRequest {
    pub input: String,
    /// Explicit language id (`"ja"` / `"en"` / `"th"`); omit to auto-detect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    #[serde(default)]
    pub mode: MappingMode,
    #[serde(default)]
    pub style: Style,
    /// Max candidates (1..=[`MAX_LIMIT`]); default [`DEFAULT_LIMIT`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Tone context for alias lookups (subbank prefix/suffix mapping).
    #[serde(default)]
    pub tone: i32,
    /// Host-assigned voicebank id, echoed in the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voicebank_id: Option<String>,
}

/// How a universal phone mapped onto the selected voicebank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasResolutionKind {
    /// The bank renders the phone directly.
    Exact,
    /// IPA → alias bridge (dictionary) hit.
    Bridge,
    /// Closest supported phone substituted (`NearestPhoneme`).
    Nearest,
    /// Nothing close enough — the phone stays verbatim and unrenderable.
    Unresolved,
}

/// One universal phone's derived alias on a voicebank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasResolution {
    pub universal_phone: String,
    pub alias: String,
    pub kind: AliasResolutionKind,
}

/// The committed pronunciation of one note (the persistence boundary).
///
/// Only this struct may be stored on the authoritative project/session
/// model: the universal phone sequence plus language, mode and style. Bank
/// aliases are derived output — recomputed via
/// [`NotePronunciation::resolve_aliases`] whenever the selected voicebank
/// changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotePronunciation {
    pub language: Language,
    pub mode: MappingMode,
    /// Resolved style of the committed sequence (never [`Style::Auto`]).
    pub style: Style,
    /// Strategy needed to reproduce the same language/style mapping.
    pub strategy: Strategy,
    /// Language-level tokens chosen by the candidate.
    pub language_phones: Vec<String>,
    /// The committed universal phone sequence.
    pub universal_phones: Vec<String>,
}

impl NotePronunciation {
    /// Commit a candidate. `mode` records which view the user edits, but
    /// the stored sequence is always the universal one; the candidate must
    /// therefore have a non-empty universal sequence.
    pub fn from_candidate(candidate: &CompositionCandidate, mode: MappingMode) -> Option<Self> {
        if candidate.universal_phones.is_empty() {
            return None;
        }
        Some(NotePronunciation {
            language: candidate.language,
            mode,
            style: candidate.style,
            strategy: candidate.strategy,
            language_phones: candidate.language_phones.clone(),
            universal_phones: candidate.universal_phones.clone(),
        })
    }

    pub fn to_domain(&self) -> domain::UPronunciation {
        domain::UPronunciation {
            language: self.language.as_str().to_string(),
            mode: self.mode.as_str().to_string(),
            style: self.style.as_str().to_string(),
            strategy: self.strategy.as_str().to_string(),
            language_phones: self.language_phones.clone(),
            universal_phones: self.universal_phones.clone(),
        }
    }

    /// Recompute bank aliases for the committed universal phones against a
    /// (possibly changed) voicebank. Unknown phones stay verbatim and are
    /// reported as [`AliasResolutionKind::Unresolved`]; without a bank
    /// every phone is unresolved.
    pub fn resolve_aliases(
        &self,
        voicebank: Option<&Voicebank>,
        tone: i32,
    ) -> Vec<AliasResolution> {
        let Some(vb) = voicebank else {
            return self
                .universal_phones
                .iter()
                .map(|p| AliasResolution {
                    universal_phone: p.clone(),
                    alias: p.clone(),
                    kind: AliasResolutionKind::Unresolved,
                })
                .collect();
        };
        let manifest = PhonemeManifest::from_voicebank(vb);
        let nearest = NearestPhoneme::new(&manifest);
        let dict = IpaDictionary::en_subset();
        self.universal_phones
            .iter()
            .map(|phone| {
                let alias = dict
                    .arpabet(phone)
                    .unwrap_or(phone.trim_matches('/'))
                    .to_string();
                if let Some(oto) = vb.lookup(&alias, tone) {
                    return AliasResolution {
                        universal_phone: phone.clone(),
                        alias: oto.alias.clone(),
                        kind: AliasResolutionKind::Exact,
                    };
                }
                if phone != &alias {
                    if let Some(oto) = vb.lookup(phone, tone) {
                        return AliasResolution {
                            universal_phone: phone.clone(),
                            alias: oto.alias.clone(),
                            kind: AliasResolutionKind::Bridge,
                        };
                    }
                }
                if let Some(n) = nearest.nearest(&alias) {
                    return AliasResolution {
                        universal_phone: phone.clone(),
                        alias: n,
                        kind: AliasResolutionKind::Nearest,
                    };
                }
                AliasResolution {
                    universal_phone: phone.clone(),
                    alias,
                    kind: AliasResolutionKind::Unresolved,
                }
            })
            .collect()
    }
}

/// The IME-style composition engine. Deterministic: identical request +
/// voicebank produce identical candidates (fixed strategy order, no
/// hash-map iteration in output paths).
pub struct PhonemeComposer {
    word_dict: crate::g2p::G2pFallbacks,
    ipa: IpaDictionary,
}

impl Default for PhonemeComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl PhonemeComposer {
    /// Composer with the built-in Rusted UTAU demo G2p dictionary.
    pub fn new() -> Self {
        PhonemeComposer {
            word_dict: crate::rusted_utau_dict::rusted_utau_demo_g2p(),
            ipa: IpaDictionary::en_subset(),
        }
    }

    /// Composer with a host-supplied G2p dictionary (a real per-language
    /// dictionary later; the demo vocabulary is only a fixture).
    pub fn with_g2p(word_dict: crate::g2p::G2pFallbacks) -> Self {
        PhonemeComposer {
            word_dict,
            ipa: IpaDictionary::en_subset(),
        }
    }

    /// Compose candidates for `request` against `voicebank` (optional —
    /// without a bank every candidate is reported unresolved).
    pub fn compose(
        &self,
        request: &CompositionRequest,
        voicebank: Option<&Voicebank>,
    ) -> CompositionResult {
        let mut diagnostics = Vec::new();

        // 1. Normalize + bound the input.
        let input: String = request.input.chars().take(MAX_INPUT_CHARS).collect();
        if request.input.chars().count() > MAX_INPUT_CHARS {
            diagnostics.push(format!("input truncated to {MAX_INPUT_CHARS} characters"));
        }

        // 2. Resolve the language: explicit request wins, detection fills in.
        let detected = detect_language(&input);
        let language = match request.language {
            Some(l) => {
                if l != detected && detected != Language::English {
                    diagnostics.push(format!(
                        "input looks like {} but language is forced to {}",
                        detected.as_str(),
                        l.as_str()
                    ));
                }
                l
            }
            None => detected,
        };

        let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let composition_id = composition_id(&input, language, request);

        let mut candidates = match language {
            _ => self.candidates_for(&input, language, request.tone, voicebank),
        };

        // 3. Style restriction: drop candidates not built for the style.
        if request.style != Style::Auto {
            let before = candidates.len();
            candidates.retain(|c| c.style == request.style);
            if candidates.is_empty() && before > 0 {
                diagnostics.push(format!(
                    "no {} candidates for this input{}",
                    request.style.as_str(),
                    match voicebank {
                        Some(_) => " in the selected voicebank",
                        None => "",
                    }
                ));
            }
        }

        // 4. Resolved-first stable sort (ties keep strategy order), then bound.
        candidates.sort_by_key(|c| !c.unresolved_phones.is_empty());
        candidates.truncate(limit);

        CompositionResult {
            composition_id,
            input,
            language,
            mode: request.mode,
            requested_style: request.style,
            voicebank_id: request.voicebank_id.clone(),
            candidates,
            diagnostics,
        }
    }

    fn candidates_for(
        &self,
        input: &str,
        language: Language,
        tone: i32,
        voicebank: Option<&Voicebank>,
    ) -> Vec<CompositionCandidate> {
        let mut out = Vec::new();
        let (word, hint) = split_hint(input);

        // Rest lyrics: one R candidate, nothing else.
        if hint.is_none() && is_rest(input) {
            let resolved = voicebank.and_then(|vb| vb.lookup("R", tone).map(|o| o.alias.clone()));
            let mut unresolved = Vec::new();
            if resolved.is_none() {
                unresolved.push("R".to_string());
            }
            out.push(self.candidate(
                input,
                language,
                Style::Cv,
                Strategy::DirectAlias,
                vec!["R".to_string()],
                vec!["/r/".to_string()],
                vec![resolved.unwrap_or_else(|| "R".to_string())],
                unresolved,
            ));
            return out;
        }

        // Strategy 1: the whole word IS a bank alias.
        if !word.is_empty() {
            if let Some(oto) = voicebank.and_then(|vb| vb.lookup(word, tone)) {
                out.push(self.candidate(
                    &oto.alias,
                    language,
                    classify_alias(&oto.alias),
                    Strategy::DirectAlias,
                    vec![word.to_string()],
                    self.universal_of_tokens(&[word.to_string()]),
                    vec![oto.alias.clone()],
                    Vec::new(),
                ));
            }
        }

        // Strategy 2: explicit [hint] tokens (authoritative, any language).
        if let Some(hint) = hint {
            let tokens = parse_hint(&hint);
            if !tokens.is_empty() {
                let (aliases, unresolved) = pair_tokens(&tokens, voicebank, tone);
                out.push(self.candidate(
                    word,
                    language,
                    Style::Cvvc,
                    Strategy::PhoneticHint,
                    tokens.clone(),
                    self.universal_of_tokens(&tokens),
                    aliases,
                    unresolved,
                ));
            }
        }

        match language {
            Language::Japanese => {
                let morae = split_morae(word);
                if !morae.is_empty() {
                    // Strategy 3: whole-lyric VCV alias (one alias per note,
                    // the OpenUtau JapaneseVCVPhonemizer shape).
                    let vcv_candidates = vec![format!("- {word}"), word.to_string()];
                    let (alias, unresolved) = first_resolved(&vcv_candidates, voicebank, tone);
                    out.push(self.candidate(
                        word,
                        language,
                        Style::Vcv,
                        Strategy::JapaneseVcv,
                        vec![word.to_string()],
                        self.universal_of_morae(&morae),
                        vec![alias],
                        unresolved,
                    ));

                    // Strategy 4: mora split (CV heads + tail vowels; pure
                    // vowel/ん/っ morae emit one head only).
                    let mut aliases = Vec::new();
                    let mut unresolved = Vec::new();
                    for mora in &morae {
                        let head_symbols: Vec<String> = match voicebank {
                            Some(vb) if vb.lookup(mora, tone).is_some() => vec![mora.clone()],
                            _ => match mora_to_english(mora) {
                                Some(syms) => syms.iter().map(|s| s.to_string()).collect(),
                                None => vec![mora.clone()],
                            },
                        };
                        for sym in &head_symbols {
                            match voicebank.and_then(|vb| vb.lookup(sym, tone)) {
                                Some(oto) => aliases.push(oto.alias.clone()),
                                None => {
                                    aliases.push(sym.clone());
                                    unresolved.push(sym.clone());
                                }
                            }
                        }
                        if !is_pure_vowel_mora(mora) {
                            if let Some(t) = tail_vowel(mora) {
                                match voicebank.and_then(|vb| vb.lookup(t, tone)) {
                                    Some(oto) => aliases.push(oto.alias.clone()),
                                    None => {
                                        aliases.push(t.to_string());
                                        unresolved.push(t.to_string());
                                    }
                                }
                            }
                        }
                    }
                    if !aliases.is_empty() {
                        let universal = self.universal_of_morae(&morae);
                        out.push(self.candidate(
                            word,
                            language,
                            Style::Cv,
                            Strategy::MoraCv,
                            morae,
                            universal,
                            aliases,
                            unresolved,
                        ));
                    }
                }
            }
            Language::English => {
                // Strategy 3: G2p dictionary + greedy pairing (CVVC-style).
                if let Some(tokens) = self.word_dict.query(word) {
                    let (aliases, unresolved) = pair_tokens(&tokens, voicebank, tone);
                    out.push(self.candidate(
                        word,
                        language,
                        Style::Cvvc,
                        Strategy::G2pCvvc,
                        tokens.clone(),
                        self.universal_of_tokens(&tokens),
                        aliases,
                        unresolved,
                    ));
                }
                // Strategy 4: letter-split fallback (mirrors the universal
                // phonemizer's app-parity path).
                let letters: Vec<String> = word
                    .chars()
                    .filter(|c| c.is_ascii_alphabetic())
                    .map(|c| c.to_ascii_lowercase().to_string())
                    .filter(|t| self.ipa.ipa(t).is_some())
                    .collect();
                if !letters.is_empty() {
                    let (aliases, unresolved) = pair_tokens(&letters, voicebank, tone);
                    out.push(self.candidate(
                        word,
                        language,
                        Style::Cvvc,
                        Strategy::LetterFallback,
                        letters.clone(),
                        self.universal_of_tokens(&letters),
                        aliases,
                        unresolved,
                    ));
                }
            }
            Language::Thai => {
                if let Some(parsed) = crate::thai::parse(&word) {
                    let tokens = parsed.tokens();
                    if !tokens.is_empty() {
                        let mut alias_sequences = Vec::new();
                        let onset = parsed.onset.as_deref().unwrap_or_default();
                        let glide = parsed.glide.as_deref().unwrap_or_default();
                        let vowel = parsed.vowel.as_deref().unwrap_or_default();
                        let coda = parsed.coda.as_deref().unwrap_or_default();
                        if !onset.is_empty() && !vowel.is_empty() {
                            let whole = format!("{onset}{glide}{vowel}");
                            alias_sequences.push(vec![whole]);
                            if !glide.is_empty() {
                                alias_sequences.push(vec![
                                    format!("{onset}{glide}"),
                                    format!("{glide}{vowel}"),
                                ]);
                            }
                        } else if !vowel.is_empty() {
                            alias_sequences.push(vec![vowel.to_string()]);
                        } else {
                            alias_sequences.push(vec![word.to_string()]);
                        }
                        if !coda.is_empty() {
                            for aliases in &mut alias_sequences {
                                aliases.push(format!("{vowel}{coda}"));
                            }
                        }
                        if onset.is_empty() && !coda.is_empty() && !vowel.is_empty() {
                            alias_sequences.insert(0, vec![format!("{vowel}{coda}")]);
                        }
                        let selected = alias_sequences
                            .iter()
                            .find(|aliases| {
                                voicebank.is_some_and(|vb| {
                                    aliases.iter().all(|a| vb.lookup(a, tone).is_some())
                                })
                            })
                            .cloned()
                            .or_else(|| alias_sequences.first().cloned())
                            .unwrap_or_default();
                        let unresolved: Vec<String> = selected
                            .iter()
                            .filter(|alias| {
                                voicebank.and_then(|vb| vb.lookup(alias, tone)).is_none()
                            })
                            .cloned()
                            .collect();
                        let aliases = selected
                            .iter()
                            .map(|alias| {
                                voicebank
                                    .and_then(|vb| vb.lookup(alias, tone))
                                    .map(|oto| oto.alias.clone())
                                    .unwrap_or_else(|| alias.clone())
                            })
                            .collect();
                        out.push(self.candidate(
                            word,
                            language,
                            Style::Vccv,
                            Strategy::ThaiVccv,
                            tokens.clone(),
                            self.universal_of_tokens(&tokens),
                            aliases,
                            unresolved,
                        ));
                    }
                }
            }
        }

        // Final strategy: universal (IPA) mapping — the universal-mode view
        // of the language sequence. For English the sequence follows the
        // same resolution ladder as the language strategies (dictionary →
        // letters → verbatim).
        let lang_phones: Vec<String> = if !word.is_empty() {
            match language {
                Language::Japanese => split_morae(word),
                Language::English => self
                    .word_dict
                    .query(word)
                    .or_else(|| {
                        let letters: Vec<String> = word
                            .chars()
                            .filter(|c| c.is_ascii_alphabetic())
                            .map(|c| c.to_ascii_lowercase().to_string())
                            .filter(|t| self.ipa.ipa(t).is_some())
                            .collect();
                        if letters.is_empty() {
                            None
                        } else {
                            Some(letters)
                        }
                    })
                    .unwrap_or_else(|| vec![word.to_string()]),
                Language::Thai => crate::thai::parse(word)
                    .map(|parsed| parsed.tokens())
                    .filter(|tokens| !tokens.is_empty())
                    .unwrap_or_else(|| vec![word.to_string()]),
            }
        } else if let Some(hint) = hint {
            parse_hint(&hint)
        } else {
            Vec::new()
        };
        if !lang_phones.is_empty() {
            let universal = match language {
                Language::Japanese => self.universal_of_morae(&lang_phones),
                _ => self.universal_of_tokens(&lang_phones),
            };
            let (aliases, kinds) = self.resolve_universal(&universal, voicebank, tone);
            let unresolved = kinds
                .iter()
                .zip(universal.iter())
                .filter(|(k, _)| **k == AliasResolutionKind::Unresolved)
                .map(|(_, p)| p.clone())
                .collect();
            out.push(self.candidate(
                word,
                language,
                Style::Cv,
                Strategy::UniversalIpa,
                lang_phones,
                universal,
                aliases,
                unresolved,
            ));
        }

        out
    }

    /// Assemble one candidate (id is deterministic for the request context).
    #[allow(clippy::too_many_arguments)]
    fn candidate(
        &self,
        display: &str,
        language: Language,
        style: Style,
        strategy: Strategy,
        language_phones: Vec<String>,
        universal_phones: Vec<String>,
        voicebank_aliases: Vec<String>,
        unresolved_phones: Vec<String>,
    ) -> CompositionCandidate {
        let id = format!(
            "cand#{}#{}#{}#{}",
            language.as_str(),
            strategy.as_str(),
            style.as_str(),
            fnv1a(&language_phones.join("\u{1}"))
        );
        CompositionCandidate {
            candidate_id: id,
            display_text: display.to_string(),
            language,
            style,
            strategy,
            language_phones,
            universal_phones,
            voicebank_aliases,
            unresolved_phones,
        }
    }

    /// Language tokens → universal (IPA where known, token verbatim else).
    fn universal_of_tokens(&self, tokens: &[String]) -> Vec<String> {
        tokens
            .iter()
            .map(|t| {
                self.ipa
                    .ipa(t)
                    .map(str::to_string)
                    .unwrap_or_else(|| t.clone())
            })
            .collect()
    }

    /// Morae → universal: transliterate each mora to English symbols, then
    /// map through the IPA dictionary. Morae the transliteration table does
    /// not cover (all kana today) pass through verbatim — a documented
    /// coverage gap, reported as an unresolved phone against the bank.
    fn universal_of_morae(&self, morae: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for mora in morae {
            match mora_to_english(mora) {
                Some(syms) => {
                    for s in syms {
                        out.push(
                            self.ipa
                                .ipa(s)
                                .map(str::to_string)
                                .unwrap_or_else(|| s.to_string()),
                        );
                    }
                }
                None => out.push(
                    self.ipa
                        .ipa(mora)
                        .map(str::to_string)
                        .unwrap_or_else(|| mora.clone()),
                ),
            }
        }
        out
    }

    /// Universal phones → bank aliases, with per-phone resolution kinds.
    fn resolve_universal(
        &self,
        phones: &[String],
        voicebank: Option<&Voicebank>,
        tone: i32,
    ) -> (Vec<String>, Vec<AliasResolutionKind>) {
        let Some(vb) = voicebank else {
            return (
                phones
                    .iter()
                    .map(|p| p.trim_matches('/').to_string())
                    .collect(),
                phones
                    .iter()
                    .map(|_| AliasResolutionKind::Unresolved)
                    .collect(),
            );
        };
        let manifest = PhonemeManifest::from_voicebank(vb);
        let nearest = NearestPhoneme::new(&manifest);
        let mut aliases = Vec::with_capacity(phones.len());
        let mut kinds = Vec::with_capacity(phones.len());
        for phone in phones {
            let alias = self
                .ipa
                .arpabet(phone)
                .unwrap_or(phone.trim_matches('/'))
                .to_string();
            if let Some(oto) = vb.lookup(&alias, tone) {
                aliases.push(oto.alias.clone());
                kinds.push(AliasResolutionKind::Exact);
                continue;
            }
            if phone != &alias {
                if let Some(oto) = vb.lookup(phone, tone) {
                    aliases.push(oto.alias.clone());
                    kinds.push(AliasResolutionKind::Bridge);
                    continue;
                }
            }
            if let Some(n) = nearest.nearest(&alias) {
                aliases.push(n);
                kinds.push(AliasResolutionKind::Nearest);
                continue;
            }
            aliases.push(alias);
            kinds.push(AliasResolutionKind::Unresolved);
        }
        (aliases, kinds)
    }
}

/// Detect the language of raw composition input from its script.
/// Kana → Japanese, Thai script → Thai, otherwise English (matches
/// `crate::detect`'s kana rule and adds the Thai block as a known gap).
pub fn detect_language(input: &str) -> Language {
    for c in input.chars() {
        match c as u32 {
            0x3040..=0x309F | 0x30A0..=0x30FF | 0x31F0..=0x31FF => return Language::Japanese,
            0x0E00..=0x0E7F => return Language::Thai,
            _ => {}
        }
    }
    Language::English
}

/// Split `word [hint]` into `(word, Some(hint))`; `(input, None)` when no
/// bracket hint is present.
fn split_hint(input: &str) -> (&str, Option<&str>) {
    let Some(start) = input.find('[') else {
        return (input, None);
    };
    let Some(end) = input[start..].find(']').map(|e| start + e) else {
        return (input, None);
    };
    (input[..start].trim(), Some(&input[start + 1..end]))
}

/// Greedy multi-token pairing against the bank's oto table (longest alias
/// first, up to 3 tokens), with the CVVC word-final coda rule (`v` → `v-`).
/// Mirrors `EnglishCvvcPhonemizer::pair`. Returns the alias sequence and
/// the tokens that did not resolve to a bank alias (all of them when no
/// bank is attached).
fn pair_tokens(
    tokens: &[String],
    voicebank: Option<&Voicebank>,
    tone: i32,
) -> (Vec<String>, Vec<String>) {
    let mut aliases = Vec::new();
    let mut unresolved = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let mut matched: Option<(String, usize)> = None;
        if let Some(vb) = voicebank {
            let max = (tokens.len() - i).min(3);
            for w in (2..=max).rev() {
                let candidate = tokens[i..i + w].join(" ");
                if vb.lookup(&candidate, tone).is_some() {
                    matched = Some((candidate, w));
                    break;
                }
            }
        }
        if let Some((alias, w)) = matched {
            aliases.push(alias);
            i += w;
            continue;
        }
        let token = &tokens[i];
        let mut alias = token.clone();
        let mut resolved = false;
        if let Some(vb) = voicebank {
            if i == tokens.len() - 1 && crate::translit::is_english_vowel(token) {
                let coda = format!("{token}-");
                if let Some(oto) = vb.lookup(&coda, tone) {
                    alias = oto.alias.clone();
                    resolved = true;
                }
            }
            if !resolved && vb.lookup(token, tone).is_some() {
                resolved = true;
            }
        }
        aliases.push(alias);
        if !resolved {
            unresolved.push(token.clone());
        }
        i += 1;
    }
    (aliases, unresolved)
}

/// First alias in `candidates` that the bank renders; otherwise the first
/// candidate verbatim with all candidates listed unresolved.
fn first_resolved(
    candidates: &[String],
    voicebank: Option<&Voicebank>,
    tone: i32,
) -> (String, Vec<String>) {
    if let Some(vb) = voicebank {
        for c in candidates {
            if vb.lookup(c, tone).is_some() {
                return (c.clone(), Vec::new());
            }
        }
    }
    (
        candidates.first().cloned().unwrap_or_default(),
        candidates.to_vec(),
    )
}

/// Classify a whole-alias candidate by its shape. Shape-based tagging is
/// only used for aliases the bank already provides — strategy-built
/// candidates always tag by construction.
fn classify_alias(alias: &str) -> Style {
    if alias.contains(' ') || alias.starts_with('-') || alias.starts_with('*') {
        Style::Vcv
    } else if alias.ends_with('-') {
        Style::Cvvc
    } else {
        Style::Cv
    }
}

/// Deterministic FNV-1a (64-bit) — candidate/composition ids.
fn fnv1a(data: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Deterministic composition id for the request context.
fn composition_id(input: &str, language: Language, request: &CompositionRequest) -> String {
    let mut key = String::from("comp\u{1}");
    key.push_str(input);
    key.push('\u{1}');
    key.push_str(language.as_str());
    key.push('\u{1}');
    key.push_str(request.mode.as_str());
    key.push('\u{1}');
    key.push_str(request.style.as_str());
    key.push('\u{1}');
    key.push_str(&request.limit.unwrap_or(DEFAULT_LIMIT).to_string());
    key.push('\u{1}');
    key.push_str(&request.tone.to_string());
    if let Some(id) = &request.voicebank_id {
        key.push('\u{1}');
        key.push_str(id);
    }
    format!("comp-{}", fnv1a(&key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use voicebank::{parse_oto_ini, Voicebank};

    /// Minimal Japanese VCV bank (oto.ini text; no wav files involved).
    /// Kana aliases exercise the kana paths; romaji aliases stay for the
    /// direct-alias test. `otos` is filled because `PhonemeManifest`
    /// derives from it.
    fn ja_voicebank() -> Voicebank {
        let ini = concat!(
            "_a_a_a.wav=- か,0.0,100.0,-275.001,1.994,10.0\n",
            "_a_a_a.wav=か,100.0,100.0,-275.001,1.994,10.0\n",
            "_a_a_a.wav=a,0.0,100.0,-275.001,1.994,10.0\n",
            "_sa+_sa+_s-.wav=- sa,80.0,251.664,-498.668,201.665,73.333\n",
            "_sa+_sa+_s-.wav=sa,736.336,185.666,-352.001,135.666,44.0\n",
            "_s3_s3_s-.wav=s,742.069,50.731,-122.467,0.731,27.133\n",
        );
        let parsed = parse_oto_ini(ini.as_bytes(), None);
        let otos = parsed.otos;
        let oto_map: HashMap<String, _> =
            otos.iter().map(|o| (o.alias.clone(), o.clone())).collect();
        Voicebank {
            name: "ja-test".into(),
            base_path: Default::default(),
            path: Default::default(),
            id: "ja-test".into(),
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
            subbanks: Vec::new(),
            oto_sets: Vec::new(),
            otos,
            oto_map,
            warnings: Vec::new(),
        }
    }

    fn request(input: &str) -> CompositionRequest {
        CompositionRequest {
            input: input.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn thai_candidates_use_rule_based_vccv_conversion_and_surface_unresolved_aliases() {
        let composer = PhonemeComposer::new();
        let result = composer.compose(&request("กะ"), None);
        assert_eq!(result.language, Language::Thai);
        let thai = result
            .candidates
            .iter()
            .find(|candidate| candidate.strategy == Strategy::ThaiVccv)
            .expect("Thai VCCV candidate");
        assert_eq!(thai.language_phones, ["k", "a"]);
        assert_eq!(thai.style, Style::Vccv);
        assert_eq!(thai.voicebank_aliases, ["ka"]);
        assert_eq!(thai.unresolved_phones, ["ka"]);

        let bank = Voicebank::from_oto_bytes(
            "thai-fixture",
            "thai-fixture",
            ".",
            ".",
            b"ka.wav=ka,0,100,0,0,0\n",
        );
        let mut bank_request = request("กะ");
        bank_request.language = Some(Language::Thai);
        bank_request.style = Style::Vccv;
        bank_request.voicebank_id = Some(bank.id.clone());
        let mapped = composer
            .compose(&bank_request, Some(&bank))
            .candidates
            .into_iter()
            .find(|candidate| candidate.strategy == Strategy::ThaiVccv)
            .expect("Thai VCCV candidate resolves against the bank");
        assert_eq!(mapped.voicebank_aliases, ["ka"]);
        assert!(mapped.unresolved_phones.is_empty());
    }

    #[test]
    fn japanese_input_lists_vcv_and_mora_candidates() {
        let composer = PhonemeComposer::new();
        let result = composer.compose(&request("か"), None);
        assert_eq!(result.language, Language::Japanese);
        let strategies: Vec<Strategy> = result.candidates.iter().map(|c| c.strategy).collect();
        assert!(strategies.contains(&Strategy::JapaneseVcv));
        assert!(strategies.contains(&Strategy::MoraCv));
        assert!(strategies.contains(&Strategy::UniversalIpa));
        // Without a bank everything is unresolved and reported verbatim.
        for c in &result.candidates {
            assert!(!c.unresolved_phones.is_empty());
        }
        // The VCV candidate shows the OpenUtau first-note alias form.
        let vcv = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::JapaneseVcv)
            .expect("vcv candidate");
        assert_eq!(vcv.voicebank_aliases, ["- か"]);
        assert_eq!(vcv.style, Style::Vcv);
    }

    #[test]
    fn direct_alias_candidate_when_input_is_a_bank_alias() {
        let vb = ja_voicebank();
        let composer = PhonemeComposer::new();
        let result = composer.compose(&request("- sa"), Some(&vb));
        let direct = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::DirectAlias)
            .expect("direct alias candidate");
        assert_eq!(direct.voicebank_aliases, ["- sa"]);
        assert!(direct.unresolved_phones.is_empty());
        assert_eq!(
            direct.style,
            Style::Vcv,
            "shape: '- sa' is a VCV head alias"
        );
        // Resolved-first ordering: the direct alias candidate is first.
        assert_eq!(result.candidates[0].candidate_id, direct.candidate_id);
    }

    #[test]
    fn japanese_candidates_resolve_against_a_kana_bank() {
        let vb = ja_voicebank();
        let composer = PhonemeComposer::new();
        let result = composer.compose(&request("か"), Some(&vb));

        let vcv = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::JapaneseVcv)
            .expect("vcv candidate");
        assert_eq!(vcv.voicebank_aliases, ["- か"]);
        assert!(vcv.unresolved_phones.is_empty());

        let mora = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::MoraCv)
            .expect("mora candidate");
        assert_eq!(mora.language_phones, ["か"]);
        assert_eq!(mora.voicebank_aliases, ["か", "a"]);
        assert!(mora.unresolved_phones.is_empty());

        // か is a CV mora, so the mora split adds the tail vowel.
        assert_eq!(
            mora.universal_phones,
            ["か"],
            "kana passes through (documented gap)"
        );
    }

    #[test]
    fn pure_vowel_mora_emits_a_single_head() {
        let composer = PhonemeComposer::new();
        let result = composer.compose(&request("あ"), None);
        let mora = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::MoraCv)
            .expect("mora candidate");
        assert_eq!(mora.voicebank_aliases, ["あ"], "no tail after a pure vowel");
        assert_eq!(mora.language_phones, ["あ"]);
    }

    #[test]
    fn english_g2p_candidate_pairs_multi_token_aliases() {
        let vb = crate::english::fixtures::en_vccv_voicebank();
        // read → [r, E, d]; the bank pairs `E d`, `r` stays verbatim
        // (proven by the english phonemizer's own g2p test).
        let mut dict = crate::g2p::G2pDictionary::new();
        dict.add_symbol("r", false, false)
            .add_symbol("E", true, false)
            .add_symbol("d", false, false);
        dict.add_entry("read", &["r", "E", "d"]);
        let composer =
            PhonemeComposer::with_g2p(crate::g2p::G2pFallbacks::new(vec![Box::new(dict)]));

        let result = composer.compose(&request("read"), Some(&vb));
        let g2p = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::G2pCvvc)
            .expect("g2p candidate");
        assert_eq!(g2p.language_phones, ["r", "E", "d"]);
        assert_eq!(g2p.voicebank_aliases, ["r", "E d"]);
        assert_eq!(g2p.unresolved_phones, ["r"]);
        assert_eq!(g2p.style, Style::Cvvc);
        assert_eq!(result.language, Language::English);
        // The universal view of the same tokens is IPA.
        assert_eq!(g2p.universal_phones, ["/r/", "/ɛ/", "/d/"]);
    }

    #[test]
    fn hint_tokens_are_authoritative_for_any_language() {
        let composer = PhonemeComposer::new();
        let result = composer.compose(&request("か [s A]"), None);
        let hint = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::PhoneticHint)
            .expect("hint candidate");
        assert_eq!(hint.language_phones, ["s", "A"]);
        assert_eq!(hint.universal_phones, ["/s/", "/ɑ/"]);
        assert_eq!(hint.display_text, "か");
    }

    #[test]
    fn style_restriction_filters_and_reports() {
        let vb = ja_voicebank();
        let composer = PhonemeComposer::new();
        let mut req = request("か");
        req.style = Style::Vccv;
        let result = composer.compose(&req, Some(&vb));
        assert!(result.candidates.is_empty());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.contains("no vccv candidates")));
    }

    #[test]
    fn limit_bounds_the_candidate_list() {
        let composer = PhonemeComposer::new();
        let mut req = request("か");
        req.limit = Some(2);
        let result = composer.compose(&req, None);
        assert_eq!(result.candidates.len(), 2);
        // Clamp: a limit over MAX_LIMIT still bounds the list.
        req.limit = Some(9999);
        let result = composer.compose(&req, None);
        assert!(result.candidates.len() <= MAX_LIMIT);
    }

    #[test]
    fn identical_requests_are_byte_identical() {
        let vb = ja_voicebank();
        let a = PhonemeComposer::new().compose(&request("か"), Some(&vb));
        let b = PhonemeComposer::new().compose(&request("か"), Some(&vb));
        assert_eq!(a, b);
        let ja = serde_json::to_string(&a).expect("serialize");
        let jb = serde_json::to_string(&b).expect("serialize");
        assert_eq!(ja, jb);
    }

    #[test]
    fn composition_id_is_deterministic_and_input_sensitive() {
        let a = PhonemeComposer::new().compose(&request("か"), None);
        let b = PhonemeComposer::new().compose(&request("き"), None);
        assert_ne!(a.composition_id, b.composition_id);
        assert_eq!(
            a.composition_id,
            PhonemeComposer::new()
                .compose(&request("か"), None)
                .composition_id
        );
    }

    #[test]
    fn long_input_is_truncated_with_a_diagnostic() {
        let composer = PhonemeComposer::new();
        let mut req = request(&"a".repeat(400));
        req.limit = Some(1);
        let result = composer.compose(&req, None);
        assert_eq!(result.input.chars().count(), MAX_INPUT_CHARS);
        assert!(result.diagnostics.iter().any(|d| d.contains("truncated")));
    }

    #[test]
    fn committed_pronunciation_keeps_universal_and_rederives_aliases() {
        let vb = ja_voicebank();
        // "na" → letter fallback [n, a] → universal [/n/, /a/].
        let result = PhonemeComposer::new().compose(&request("na"), Some(&vb));
        let letters = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::LetterFallback)
            .expect("letter fallback candidate");
        assert_eq!(letters.universal_phones, ["/n/", "/a/"]);

        let committed = NotePronunciation::from_candidate(letters, MappingMode::VoicebankNative)
            .expect("candidate has universal phones");
        assert_eq!(committed.style, Style::Cvvc);
        assert_eq!(committed.language, Language::English);

        // Same bank: /a/ resolves exactly, /n/ is reported unresolved.
        let ja = committed.resolve_aliases(Some(&vb), 60);
        assert_eq!(ja[0].kind, AliasResolutionKind::Unresolved);
        assert_eq!(ja[0].alias, "n");
        assert_eq!(ja[1].kind, AliasResolutionKind::Exact);
        assert_eq!(ja[1].alias, "a");

        // A different bank recomputes aliases; the universal choice is
        // unchanged (the persistence boundary).
        let committed_json = serde_json::to_string(&committed).expect("serialize");
        let empty_bank = Voicebank {
            otos: Vec::new(),
            oto_map: HashMap::new(),
            ..ja_voicebank()
        };
        let other = committed.resolve_aliases(Some(&empty_bank), 60);
        assert!(other
            .iter()
            .all(|r| r.kind == AliasResolutionKind::Unresolved));
        assert_eq!(
            serde_json::to_string(&committed).expect("serialize"),
            committed_json,
            "commit is not mutated by alias recomputation"
        );

        // No bank: everything unresolved, phones unchanged.
        let none = committed.resolve_aliases(None, 60);
        assert!(none
            .iter()
            .all(|r| r.kind == AliasResolutionKind::Unresolved));
        assert_eq!(none.len(), committed.universal_phones.len());
    }

    #[test]
    fn universal_candidate_maps_through_the_manifest() {
        let vb = ja_voicebank();
        // "na" → letters [n, a] → universal [/n/, /a/]; the universal
        // candidate maps /a/ exactly and reports /n/ unresolved.
        let result = PhonemeComposer::new().compose(&request("na"), Some(&vb));
        let uni = result
            .candidates
            .iter()
            .find(|c| c.strategy == Strategy::UniversalIpa)
            .expect("universal candidate");
        assert_eq!(uni.language_phones, ["n", "a"]);
        assert_eq!(uni.universal_phones, ["/n/", "/a/"]);
        assert_eq!(uni.voicebank_aliases.len(), uni.universal_phones.len());
        // Unresolved phones are reported in the candidate's own sequence
        // granularity: the universal candidate reports universal phones.
        assert_eq!(uni.unresolved_phones, ["/n/"]);
        assert_eq!(uni.voicebank_aliases, ["n", "a"]);
    }

    #[test]
    fn unknown_language_id_is_rejected_not_guessed() {
        assert_eq!(Language::from_id("ja"), Some(Language::Japanese));
        assert_eq!(Language::from_id("tlh"), None);
    }

    #[test]
    fn rest_input_emits_a_single_candidate() {
        let composer = PhonemeComposer::new();
        let result = composer.compose(&request("R"), None);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].voicebank_aliases, ["R"]);
        assert_eq!(result.language, Language::English);
    }

    #[test]
    fn forced_language_mismatch_is_reported() {
        let composer = PhonemeComposer::new();
        let mut req = request("か");
        req.language = Some(Language::English);
        let result = composer.compose(&req, None);
        assert_eq!(result.language, Language::English);
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.contains("looks like ja but language is forced to en")));
    }
}
