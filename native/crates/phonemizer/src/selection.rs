//! Which phonemizer derives a track's phonemes — the one place that decides.
//!
//! This used to live in `synth-cli`'s `pipeline` module, which made the rule
//! unreachable to every other consumer: `project-session` and any future host
//! had to depend on a **CLI binary crate** to learn which phonemizer a track
//! selects. `engine-wasm` did exactly that (`synth-cli = { path = ... }`).
//!
//! Selecting a phonemizer is musical state, so it belongs beside the phonemizers
//! themselves. Everything that needs the answer now asks the `phonemizer` crate.
//!
//! [`PhonemizerKind::from_track`] mirrors OpenUtau's own resolution: a `.ustx`
//! stores the *registered phonemizer key* (e.g.
//! `OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer`), and OpenUtau looks that
//! key up in its factory table. The key list below is taken from the checked-in
//! reference sources — `ref-openutau-clean/core/OpenUtau.Core` and
//! `ref-openutau-clean/desktop-ref/OpenUtau.Plugin.Builtin` — not invented.

use domain::UTrack;

/// Which phonemizer derives the phonemes of a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhonemizerKind {
    /// Latin/CVVC path with a G2p dictionary (`EnglishCvvcPhonemizer`).
    English,
    /// Japanese VCV mora path (`JapaneseVcvPhonemizer`).
    Japanese,
    /// IPA-first (L1+L2): detect → IPA → voicebank aliases via the manifest.
    /// Handles English AND Japanese through the same path.
    Universal,
    /// OpenUtau `DefaultPhonemizer` parity: exactly ONE phoneme per note — the
    /// lyric verbatim, tone-mapped to an oto alias — and a note whose alias does
    /// not exist emits NO phoneme (it is dropped instead of producing an
    /// unresolvable phoneme). See `default.rs`.
    Default,
}

/// Keys OpenUtau writes into `.ustx`, mapped to the implementation we run.
///
/// These are the exact registered keys from the reference sources. A project
/// saved by OpenUtau carries one of these strings verbatim, so an exact match is
/// the common case and is checked first; the substring pass in
/// [`PhonemizerKind::from_track`] is only a fallback for hand-edited or
/// user-typed names.
///
/// `English` is deliberately absent: OpenUtau has no single "English" key —
/// it has Arpasing, C+V, VCCV, X-SAMPA, and the EN→JA bridges, which are
/// different algorithms, and our legacy English CVVC path skips whole phrases on
/// unknown words. Those names all fall through to [`PhonemizerKind::Universal`].
const EXACT: &[(&str, PhonemizerKind)] = &[
    // OpenUtau.Core — `[Phonemizer("Default Phonemizer")]`
    ("OpenUtau.Core.DefaultPhonemizer", PhonemizerKind::Default),
    // Japanese VCV family. `JapanesePresampPhonemizer` is what the real
    // `.ustx` fixtures select, and it is the VCV algorithm we implement.
    (
        "OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer",
        PhonemizerKind::Japanese,
    ),
    (
        "OpenUtau.Plugin.Builtin.JapaneseCVVCPhonemizer",
        PhonemizerKind::Japanese,
    ),
    (
        "OpenUtau.Plugin.Builtin.JapaneseVccvPhonemizer",
        PhonemizerKind::Japanese,
    ),
];

/// Whether a lowercased phonemizer name refers to the Japanese path.
///
/// The `ja` test is a WHOLE-TOKEN match on purpose. OpenUtau's registered
/// aliases are language-prefixed (`"JA CVVC"`, `"DIFFS JA"`, `"S-VOICEVOX JA"`),
/// but a bare `contains("ja")` would also swallow `"Filipino VCV & CVVC"` and
/// `"EN to JA"` — both of which are not the Japanese VCV algorithm.
fn is_japanese_name(lower: &str) -> bool {
    // Note: a bare "vcv" is deliberately NOT a marker. It is ambiguous —
    // OpenUtau registers Filipino as `"FIL VCV & CVVC"` — and every real
    // Japanese key carries "japanese", "presamp", or a `ja` language token.
    // A name of just `"vcv"` falls through to Universal, which delegates kana
    // to the VCV phonemizer anyway.
    if lower.contains("japanese") || lower.contains("presamp") {
        return true;
    }
    // `ja` as a standalone token, plus the hyphenated namespace form
    // (`ja-cvvc`), but not `...japanese` (already handled) or `fijian`.
    let bytes = lower.as_bytes();
    let mut start = 0;
    while let Some(found) = lower[start..].find("ja") {
        let at = start + found;
        let before_ok = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let after = at + 2;
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = at + 2;
    }
    false
}

impl PhonemizerKind {
    /// Pick the phonemizer a track asks for.
    ///
    /// Resolution order, and why:
    ///
    /// 1. `RUSTED_UTAU_PHONEMIZER=default` — an explicit A/B override that beats the
    ///    track, kept from T12 so parity runs are reproducible.
    /// 2. An exact OpenUtau registered key (case-insensitive).
    /// 3. A substring match, so `"Japanese VCV"` or a bare `"japanese"` still
    ///    resolves. Order matters: `default` is checked before the catch-all so
    ///    an opt-in is never swallowed by it.
    /// 4. No phonemizer on the track, or an unrecognised name → `Universal`,
    ///    which is the one path that handles every lyric.
    ///
    /// `English` is deliberately NOT reachable from a track name — see the
    /// comment at the catch-all. It remains reachable through the explicit
    /// `--phonemizer english` CLI flag.
    pub fn from_track(track: &UTrack) -> Self {
        // Opt-in override (T12), e.g. for A/B against OpenUtau: the env var
        // beats the track setting, and the DEFAULT stays Universal.
        if std::env::var("RUSTED_UTAU_PHONEMIZER")
            .map(|v| v.eq_ignore_ascii_case("default"))
            .unwrap_or(false)
        {
            return PhonemizerKind::Default;
        }
        let Some(name) = track.phonemizer.as_deref() else {
            return PhonemizerKind::Universal;
        };
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return PhonemizerKind::Universal;
        }

        // 2. Exact registered key, compared case-insensitively because OpenUtau
        //    writes the key with its namespace casing but hand-edited projects
        //    do not always.
        for (key, kind) in EXACT {
            if trimmed.eq_ignore_ascii_case(key) {
                return *kind;
            }
        }

        // 3. Substring fallback, checked in a fixed order so the result does not
        //    depend on the order of a table.
        //
        //    Japanese is tested FIRST, and on the `ja` language token as well as
        //    the words. OpenUtau registers its phonemizers as
        //    `[Phonemizer(name, alias, …, language: "JA")]`, so the short alias
        //    a user is likely to type is `"JA CVVC"` — which contains "cvvc" and
        //    no "japanese". Testing English first sent it down the English path.
        let lower = trimmed.to_ascii_lowercase();
        if is_japanese_name(&lower) {
            PhonemizerKind::Japanese
        } else if lower.contains("default") {
            PhonemizerKind::Default
        } else {
            // 4. Everything else — including every English-declared name — is
            //    Universal. This is DELIBERATE and predates this module: the
            //    legacy English CVVC path falls back to the lyric VERBATIM on an
            //    unknown word, so "shi" finds no oto alias and the WHOLE phrase
            //    is skipped (a song renders only its first phrase). Universal
            //    letter-splits and NearestPhoneme-maps unknown words, so it
            //    always produces something.
            //
            //    Routing English here would silently bring that failure back.
            //    `PhonemizerKind::English` stays reachable ONLY through the
            //    explicit `--phonemizer english` CLI flag, where the caller has
            //    asked for that algorithm by name and gets it.
            //
            //    Pinned by `synth-cli`'s `phonemizer_kind_from_track_name`.
            PhonemizerKind::Universal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_with(name: Option<&str>) -> UTrack {
        let mut track = UTrack::default();
        track.phonemizer = name.map(str::to_string);
        track
    }

    #[test]
    fn a_track_with_no_phonemizer_is_universal() {
        assert_eq!(
            PhonemizerKind::from_track(&track_with(None)),
            PhonemizerKind::Universal
        );
    }

    #[test]
    fn an_empty_or_blank_name_is_universal() {
        // A `.ustx` round-trip can leave `phonemizer: ` empty; that is "unset",
        // not a request for a nameless phonemizer.
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some(""))),
            PhonemizerKind::Universal
        );
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("   "))),
            PhonemizerKind::Universal
        );
    }

    #[test]
    fn real_ustx_keys_resolve_by_exact_match() {
        // Both strings are lifted verbatim from the checked-in sample projects in
        // `test-ustx-example/`, so this is the case that actually ships.
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some(
                "OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer"
            ))),
            PhonemizerKind::Japanese
        );
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("OpenUtau.Core.DefaultPhonemizer"))),
            PhonemizerKind::Default
        );
    }

    #[test]
    fn exact_match_ignores_case_but_not_the_namespace() {
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("openutau.core.defaultphonemizer"))),
            PhonemizerKind::Default
        );
        // Not an OpenUtau key at all — falls through to the substring pass,
        // which still recognises "default".
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("MyBank.DefaultPhonemizer"))),
            PhonemizerKind::Default
        );
    }

    #[test]
    fn an_english_name_still_routes_to_universal_not_the_english_path() {
        // Deliberate, and easy to break by "helpfully" honouring the name: the
        // legacy English CVVC path falls back to the lyric verbatim, so an
        // unknown word ("shi") yields no oto alias and the WHOLE phrase is
        // skipped. Universal always produces phonemes.
        for name in [
            "english",
            "English VCCV",
            "EnglishVCCVPhonemizer",
            "English Arpasing Phonemizer",
            "FIL VCV & CVVC",
        ] {
            assert_eq!(
                PhonemizerKind::from_track(&track_with(Some(name))),
                PhonemizerKind::Universal,
                "{name} must not select the phrase-skipping English CVVC path"
            );
        }
    }

    #[test]
    fn japanese_variants_still_resolve() {
        for name in [
            "japanese",
            "Japanese VCV",
            "JapanesePresampPhonemizer",
            "Builtin.JapaneseCVVCPhonemizer",
            // Real registered aliases from the OpenUtau sources: the short
            // alias a user actually types is language-prefixed.
            "JA CVVC",
            "DIFFS JA",
            "S-VOICEVOX JA",
            "ja-vcvc",
        ] {
            assert_eq!(
                PhonemizerKind::from_track(&track_with(Some(name))),
                PhonemizerKind::Japanese,
                "{name} must resolve to the Japanese VCV path"
            );
        }
    }

    #[test]
    fn a_ja_substring_inside_another_language_is_not_japanese() {
        // "Filipino" contains "ji" and its registered alias starts "FIL VCV";
        // treating either as Japanese would send a Filipino bank down the
        // Japanese VCV path and silently change what it sings.
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("Filipino Phonemizer"))),
            PhonemizerKind::Universal
        );
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("FIL VCV & CVVC"))),
            PhonemizerKind::Universal
        );
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("Fijian Phonemizer"))),
            PhonemizerKind::Universal
        );
    }

    #[test]
    fn default_is_checked_before_the_other_substrings() {
        // "default" must win even though the name also contains no other hint —
        // pinned because the fallback order is otherwise free to drift.
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("default"))),
            PhonemizerKind::Default
        );
    }

    #[test]
    fn an_unrecognised_name_falls_back_to_universal() {
        // Universal is the path that handles every lyric, so an unknown name
        // must still render rather than fail.
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("Enunu Onnx Phonemizer"))),
            PhonemizerKind::Universal
        );
        assert_eq!(
            PhonemizerKind::from_track(&track_with(Some("Cantonese Syo-Style Phonemizer"))),
            PhonemizerKind::Universal
        );
    }
}
