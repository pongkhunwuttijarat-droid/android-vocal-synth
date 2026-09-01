//! Engine capability declaration (engine-agnostic).
//!
//! Every rendering engine declares what it needs and supports through
//! [`EngineCapabilities`]. The phonemizer layer (L1/L2) and the host
//! (`synth-cli`/`synth-server`) consult this trait — NEVER the concrete
//! engine type — so the phonemizer crate does not depend on worldline
//! (or any other engine). Engines implement this trait in their own
//! crate (worldline-plugin implements it for `WorldlineCapabilities`).
//!
//! Direction of dependencies:
//!   domain (this trait + PhonemeManifest in voicebank)
//!     ├── phonemizer  → uses the trait only
//!     └── worldline-plugin → implements the trait

/// What a rendering engine needs from the voicebank / supports at
/// render time. Mirrors the fields of OpenUtau's
/// `WorldlineRenderer.supportedExp` + plugin capabilities, minus the
/// worldline-specific bits (modes).
pub trait EngineCapabilities {
    /// Stable engine id, e.g. `"worldline"`.
    fn id(&self) -> &'static str;

    /// True when the engine needs the actual wav samples of the mapped
    /// phonemes (worldline does; a pure-model engine might not).
    fn needs_wav_samples(&self) -> bool;

    /// True when the engine needs an oto.ini alias per phoneme.
    fn needs_oto(&self) -> bool;

    /// True when the engine consumes `.frq` pitch files (worldline does
    /// not — it uses pyin + f0 curves instead).
    fn needs_frq(&self) -> bool;

    /// Supported expression abbreviations (curves + numerical), in
    /// preference order.
    fn expressions(&self) -> &'static [&'static str];

    /// Preferred output sample rate (Hz).
    fn sample_rate(&self) -> u32;

    /// Output channel count (mono = 1).
    fn channels(&self) -> u16;
}
