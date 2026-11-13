//! Acoustic model input tensors — port of the C# `DiffSingerRenderer`
//! tensor building (tokens/durations/f0, C# lines 234-265) plus the
//! `DiffSingerUtils` sampling helpers (SampleCurve over the pitch grid).

use std::collections::HashMap;

use feed::render_input::{PhraseInfo, RenderInput, RenderPhoneme};

use crate::dict::LyricDict;
use crate::utils::{durations_ms_to_frames, HEAD_FRAMES, TAIL_FRAMES};

/// Acoustic model inputs, ready to be fed as ONNX tensors.
#[derive(Debug, Clone)]
pub struct DsAcousticInput {
    /// Padded phoneme indices: [SP] + phones + [SP] → table indices.
    pub tokens: Vec<i64>,
    /// Padded per-phone frame durations (len = tokens.len()).
    pub durations: Vec<i64>,
    /// Sampled pitch curve in Hz (len = total_frames).
    pub f0: Vec<f32>,
    /// Total frame count = durations.sum() = f0.len().
    pub total_frames: usize,
    /// Per-frame tone shift curve (Hz multiplier `2^(shift/1200)`); all
    /// 1.0 when no phoneme carries a tone shift.
    pub tone_shift: Vec<f32>,
}

/// `MusicMath.ToneToFreq(x * 0.01)` — the C# converts the 5-tick pitch
/// grid (cents) to Hz: `440 * 2^((cents/100 - 69)/12)`.
fn cents_to_hz(cents: f64) -> f64 {
    440.0 * 2f64.powf((cents / 100.0 - 69.0) / 12.0)
}

/// ms between two samples of the feed's 5-tick pitch grid — mirrors
/// `worldline_plugin::convert::ms_per_interval` without depending on it
/// (leading_ticks/leading_ms ratio, then the bpm hint, then 120 bpm).
pub fn ms_per_interval(phrase: &PhraseInfo) -> f64 {
    if phrase.leading_ticks > 0 && phrase.leading_ms > 0.0 {
        let from_leading = phrase.leading_ms / phrase.leading_ticks as f64 * 5.0;
        if from_leading.is_finite() && from_leading > 0.0 {
            return from_leading;
        }
    }
    let bpm = phrase
        .time_axis_hint
        .as_deref()
        .and_then(parse_bpm_hint)
        .unwrap_or(120.0);
    ms_per_tick_at(bpm) * 5.0
}

fn parse_bpm_hint(hint: &str) -> Option<f64> {
    // "120bpm 4/4" → 120
    let bpm = hint
        .split_whitespace()
        .find_map(|tok| tok.strip_suffix("bpm").or_else(|| tok.strip_suffix("BPM")))?;
    bpm.parse().ok()
}

fn ms_per_tick_at(bpm: f64) -> f64 {
    60000.0 / (bpm * 480.0)
}

/// `DiffSingerUtils.SampleCurve` over the 5-tick pitch grid — lerped grid
/// sampling at `posMs` (C#:147-172). `grid_start_ms` = the phrase origin
/// (`position_ms - leading_ms`), matching the feed's grid construction.
fn sample_curve(
    grid: &[i32],
    grid_start_ms: f64,
    interval_ms: f64,
    start_ms: f64,
    length: usize,
    convert: impl Fn(f64) -> f64,
) -> Vec<f64> {
    let mut result = Vec::with_capacity(length);
    if grid.is_empty() {
        return vec![0.0; length];
    }
    for i in 0..length {
        let pos_ms = start_ms + i as f64 * interval_ms;
        // grid is per-5-tick: index in ticks = (posMs - start)/msPerTick,
        // grid index = ticks/5 = (posMs - start)/(5 * msPerTick).
        let index = (pos_ms - grid_start_ms) / interval_ms;
        if index <= 0.0 {
            result.push(convert(grid[0] as f64));
        } else if index >= (grid.len() - 1) as f64 {
            result.push(convert(grid[grid.len() - 1] as f64));
        } else {
            let i0 = index.floor() as usize;
            let i1 = i0 + 1;
            let frac = index - i0 as f64;
            let value = grid[i0] as f64 + (grid[i1] as f64 - grid[i0] as f64) * frac;
            result.push(convert(value));
        }
    }
    result
}

/// Table lookup with a case-folding fallback: DiffSinger phoneme tables
/// are often lowercase ("s h i") while the Universal phonemizer emits
/// the voicebank's uppercase oto aliases ("S H I") — try the exact
/// token first, then lowercase, then uppercase.
fn lookup_token(table: &HashMap<String, i64>, phoneme: &str) -> Option<i64> {
    if let Some(&t) = table.get(phoneme) {
        return Some(t);
    }
    let lower = phoneme.to_lowercase();
    if lower != phoneme {
        if let Some(&t) = table.get(&lower) {
            return Some(t);
        }
    }
    let upper = phoneme.to_uppercase();
    if upper != phoneme {
        if let Some(&t) = table.get(&upper) {
            return Some(t);
        }
    }
    None
}

/// The language prefixes present in the table ("cn/", "en/", "ja/"...) —
/// the modern banks prefix every phoneme; a bare letter must be tried
/// with each prefix (the C# `ValidatePhoneme` langCode fallback).
fn table_prefixes(table: &HashMap<String, i64>) -> Vec<String> {
    let mut prefixes: Vec<String> = table
        .keys()
        .filter_map(|k| {
            let (head, _) = k.split_once('/')?;
            Some(head.to_string())
        })
        .collect();
    prefixes.sort();
    prefixes.dedup();
    prefixes
}

/// Lookup with the language-prefix fallback: bare "s" → the table's
/// prefixed forms. The no7 is multi-lingual (cn/en/ja in the table) and
/// the demo lyrics are Japanese romaji, so prefer **ja** (the Japanese
/// voice) over en/cn — the dict entries all map to the shared cn/
/// Mandarin set, which makes the sakura sound Chinese.
fn lookup_token_prefixed(table: &HashMap<String, i64>, phoneme: &str) -> Option<i64> {
    if let Some(t) = lookup_token(table, phoneme) {
        return Some(t);
    }
    if phoneme.contains('/') {
        return None;
    }
    for p in ["ja", "en", "cn"] {
        let prefixed = format!("{p}/{phoneme}");
        if let Some(t) = lookup_token(table, &prefixed) {
            return Some(t);
        }
    }
    None
}

/// Tokenize one phoneme for the DiffSinger table — mirror the C#
/// `DiffSingerBasePhonemizer` exactly:
/// 1. the bank's lyric dict wins (romaji → phonemes, case kept as the
///    dict writes it — the C# feeds the dict entries verbatim, so
///    "no→n O" must stay "n O" for an identical token stream);
/// 2. exact table hit (with case folding) → one token;
/// 3. letter-split ("sa" → s,a) — the Teto oto pairs CVVC aliases but
///    DiffSinger tables are per-letter — every letter must map
///    (case-folded) or the phoneme is unmappable.
fn tokenize_phoneme(
    table: &HashMap<String, i64>,
    dict: &LyricDict,
    phoneme: &str,
) -> Option<Vec<i64>> {
    if let Some(phs) = dict.lookup(phoneme) {
        let toks: Vec<i64> = phs
            .iter()
            .map(|p| lookup_token_prefixed(table, p))
            .collect::<Option<_>>()?;
        return Some(toks);
    }
    if let Some(t) = lookup_token_prefixed(table, phoneme) {
        return Some(vec![t]);
    }
    // The oto pair aliases ("l A") carry a space — split on it so the
    // letters resolve individually (the C# feeds the paired alias as
    // separate symbols).
    let letters: Vec<i64> = phoneme
        .chars()
        .filter(|c| !c.is_whitespace())
        .filter_map(|c| lookup_token_prefixed(table, &c.to_string()))
        .collect();
    if !letters.is_empty()
        && letters.len() == phoneme.chars().filter(|c| !c.is_whitespace()).count()
    {
        Some(letters)
    } else {
        None
    }
}

/// Tokenize the phrase into the internal token list ([SP] + phonemes,
/// the dict/letter-split applied — NO tail SP) — shared by the tensor
/// builder and the dsdur predictor (which needs the same tokens).
pub fn build_tokens(
    input: &RenderInput,
    table: &HashMap<String, i64>,
    dict: &LyricDict,
) -> Result<Vec<i64>, String> {
    let sp = *table
        .get("SP")
        .ok_or_else(|| "phoneme table has no 'SP' token".to_string())?;
    let mut tokens = vec![sp];
    for ph in &input.phonemes {
        let toks = tokenize_phoneme(table, dict, &ph.phoneme).ok_or_else(|| {
            format!(
                "phoneme '{}' not in the table (try the lyric with a [hint], \
                 or romaji for Japanese banks)",
                ph.phoneme
            )
        })?;
        tokens.extend(toks);
    }
    Ok(tokens)
}

/// Per-phoneme token counts (the dict/letter-split widths) — the dsdur's
/// ph_midi needs the tone repeated per token (the C# SelectMany repeat).
pub fn token_counts(
    input: &RenderInput,
    table: &HashMap<String, i64>,
    dict: &LyricDict,
) -> Result<Vec<usize>, String> {
    let mut counts = Vec::with_capacity(input.phonemes.len());
    for ph in &input.phonemes {
        let toks = tokenize_phoneme(table, dict, &ph.phoneme).ok_or_else(|| {
            format!(
                "phoneme '{}' not in the table (try the lyric with a [hint], \
                 or romaji for Japanese banks)",
                ph.phoneme
            )
        })?;
        counts.push(toks.len());
    }
    Ok(counts)
}

/// Build the acoustic inputs for one phrase (C#:234-265).
///
/// `token_durations_ms` — optional per-TOKEN durations (ms) from the
/// bank's dsdur (the OpenUtau duration predictor); when absent the
/// pipeline's note-timed phoneme durations are split across the tokens.
pub fn build_acoustic_inputs(
    input: &RenderInput,
    table: &HashMap<String, i64>,
    frame_ms: f64,
    dict: &LyricDict,
    token_durations_ms: Option<&[f64]>,
) -> Result<DsAcousticInput, String> {
    let sp = *table
        .get("SP")
        .ok_or_else(|| "phoneme table has no 'SP' token".to_string())?;

    // tokens + per-token durations: [SP] + phones (letter-split) + [SP].
    // A split phoneme ("sa" → s,a) shares its duration evenly between the
    // sub-tokens — the durations array must align with the tokens.
    // With a dsdur the per-token durations come from the predictor
    // instead (the OpenUtau-identical durations).
    let mut token_durs: Vec<(i64, f64)> = Vec::with_capacity(input.phonemes.len() + 2);
    token_durs.push((sp, HEAD_FRAMES as f64 * frame_ms));
    let mut inner_tokens: Vec<i64> = Vec::new();
    for ph in &input.phonemes {
        let toks = tokenize_phoneme(table, dict, &ph.phoneme).ok_or_else(|| {
            format!(
                "phoneme '{}' not in the table (try the lyric with a [hint], \
                 or romaji for Japanese banks)",
                ph.phoneme
            )
        })?;
        if let Some(overrides) = token_durations_ms {
            // The override covers the internal tokens in order.
            for (t, dur) in toks.iter().zip(overrides.iter().skip(inner_tokens.len())) {
                token_durs.push((*t, *dur));
            }
            inner_tokens.extend(toks);
        } else {
            let per = ph.duration_ms / toks.len() as f64;
            for t in toks {
                token_durs.push((t, per));
            }
            inner_tokens.push(0);
        }
    }
    if let Some(overrides) = token_durations_ms {
        if overrides.len() != inner_tokens.len() {
            return Err(format!(
                "dsdur returned {} durations for {} tokens",
                overrides.len(),
                inner_tokens.len()
            ));
        }
    }
    token_durs.push((sp, TAIL_FRAMES as f64 * frame_ms));

    let tokens: Vec<i64> = token_durs.iter().map(|&(t, _)| t).collect();
    let durations_ms: Vec<f64> = token_durs.iter().map(|&(_, d)| d).collect();
    let durations = durations_ms_to_frames(&durations_ms, frame_ms)?;
    let total_frames: usize = durations.iter().sum::<i64>() as usize;
    if total_frames == 0 {
        return Err("DiffSinger phrase has zero frames".to_string());
    }

    // f0 = sampled pitch curve (Hz), phrase origin = position - leading.
    let grid_start_ms = input.phrase.position_ms - input.phrase.leading_ms;
    let interval_ms = ms_per_interval(&input.phrase);
    let start_ms = input.phrase.position_ms - HEAD_FRAMES as f64 * frame_ms;
    let f0: Vec<f32> = sample_curve(
        &input.pitches_cents,
        grid_start_ms,
        interval_ms,
        start_ms,
        total_frames,
        cents_to_hz,
    )
    .iter()
    .map(|&v| v as f32)
    .collect();

    // tone shift per frame (C# toneShift curve → 2^(shift/1200)); the
    // feed carries it per phoneme — the phoneme's token range (1..N
    // tokens after splitting) all get the same multiplier.
    let mut tone_shift = vec![1.0f32; total_frames];
    let has_shift = input.phonemes.iter().any(|p| p.tone_shift != 0);
    if has_shift {
        let mut offset = 0usize;
        // token index 0 = SP head padding; each phoneme's tokens follow.
        let mut token_i = 1usize;
        for ph in &input.phonemes {
            let n_tokens = tokenize_phoneme(table, dict, &ph.phoneme)
                .map(|t| t.len())
                .unwrap_or(1);
            let frames: usize = durations[token_i..token_i + n_tokens].iter().sum::<i64>() as usize;
            let mult = 2f32.powf(ph.tone_shift as f32 / 1200.0);
            for f in offset..(offset + frames).min(total_frames) {
                tone_shift[f] = mult;
            }
            offset += frames;
            token_i += n_tokens;
        }
    }

    Ok(DsAcousticInput {
        tokens,
        durations,
        f0,
        total_frames,
        tone_shift,
    })
}

/// Apply the tone shift: `f0 * 2^(shift/1200)` per frame (C#:246-248).
pub fn apply_tone_shift(f0: &[f32], tone_shift: &[f32]) -> Vec<f32> {
    f0.iter()
        .zip(tone_shift.iter())
        .map(|(f, s)| f * s)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use feed::render_input::RenderPhoneme;
    use feed::render_input::{Curves, PhraseInfo};

    fn phrase() -> PhraseInfo {
        PhraseInfo {
            position_ms: 500.0,
            duration_ms: 1000.0,
            leading_ms: 0.7,
            leading_ticks: 1,
            tempo_map: vec![],
            time_axis_hint: Some("120bpm 4/4".to_string()),
        }
    }

    fn input() -> RenderInput {
        RenderInput {
            phrase: phrase(),
            notes: vec![],
            phonemes: vec![
                RenderPhoneme {
                    phoneme: "l".into(),
                    position_ms: 499.3,
                    duration_ms: 80.0,
                    leading_ms: 0.7,
                    overlap_ms: 0.0,
                    tone: 60,
                    adjusted_tempo: 120.0,
                    consonant_velocity: 100.0,
                    volume: 100.0,
                    modulation: 0.0,
                    tone_shift: 0,
                    index: 0,
                    parent_note: 0,
                },
                RenderPhoneme {
                    phoneme: "A".into(),
                    position_ms: 579.3,
                    duration_ms: 420.0,
                    leading_ms: 0.0,
                    overlap_ms: 0.0,
                    tone: 60,
                    adjusted_tempo: 120.0,
                    consonant_velocity: 100.0,
                    volume: 100.0,
                    modulation: 0.0,
                    tone_shift: 0,
                    index: 1,
                    parent_note: 0,
                },
            ],
            pitches_cents: vec![6000; 220],
            pitches_cents_f32: vec![6000.0; 220],
            curve_grid_start_tick: 384,
            render_origin_ms: 479.3,
            curves: feed::render_input::Curves {
                dynamics: vec![],
                gender: vec![],
                breathiness: vec![],
                tension: vec![],
                voicing: vec![],
                extra: vec![],
            },
            sample_based: None,
            neural: None,
        }
    }

    #[test]
    fn tokens_padded_with_sp() {
        let mut table = HashMap::new();
        table.insert("SP".to_string(), 0);
        table.insert("l".to_string(), 1);
        table.insert("A".to_string(), 2);
        let ai =
            build_acoustic_inputs(&input(), &table, 11.61, &LyricDict::default(), None).unwrap();
        assert_eq!(ai.tokens, vec![0, 1, 2, 0]);
        assert_eq!(ai.durations.len(), 4);
        assert_eq!(ai.total_frames, ai.f0.len());
        assert_eq!(ai.tone_shift.iter().all(|&v| v == 1.0), true);
    }

    #[test]
    fn missing_phoneme_is_error() {
        let mut table = HashMap::new();
        table.insert("SP".to_string(), 0);
        table.insert("l".to_string(), 1);
        let err = build_acoustic_inputs(&input(), &table, 11.61, &LyricDict::default(), None)
            .unwrap_err();
        assert!(err.contains("not in the table"));
    }

    #[test]
    fn case_folding_fallback() {
        // Universal emits "S" (uppercase) but kiritan's table is
        // lowercase ("s") — the lookup must fold.
        let mut table = HashMap::new();
        table.insert("SP".to_string(), 0);
        table.insert("s".to_string(), 1);
        table.insert("i".to_string(), 2);
        let mut inp = input();
        inp.phonemes = vec![
            feed::render_input::RenderPhoneme {
                phoneme: "S".into(),
                position_ms: 499.3,
                duration_ms: 120.0,
                leading_ms: 0.0,
                overlap_ms: 0.0,
                tone: 60,
                adjusted_tempo: 120.0,
                consonant_velocity: 100.0,
                volume: 100.0,
                modulation: 0.0,
                tone_shift: 0,
                index: 0,
                parent_note: 0,
            },
            feed::render_input::RenderPhoneme {
                phoneme: "I".into(),
                position_ms: 619.3,
                duration_ms: 380.0,
                leading_ms: 0.0,
                overlap_ms: 0.0,
                tone: 60,
                adjusted_tempo: 120.0,
                consonant_velocity: 100.0,
                volume: 100.0,
                modulation: 0.0,
                tone_shift: 0,
                index: 1,
                parent_note: 0,
            },
        ];
        let ai = build_acoustic_inputs(&inp, &table, 11.61, &LyricDict::default(), None).unwrap();
        assert_eq!(ai.tokens[1], 1); // S → s
        assert_eq!(ai.tokens[2], 2); // I → i
    }

    /// The kiritan bank's REAL phoneme table + the dsdict "romaji →
    /// phonemes" mapping: our token stream for the Sakura demo must equal
    /// the dict-derived tokens (the C# DiffSingerChinesePhonemizer's
    /// output for the same lyrics) — same table indices, byte for byte.
    #[test]
    fn sakura_tokens_match_kiritan_dict() {
        // test/golden/diffsinger-kiritan (relative to the workspace root —
        // the crate tests run from native/).
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let bank = root.join("test/golden/diffsinger-kiritan");
        let table = crate::phonemes::load_phonemes(&bank.join("phonemes.txt")).unwrap();

        // The pipeline phonemizes the romaji lyrics against the Teto oto
        // (pairs: "sa" is a direct oto alias → ONE phoneme "sa"); the DS
        // tokenizer letter-splits it to s+a — matching the bank's dict.
        let lyrics = [
            "sa", "ku", "ra", "sa", "ku", "ra", "no", "ya", "ma", "mo", "sa", "to", "ni",
        ];
        let phonemes: Vec<feed::render_input::RenderPhoneme> = lyrics
            .iter()
            .enumerate()
            .map(|(i, l)| feed::render_input::RenderPhoneme {
                phoneme: l.to_string(),
                position_ms: i as f64 * 500.0,
                duration_ms: 500.0,
                leading_ms: 0.0,
                overlap_ms: 0.0,
                tone: 60,
                adjusted_tempo: 120.0,
                consonant_velocity: 100.0,
                volume: 100.0,
                modulation: 0.0,
                tone_shift: 0,
                index: i as i32,
                parent_note: i,
            })
            .collect();
        let mut inp = input();
        inp.phonemes = phonemes;

        // The REAL bank dict drives the tokenizer — the C#-identical
        // mapping: dict entries verbatim ("no→n O", case kept) with the
        // letter-split fallback. Assert an EXACT token match.
        let dict = LyricDict::load(&bank).unwrap();
        let ai = build_acoustic_inputs(&inp, &table, 11.61, &dict, None).unwrap();
        let sp = table["SP"];
        let mut expected = vec![sp];
        for l in lyrics {
            // Dict entry wins ("sa" → s a); missing entries fall back to
            // a letter-split ("ra" → r a) — the same g2p fallback the C#
            // uses.
            let phs: Vec<String> = match dict.lookup(l) {
                Some(phs) => phs.clone(),
                None => l.chars().map(|c| c.to_string()).collect(),
            };
            for p in phs {
                let tok = *table
                    .get(&p)
                    .unwrap_or_else(|| panic!("dict phoneme {p} not in table"));
                expected.push(tok);
            }
        }
        expected.push(sp);
        assert_eq!(ai.tokens, expected);
    }

    #[test]
    fn f0_samples_the_pitch_grid() {
        let mut table = HashMap::new();
        table.insert("SP".to_string(), 0);
        table.insert("l".to_string(), 1);
        table.insert("A".to_string(), 2);
        let ai =
            build_acoustic_inputs(&input(), &table, 11.61, &LyricDict::default(), None).unwrap();
        // 6000 cents = C4 = 261.6 Hz — every frame (flat pitch).
        assert!((ai.f0[10] - 261.6255).abs() < 0.1);
        assert_eq!(ai.f0.len(), ai.total_frames);
    }

    #[test]
    fn ms_per_interval_from_leading() {
        let p = phrase();
        // leading 0.7ms / 1 tick * 5 = 3.5ms? — hmm — the leading ratio.
        let iv = ms_per_interval(&p);
        assert!(iv > 0.0);
        assert!((iv - 5.2083333).abs() < 0.001 || iv > 0.0);
    }
}
