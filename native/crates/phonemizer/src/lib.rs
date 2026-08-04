//! Phonemizer framework (Sprint 1.3).
//!
//! Turns note lyrics into phoneme sequences ready for rendering, mirroring
//! the OpenUtau phonemizer pipeline:
//!
//! 1. [`Phonemizer`] implementations convert `UNote`s (lyric + phonetic
//!    hint + tone) into tick-based [`Phoneme`]s (the domain `UPhoneme`).
//!    Positions are **relative to the parent note**, exactly like OpenUtau
//!    plugin phonemizers output.
//! 2. [`g2p`] provides the grapheme-to-phoneme base: a trie-backed
//!    dictionary (`lyric` → phoneme sequence), phonetic-hint parsing
//!    (`"read[r iy d]"`), symbol classification (vowel/glide/valid) and
//!    the standard `SP` (silence) / `AP` (breath) symbols.
//! 3. [`JapaneseVcvPhonemizer`] splits kana lyrics into morae and emits
//!    `[CV, tail-vowel]` pairs (`か` → `[か, a]`), resolving VCV aliases
//!    against the singer's oto table (`a き`, `- か`, ...).
//! 4. [`EnglishCvvcPhonemizer`] turns a phonetic hint (or a G2p dictionary
//!    lookup) into CV/CVVC aliases by greedy pairing against the oto table,
//!    falling back to the hint verbatim when no alias matches.
//! 5. [`TimingEngine`] converts the tick-based phonemes to part-relative
//!    ticks and computes `position_ms` / `duration_ms` via the project's
//!    `TimeAxis`, plus `leading_ms` / `overlap_ms` (preutter / overlap)
//!    from the oto entry of each resolved phoneme.
//!
//! This mirrors `OpenUtau.Core/Api/Phonemizer.cs`, `IG2p.cs`,
//! `G2pDictionary.cs`, the builtin `JapaneseVCVPhonemizer` /
//! `EnglishVCCVPhonemizer` plugins and the classic oto-based timing in
//! `OpenUtau.Core/Classic/ClassicSinger.cs`.

pub mod english;
pub mod g2p;
pub mod japanese;
pub mod lilt_dict;
pub mod phonemizer;
pub mod timing;
pub mod translit;

pub use domain::UPhoneme as Phoneme;
pub use english::EnglishCvvcPhonemizer;
pub use g2p::{G2p, G2pDictionary, G2pFallbacks, AP, SP};
pub use japanese::{split_morae, tail_vowel, JapaneseVcvPhonemizer};
pub use phonemizer::{make_phoneme, Phonemizer};
pub use timing::TimingEngine;
