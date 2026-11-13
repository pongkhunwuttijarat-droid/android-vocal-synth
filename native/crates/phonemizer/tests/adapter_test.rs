//! L2 adapter tests — IPA tokens → voicebank aliases via the manifest.
//!
//! Task 4: IpaAdapter maps universal IPA tokens to the bank's aliases
//! (through PhonemeManifest + CapabilityManager), assembling CVVC-style
//! pairs; NearestPhoneme fills the gap when the bank lacks a phoneme.
//!
//! Task 4.5: CapabilityManager composes the voicebank manifest with the
//! engine's capabilities (EngineCapabilities trait — engine-agnostic).

use phonemizer::adapter::{IpaAdapter, NearestPhoneme};
use phonemizer::ipa::IpaDictionary;
use voicebank::load_voicebank;
use voicebank::manifest::PhonemeManifest;

fn teto_manifest() -> PhonemeManifest {
    let lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../test/golden/teto-english/library");
    load_voicebank(&lib)
        .expect("load voicebank")
        .phoneme_manifest()
}

#[test]
fn adapter_maps_ipa_tokens_to_existing_aliases() {
    let manifest = teto_manifest();
    let dict = IpaDictionary::en_subset();
    let adapter = IpaAdapter::new(&manifest, &dict);

    // IPA sequence from "la[l A]": /l/ /ɑ/ → aliases l, A (both exist).
    let aliases = adapter.map_ipa(&["/l/", "/ɑ/"]);
    assert_eq!(aliases, vec!["l".to_string(), "A".to_string()]);
}

#[test]
fn adapter_uses_nearest_when_phoneme_missing() {
    let manifest = teto_manifest();
    let dict = IpaDictionary::en_subset();
    let adapter = IpaAdapter::new(&manifest, &dict);

    // /r/ is NOT in the bank as a single alias (it is "r3"); the nearest
    // mapper should land on r3 (distance ≈ 0 for rhotic).
    let aliases = adapter.map_ipa(&["/r/"]);
    assert_eq!(aliases, vec!["r3".to_string()], "expected r→r3 nearest");
}

#[test]
fn nearest_known_cases() {
    let manifest = teto_manifest();
    let nn = NearestPhoneme::new(&manifest);

    // a → A (same vowel quality), r → r3 (rhotic approx), both d≈0.
    assert_eq!(nn.nearest("a"), Some("A".to_string()));
    assert_eq!(nn.nearest("r"), Some("r3".to_string()));
    // Existing phonemes are never remapped.
    assert_eq!(nn.nearest("v"), Some("v".to_string()));
}

#[test]
fn nearest_unknown_stays_none() {
    let manifest = teto_manifest();
    let nn = NearestPhoneme::new(&manifest);
    // A phoneme with no close match in the bank must NOT be mapped
    // (threshold guard) — the caller falls back to the lyric verbatim.
    assert_eq!(nn.nearest("R"), None);
}

#[test]
fn capability_manager_is_engine_agnostic() {
    // Compile-time guarantee: CapabilityManager takes a
    // `&dyn EngineCapabilities` (domain trait) — never a concrete
    // engine type. A minimal test double stands in for worldline.
    use domain::capabilities::EngineCapabilities;
    use phonemizer::adapter::CapabilityManager;

    struct FakeEngine;
    impl EngineCapabilities for FakeEngine {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn needs_wav_samples(&self) -> bool {
            true
        }
        fn needs_oto(&self) -> bool {
            true
        }
        fn needs_frq(&self) -> bool {
            false
        }
        fn expressions(&self) -> &'static [&'static str] {
            &[]
        }
        fn sample_rate(&self) -> u32 {
            44100
        }
        fn channels(&self) -> u16 {
            1
        }
    }

    let manifest = teto_manifest();
    let mgr = CapabilityManager::new(&manifest, &FakeEngine);
    // The bank can render v; a missing phoneme reports via missing().
    assert!(mgr.can_render("v"));
    assert_eq!(mgr.missing(&["a", "v"]), vec!["a"]);
    assert!(mgr.needs_oto());
    assert_eq!(mgr.engine_id(), "fake");
}
