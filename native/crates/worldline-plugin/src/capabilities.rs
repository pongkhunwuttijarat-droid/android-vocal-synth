//! Static renderer capabilities — the Rust mirror of the
//! `PluginCapabilities` concept from `native/plugins/abi/plugin_abi.h`
//! (identity + singer types + expressions + input requirements + output
//! format). The host consults this to decide which `RenderInput` sections
//! to fill (see `docs/architecture/feed-data-flow.md`).
//!
//! The worldline renderer is sample-based: it needs per-phoneme oto
//! entries and the wav samples, and never touches `.frq` files
//! (`needs_frq: false` — the .so estimates F0 with pyin and overrides it
//! with the f0 curve).

use domain::{BREC, CLR, DIR, DYN, GENC, MOD, PITD, SHFT, TENC, VEL, VOIC, VOL};

/// Rendering modes of the worldline library (docs/architecture/rendering-systems.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldlineMode {
    /// Per-phoneme `Resample` + concatenation (UTAU-compat feeder mode).
    Classic,
    /// `PhraseSynth` WORLD analysis + synthesis (v1) — what this crate implements.
    Worldline,
    /// v2: `PhraseSynth` features + neural vocoder (future; needs the
    /// ONNX runtime and vocoder packages).
    Worldline2,
}

impl WorldlineMode {
    /// Every mode the worldline library supports, in preference order.
    pub const ALL: [WorldlineMode; 3] = [
        WorldlineMode::Classic,
        WorldlineMode::Worldline,
        WorldlineMode::Worldline2,
    ];

    /// Stable string id, e.g. `"worldline"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            WorldlineMode::Classic => "classic",
            WorldlineMode::Worldline => "worldline",
            WorldlineMode::Worldline2 => "worldline2",
        }
    }
}

/// What the worldline renderer needs and supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldlineCapabilities {
    /// Supported rendering modes (this crate drives the `Worldline` path).
    pub modes: [WorldlineMode; 3],
    /// Requires the actual wav samples of the mapped phonemes.
    pub needs_wav_samples: bool,
    /// Requires oto.ini aliases per phoneme.
    pub needs_oto: bool,
    /// Never uses `.frq` pitch files (pyin + f0 curve override instead).
    pub needs_frq: bool,
    /// Supported expression abbreviations (curves and numerical), in the
    /// order OpenUtau's `WorldlineRenderer.supportedExp` lists them.
    pub expressions: &'static [&'static str],
    /// Preferred output sample rate.
    pub sample_rate: u32,
    /// Output channel count (mono).
    pub channels: u16,
}

impl WorldlineCapabilities {
    /// The single static capabilities instance (mirrors
    /// `plugin_get_capabilities()` returning a static struct).
    pub const fn get() -> &'static WorldlineCapabilities {
        &WorldlineCapabilities {
            modes: WorldlineMode::ALL,
            needs_wav_samples: true,
            needs_oto: true,
            needs_frq: false,
            expressions: &[
                DYN, PITD, GENC, BREC, TENC, VOIC, VEL, VOL, MOD, CLR, SHFT, DIR,
            ],
            sample_rate: crate::renderer::DEFAULT_SAMPLE_RATE,
            channels: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_declare_sample_based_requirements() {
        let c = WorldlineCapabilities::get();
        assert!(c.needs_wav_samples);
        assert!(c.needs_oto);
        assert!(!c.needs_frq);
        assert_eq!(c.sample_rate, 44100);
        assert_eq!(c.channels, 1);
        assert_eq!(
            c.expressions,
            &[DYN, PITD, GENC, BREC, TENC, VOIC, VEL, VOL, MOD, CLR, SHFT, DIR]
        );
    }

    #[test]
    fn mode_ids_stable() {
        assert_eq!(WorldlineMode::Classic.as_str(), "classic");
        assert_eq!(WorldlineMode::Worldline.as_str(), "worldline");
        assert_eq!(WorldlineMode::Worldline2.as_str(), "worldline2");
        assert_eq!(WorldlineMode::ALL.len(), 3);
    }
}
