//! L2 — IPA → voicebank adapter + capability manager.
//!
//! [`IpaAdapter`] maps universal IPA tokens (from the L1 layer) to the
//! target voicebank's aliases through its [`PhonemeManifest`] (L3),
//! using [`NearestPhoneme`] when the bank lacks a phoneme.
//!
//! [`CapabilityManager`] composes the voicebank manifest with the
//! engine's capabilities — queried through the engine-agnostic
//! `domain::EngineCapabilities` trait, never a concrete engine type.

use domain::capabilities::EngineCapabilities;
use voicebank::manifest::{PhonemeManifest, PhonemeType};

use crate::ipa::IpaDictionary;

/// Map a missing phoneme to the closest alias the bank has.
///
/// Feature vectors per IPA symbol (from the L1 dict) + weighted distance.
/// Only same-type candidates (vowel→vowel, consonant→consonant) are
/// considered, and a distance threshold guards against wrong mappings
/// (e.g. "v"→"b" is close but wrong — allowed only when nothing better).
pub struct NearestPhoneme<'a> {
    manifest: &'a PhonemeManifest,
}

impl<'a> NearestPhoneme<'a> {
    pub fn new(manifest: &'a PhonemeManifest) -> Self {
        NearestPhoneme { manifest }
    }

    /// Closest bank alias for `ipa_token` (an IPA string like `/r/`), or
    /// `None` when nothing is close enough. Existing aliases return
    /// themselves (no remap of renderable phonemes).
    pub fn nearest(&self, ipa_token: &str) -> Option<String> {
        // 1. Exact alias already exists → no mapping needed.
        let alias = ipa_to_alias(ipa_token);
        if self.manifest.can_render(&alias) {
            return Some(alias);
        }
        // 2. Nearest by feature vector (same type only).
        let want = feature_of(ipa_token)?;
        let mut best: Option<(f64, String)> = None;
        for candidate in &self.manifest.phoneme_set {
            if self.manifest.types.get(candidate) != Some(&want.ty) {
                continue;
            }
            let Some(cand) = feature_of(candidate) else { continue };
            let d = want.distance(&cand);
            if d <= THRESHOLD && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, candidate.clone()));
            }
        }
        best.map(|(_, alias)| alias)
    }
}

/// Distance threshold — phonemes farther apart than this are NOT mapped
/// (the caller falls back to the lyric verbatim). Prevents v→b style
/// wrong mappings when no close vowel/consonant exists.
const THRESHOLD: f64 = 1.0;

/// ARPABET alias for an IPA token: strips the `/` slashes. IPA and
/// ARPABET share the same alias space in the Teto banks ("/r/" → "r").
fn ipa_to_alias(ipa_token: &str) -> String {
    ipa_token.trim_matches('/').to_string()
}

/// Feature vector of a phoneme (IPA or alias): used for nearest mapping.
#[derive(Debug, Clone, Copy)]
struct Features {
    ty: PhonemeType,
    /// Vowel: [height, backness, rounding]; consonant: [place, manner,
    /// voicing]. Distance is weighted Euclidean on these dims.
    v: [f64; 3],
}

impl Features {
    fn distance(&self, other: &Features) -> f64 {
        let d = self.v[0] - other.v[0];
        let d1 = self.v[1] - other.v[1];
        let d2 = self.v[2] - other.v[2];
        (d * d + d1 * d1 + d2 * d2).sqrt()
    }
}

fn feature_of(token: &str) -> Option<Features> {
    let t = token.trim_matches('/');
    let ty = if VOWELS.contains(&t) {
        PhonemeType::Vowel
    } else if GLIDES.contains(&t) {
        PhonemeType::Glide
    } else {
        PhonemeType::Consonant
    };
    // Vowel feature table: [height(0-3), backness(0-3), rounding(0/1)]
    let v = match t {
        "a" | "A" => Some([2.0, 2.0, 0.0]),
        "3" => Some([1.0, 1.0, 0.0]),
        "i" | "I" => Some([3.0, 0.0, 0.0]),
        "e" | "E" => Some([2.0, 1.0, 0.0]),
        "u" | "U" => Some([3.0, 3.0, 1.0]),
        "o" | "O" => Some([2.0, 3.0, 1.0]),
        "aI" | "aU" | "eI" | "oU" => Some([2.0, 2.0, 0.0]),
        // Consonant feature table: [place(0-7), manner(0-6), voicing]
        "l" => Some([4.0, 3.0, 1.0]), // alveolar lateral
        "r" | "r3" => Some([4.0, 4.0, 1.0]), // alveolar approx
        "n" => Some([4.0, 5.0, 1.0]), // alveolar nasal
        "m" => Some([3.0, 5.0, 1.0]), // bilabial nasal
        "b" => Some([3.0, 0.0, 1.0]), // bilabial stop
        "p" => Some([3.0, 0.0, 0.0]),
        "d" => Some([4.0, 0.0, 1.0]),
        "t" => Some([4.0, 0.0, 0.0]),
        "g" => Some([5.0, 0.0, 1.0]),
        "k" => Some([5.0, 0.0, 0.0]),
        "s" => Some([4.0, 2.0, 0.0]),
        "z" => Some([4.0, 2.0, 1.0]),
        "f" => Some([2.0, 2.0, 0.0]),
        "v" => Some([2.0, 2.0, 1.0]),
        "S" => Some([6.0, 2.0, 0.0]),
        "Z" => Some([6.0, 2.0, 1.0]),
        "h" => Some([7.0, 2.0, 0.0]),
        "j" => Some([5.0, 4.0, 1.0]),
        "w" => Some([1.0, 4.0, 1.0]),
        _ => None,
    }?;
    Some(Features { ty, v })
}

const VOWELS: &[&str] = &["a", "A", "3", "i", "I", "e", "E", "u", "U", "o", "O"];
const GLIDES: &[&str] = &["j", "w", "y"];

/// L2 adapter: IPA tokens → bank aliases through the manifest.
pub struct IpaAdapter<'a> {
    manifest: &'a PhonemeManifest,
    dict: &'a IpaDictionary,
    nearest: NearestPhoneme<'a>,
}

impl<'a> IpaAdapter<'a> {
    pub fn new(manifest: &'a PhonemeManifest, dict: &'a IpaDictionary) -> Self {
        IpaAdapter { manifest, dict, nearest: NearestPhoneme::new(manifest) }
    }

    /// The manifest this adapter maps against (the universal phonemizer
    /// consults it for direct-alias lookups).
    pub fn manifest(&self) -> &'a PhonemeManifest {
        self.manifest
    }

    /// Map a sequence of IPA tokens to bank aliases. Missing phonemes
    /// fall back to nearest; when even that fails the token passes
    /// through verbatim (the caller can still fall back to the lyric).
    pub fn map_ipa(&self, ipa_tokens: &[&str]) -> Vec<String> {
        ipa_tokens
            .iter()
            .map(|tok| {
                // Bridge IPA → ARPABET alias space via the dict; unknown
                // tokens fall back to the trimmed token verbatim.
                let alias = self
                    .dict
                    .arpabet(tok)
                    .unwrap_or(tok.trim_matches('/'))
                    .to_string();
                if self.manifest.can_render(&alias) {
                    alias
                } else if let Some(n) = self.nearest.nearest(&alias) {
                    n
                } else {
                    alias
                }
            })
            .collect()
    }
}

/// Composes the voicebank manifest with the engine's capabilities.
/// Engine-agnostic: the engine is only consulted through
/// `&dyn EngineCapabilities`.
pub struct CapabilityManager<'a> {
    manifest: &'a PhonemeManifest,
    engine: &'a dyn EngineCapabilities,
}

impl<'a> CapabilityManager<'a> {
    pub fn new(manifest: &'a PhonemeManifest, engine: &'a dyn EngineCapabilities) -> Self {
        CapabilityManager { manifest, engine }
    }

    pub fn can_render(&self, phoneme: &str) -> bool {
        self.manifest.can_render(phoneme)
    }

    pub fn missing<'b>(&self, phonemes: &[&'b str]) -> Vec<&'b str> {
        self.manifest.missing(phonemes)
    }

    pub fn needs_oto(&self) -> bool {
        self.engine.needs_oto()
    }

    pub fn engine_id(&self) -> &'static str {
        self.engine.id()
    }
}
