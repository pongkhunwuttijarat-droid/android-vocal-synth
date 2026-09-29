//! Japanese VCV phonemizer.
//!
//! Splits a kana lyric into morae and emits a VCV-style phoneme sequence
//! per note: a consonant-onset mora becomes `[CV, tail-vowel]`
//! (`か` → `[か, a]`), a pure vowel mora stays a single phoneme
//! (`あ` → `[あ]`, `ん` → `[ん]`), and `っ` / `ー` emit themselves.
//!
//! Head phonemes are resolved against the singer's oto table using the
//! running tail vowel of the previous mora/note, mirroring the alias
//! candidate order of OpenUtau's `JapaneseVCVPhonemizer`:
//!
//! * with a previous tail `v`: `["v lyric", "* lyric", "lyric", "- lyric"]`
//! * without: `["- lyric", "lyric"]`
//!
//! e.g. `か` followed by `き` resolves the `き` head to the real VCV alias
//! `a き`. When no alias matches (or no singer is given) the raw kana is
//! kept. Rest notes (`-`, `R`, ...) emit the single phoneme `R`, which the
//! ja_vcv test bank aliases as `{vowel} R`.
//!
//! The kana → tail-vowel table mirrors `JapaneseVCVPhonemizer.vowels`.

use std::collections::HashMap;
use std::sync::OnceLock;

use domain::{UNote, UPhoneme};
use voicebank::Voicebank;

use crate::g2p::parse_hint;
use crate::phonemizer::{
    is_rest, make_phoneme, positions_from_durations, split_duration, Phonemizer,
};

/// Kana rows of the OpenUtau Japanese VCV phonemizer: tail vowel per kana.
/// Romaji `a i u e o n` are included so ASCII lyrics still split.
const VOWEL_ROWS: &[(&str, &[&str])] = &[
    (
        "a",
        &[
            "ぁ", "あ", "か", "が", "さ", "ざ", "た", "だ", "な", "は", "ば", "ぱ", "ま", "ゃ", "や",
            "ら", "わ", "ァ", "ア", "カ", "ガ", "サ", "ザ", "タ", "ダ", "ナ", "ハ", "バ", "パ", "マ",
            "ャ", "ヤ", "ラ", "ワ", "a",
        ],
    ),
    (
        "i",
        &[
            "ぃ", "い", "き", "ぎ", "し", "じ", "ち", "ぢ", "に", "ひ", "び", "ぴ", "み", "り", "ゐ",
            "ィ", "イ", "キ", "ギ", "シ", "ジ", "チ", "ヂ", "ニ", "ヒ", "ビ", "ピ", "ミ", "リ", "ヰ",
            "i",
        ],
    ),
    (
        "u",
        &[
            "ぅ", "う", "く", "ぐ", "す", "ず", "つ", "づ", "ぬ", "ふ", "ぶ", "ぷ", "む", "ゅ", "ゆ",
            "る", "ゥ", "ウ", "ク", "グ", "ス", "ズ", "ツ", "ヅ", "ヌ", "フ", "ブ", "プ", "ム", "ュ",
            "ユ", "ル", "ヴ", "u",
        ],
    ),
    (
        "e",
        &[
            "ぇ", "え", "け", "げ", "せ", "ぜ", "て", "で", "ね", "へ", "べ", "ぺ", "め", "れ", "ゑ",
            "ェ", "エ", "ケ", "ゲ", "セ", "ゼ", "テ", "デ", "ネ", "ヘ", "ベ", "ペ", "メ", "レ", "ヱ",
            "e",
        ],
    ),
    (
        "o",
        &[
            "ぉ", "お", "こ", "ご", "そ", "ぞ", "と", "ど", "の", "ほ", "ぼ", "ぽ", "も", "ょ", "よ",
            "ろ", "を", "ォ", "オ", "コ", "ゴ", "ソ", "ゾ", "ト", "ド", "ノ", "ホ", "ボ", "ポ", "モ",
            "ョ", "ヨ", "ロ", "ヲ", "o",
        ],
    ),
    ("n", &["ん", "n"]),
    ("ng", &["ン"]),
];

/// `kana → tail vowel`, including the romaji vowels (`"a"` → `"a"`).
fn vowel_lookup() -> &'static HashMap<&'static str, &'static str> {
    static LOOKUP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    LOOKUP.get_or_init(|| {
        let mut map = HashMap::new();
        for (vowel, kanas) in VOWEL_ROWS {
            for kana in *kanas {
                map.insert(*kana, *vowel);
            }
        }
        map
    })
}

/// Tail vowel of a single mora (`か` → `Some("a")`, `ん` → `Some("n")`,
/// `っ` / `ー` / unknown → `None`).
pub fn tail_vowel(mora: &str) -> Option<&'static str> {
    vowel_lookup().get(mora).copied()
}

/// Kana that are vowels on their own (a single-phoneme mora).
fn is_pure_vowel_mora(mora: &str) -> bool {
    let mut chars = mora.chars();
    let Some(c) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false; // multi-char mora such as きゃ is a CV cluster
    }
    matches!(
        c,
        'ぁ' | 'あ'
            | 'ぃ'
            | 'い'
            | 'ぅ'
            | 'う'
            | 'ぇ'
            | 'え'
            | 'ぉ'
            | 'お'
            | 'を'
            | 'ん'
            | 'ン'
            | 'ァ'
            | 'ア'
            | 'ィ'
            | 'イ'
            | 'ゥ'
            | 'ウ'
            | 'ェ'
            | 'エ'
            | 'ォ'
            | 'オ'
            | 'ヲ'
            | 'a'
            | 'i'
            | 'u'
            | 'e'
            | 'o'
            | 'n'
    )
}

fn is_small_yoon(ch: char) -> bool {
    matches!(ch, 'ゃ' | 'ゅ' | 'ょ' | 'ャ' | 'ュ' | 'ョ')
}

fn is_sokuon(ch: char) -> bool {
    matches!(ch, 'っ' | 'ッ')
}

/// Split a kana lyric into morae.
///
/// Rules:
/// * a small yōon kana (`ゃ ゅ ょ`) attaches to the previous mora
///   (`きゃ` → one mora);
/// * `っ` / `ッ` (sokuon) and `ー` (chōonpu) are their own mora;
/// * any other character (kana or not) starts a new mora.
pub fn split_morae(lyric: &str) -> Vec<String> {
    let mut morae: Vec<String> = Vec::new();
    for ch in lyric.chars() {
        if is_small_yoon(ch) && !morae.is_empty() {
            morae.last_mut().expect("non-empty").push(ch);
        } else if is_sokuon(ch) {
            morae.push("っ".to_string());
        } else if ch == 'ー' {
            morae.push("ー".to_string());
        } else {
            morae.push(ch.to_string());
        }
    }
    morae
}

/// A mora of a note plus its resolved tail vowel.
struct MoraPlan {
    /// Phoneme symbol: the mora itself (e.g. `か`, `きゃ`, `っ`, `ー`).
    mora: String,
    /// Tail vowel contributed to the next mora/note (`None` for
    /// `っ`/unknown; `ー` inherits the previous mora's tail).
    tail: Option<&'static str>,
    /// Whether the mora emits only itself (vowel kana, `ん`, `っ`, `ー`)
    /// instead of `[mora, tail]`.
    pure: bool,
}

/// Plan the phoneme sequence for a kana lyric, honouring ん/っ/ー rules.
/// `initial_tail` is the running tail entering the note (used when the
/// first mora is a `ー` extending the previous note's vowel).
fn plan_morae(lyric: &str, initial_tail: Option<&'static str>) -> Vec<MoraPlan> {
    let morae = split_morae(lyric);
    let mut prev_tail = initial_tail;
    let mut plans = Vec::with_capacity(morae.len());
    for mora in morae {
        let tail = if mora == "ー" {
            prev_tail // chōonpu extends the previous vowel
        } else {
            tail_vowel(&mora)
        };
        let pure = mora == "ー" || tail.is_none() || is_pure_vowel_mora(&mora);
        plans.push(MoraPlan { mora, tail, pure });
        prev_tail = tail;
    }
    plans
}

/// Japanese VCV phonemizer (rule-based kana → mora split).
pub struct JapaneseVcvPhonemizer;

impl Phonemizer for JapaneseVcvPhonemizer {
    fn process(&self, notes: &[UNote], singer: Option<&Voicebank>) -> Vec<UPhoneme> {
        let mut out: Vec<UPhoneme> = Vec::new();
        // Tail vowel of the previous note (or previous mora within a note),
        // used to resolve the next head alias, e.g. か → "a き".
        let mut running_tail: Option<&'static str> = None;

        for (nidx, note) in notes.iter().enumerate() {
            let (lyric, hint) = note.phonetic_hint();

            if let Some(hint) = hint {
                // A phonetic hint is authoritative: emit its tokens as-is,
                // one phoneme each, evenly splitting the note.
                let tokens = parse_hint(&hint);
                if !tokens.is_empty() {
                    let durations = split_duration(note.duration, tokens.len());
                    let positions = positions_from_durations(&durations);
                    for (i, (token, (&pos, &dur))) in tokens
                        .iter()
                        .zip(positions.iter().zip(durations.iter()))
                        .enumerate()
                    {
                        let candidates = vec![token.clone(), format!("- {token}")];
                        let mut ph = make_phoneme(
                            token.clone(),
                            pos,
                            dur,
                            i as i32,
                            note.tone,
                            nidx,
                        );
                        resolve(&mut ph, singer, note.tone, &candidates);
                        out.push(ph);
                    }
                    running_tail = None;
                    continue;
                }
            }

            if is_rest(&lyric) {
                // Rest: single `R` phoneme, aliased `{vowel} R` after a
                // vowel note (the ja_vcv bank has e.g. `u RA3`).
                let candidates = match running_tail {
                    Some(t) => vec![format!("{t} R"), "R".to_string(), "- R".to_string()],
                    None => vec!["R".to_string(), "- R".to_string()],
                };
                let mut ph = make_phoneme("R", 0, note.duration, 0, note.tone, nidx);
                resolve(&mut ph, singer, note.tone, &candidates);
                out.push(ph);
                running_tail = None;
                continue;
            }

            let plans = plan_morae(&lyric, running_tail);

            // --- ONE PHONEME PER NOTE (OpenUtau parity) ---------------------
            // The reference JapaneseVCVPhonemizer.Process returns EXACTLY ONE
            // phoneme for the whole note: the VCV alias picked by trying
            //   "{prev vowel} {lyric}", "* {lyric}", "{lyric}", "- {lyric}"
            // (ref-openutau-clean/.../JapaneseVCVPhonemizer.cs:56-80). The note
            // duration is never divided between phonemes.
            //
            // Splitting a note into [consonant, vowel] and sharing the note
            // duration evenly (the mora path below) put the vowel in the MIDDLE
            // of the note: a 1500 ms "sa" note held the consonant for 750 ms
            // before the vowel started, which is the reported broken onset.
            let whole_candidates: Vec<String> = match running_tail {
                Some(t) => vec![
                    format!("{t} {lyric}"),
                    format!("* {lyric}"),
                    lyric.clone(),
                    format!("- {lyric}"),
                ],
                None => vec![format!("- {lyric}"), lyric.clone()],
            };
            let mut whole = make_phoneme(lyric.clone(), 0, note.duration, 0, note.tone, nidx);
            if resolve_any(&mut whole, singer, note.tone, &whole_candidates) {
                running_tail = plans.last().and_then(|p| p.tail);
                out.push(whole);
                continue;
            }
            // Fallback for banks without a whole-lyric alias (e.g. a bank that
            // only has mora-level aliases): keep the mora/consonant split.

            // Flatten to (symbol, is_tail) pairs: [か, a] or [あ].
            let mut symbols: Vec<(String, bool)> = Vec::new();
            for plan in &plans {
                symbols.push((plan.mora.clone(), false));
                if !plan.pure {
                    if let Some(t) = plan.tail {
                        symbols.push((t.to_string(), true));
                    }
                }
            }
            if symbols.is_empty() {
                symbols.push((lyric.clone(), false));
            }

            // CROSS-LANGUAGE: when the bank has no alias for a Japanese mora
            // (e.g. an English voicebank), transliterate the mora to English
            // phoneme symbols the bank DOES alias (せ → s e, ぼ → b O ...).
            // Only heads are transliterated; tail vowels pass through.
            let mut symbols: Vec<(String, bool)> = symbols
                .into_iter()
                .flat_map(|(sym, is_tail)| {
                    if is_tail {
                        vec![(sym, true)]
                    } else if let Some(eng) = crate::translit::mora_to_english(&sym) {
                        eng.iter().map(|s| (s.to_string(), false)).collect()
                    } else {
                        vec![(sym, false)]
                    }
                })
                .collect();
            if symbols.is_empty() {
                symbols.push((lyric.clone(), false));
            }

            let durations = split_duration(note.duration, symbols.len());
            let positions = positions_from_durations(&durations);
            let mut mora_tail = running_tail;
            for (i, ((symbol, is_tail), (&pos, &dur))) in symbols
                .iter()
                .zip(positions.iter().zip(durations.iter()))
                .enumerate()
            {
                let mut ph = make_phoneme(symbol.clone(), pos, dur, i as i32, note.tone, nidx);
                if *is_tail {
                    // Tail vowel: plain lookup (no VCV bridging).
                    let candidates = vec![symbol.clone()];
                    resolve(&mut ph, singer, note.tone, &candidates);
                } else {
                    // Head: VCV alias with the running tail vowel.
                    let candidates = match mora_tail {
                        Some(t) => vec![
                            format!("{t} {symbol}"),
                            format!("* {symbol}"),
                            symbol.clone(),
                            format!("- {symbol}"),
                        ],
                        None => vec![symbol.clone(), format!("- {symbol}")],
                    };
                    resolve(&mut ph, singer, note.tone, &candidates);
                    mora_tail = tail_vowel(symbol);
                }
                out.push(ph);
            }
            running_tail = plans.last().and_then(|p| p.tail);
        }
        out
    }
}

/// Try each candidate alias against the singer's oto table (tone-mapped,
/// with plain fallback) and keep the first match in ph.phoneme.
/// raw_phoneme always stays the original symbol.
fn resolve(ph: &mut UPhoneme, singer: Option<&Voicebank>, tone: i32, candidates: &[String]) {
    let _ = resolve_any(ph, singer, tone, candidates);
}

/// resolve that also reports whether a candidate alias was found in the
/// singer oto table (the caller needs to fall back when none matches).
fn resolve_any(
    ph: &mut UPhoneme,
    singer: Option<&Voicebank>,
    tone: i32,
    candidates: &[String],
) -> bool {
    if let Some(vb) = singer {
        for candidate in candidates {
            if vb.lookup(candidate, tone).is_some() {
                ph.phoneme = candidate.clone();
                return true;
            }
        }
    }
    ph.phoneme = ph.raw_phoneme.clone();
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use voicebank::{parse_oto_ini, Voicebank};

    /// Minimal ja_vcv-style bank written as oto.ini TEXT (no wav is read):
    /// a whole-note VCV alias (`- sa`), the CV alias (`sa`) and the
    /// consonant/tail aliases (`s`, `- a`) the old split path used.
    fn jp_voicebank() -> Voicebank {
        let ini = concat!(
            "_sau+_sau+_s-.wav=- sa,80.0,251.664,-498.668,201.665,73.333\n",
            "_sau+_sau+_s-.wav=sa,736.336,185.666,-352.001,135.666,44.0\n",
            "_s3_s3_s-.wav=s,742.069,50.731,-122.467,0.731,27.133\n",
            "_sau+_sau+_s-.wav=- a,100.0,100.0,-275.001,1.994,10.0\n",
        );
        let parsed = parse_oto_ini(ini.as_bytes(), None);
        let oto_map: HashMap<String, _> =
            parsed.otos.into_iter().map(|o| (o.alias.clone(), o)).collect();
        Voicebank {
            name: "jp-test".into(),
            base_path: Default::default(),
            path: Default::default(),
            id: "jp-test".into(),
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
            otos: Vec::new(),
            oto_map,
            warnings: Vec::new(),
        }
    }

    fn note(lyric: &str) -> UNote {
        UNote {
            lyric: lyric.to_string(),
            duration: 1440,
            tone: 69,
            ..Default::default()
        }
    }

    /// Regression (T9): a single `sa` note used to be split into the
    /// consonant `s` and the tail `- a` with the note duration divided
    /// EVENLY — the vowel only started at 750 ms of a 1500 ms note, which is
    /// the reported "broken onset". OpenUtau's JapaneseVCVPhonemizer emits
    /// exactly ONE phoneme per note (see its Process(): a single Phoneme).
    #[test]
    fn one_phoneme_per_note_like_openutau() {
        let vb = jp_voicebank();
        let phs = JapaneseVcvPhonemizer.process(&[note("sa")], Some(&vb));
        assert_eq!(phs.len(), 1, "one phoneme per note, got {phs:?}");
        assert_eq!(phs[0].phoneme, "- sa", "VCV alias, as the reference picks it");
        assert_eq!(phs[0].position, 0);
        assert_eq!(phs[0].duration, 1440, "the phoneme owns the whole note");
    }

    /// The split path stays available for banks with no whole-lyric alias.
    #[test]
    fn falls_back_to_the_mora_split_without_a_whole_alias() {
        let mut vb = jp_voicebank();
        vb.oto_map.remove("- sa");
        vb.oto_map.remove("sa");
        let phs = JapaneseVcvPhonemizer.process(&[note("sa")], Some(&vb));
        assert!(phs.len() > 1, "expected the mora split, got {phs:?}");
    }
}

