//! world-rs: Rust reimplementation of the mmorise/World synthesis DSP core.
//!
//! This crate reimplements the C++ `libworldline.so` WORLD resynthesis kernel in
//! pure Rust so it can run natively in a browser (wasm32-unknown-unknown) with no
//! dlopen'd native `.so`. The upstream phonemizer already produces per-frame f0 +
//! expression curves; this crate consumes those and performs the actual spectral
//! envelope extraction, minimum-phase synthesis, and overlap-add.

pub mod analysis;
pub mod cheaptrick;
pub mod d4c;
pub mod f0;
pub mod fft;
pub mod helpers;
pub mod synthesis;

/// Contiguous WORLD analysis features. `spectral` and `aperiodicity` are
/// row-major arrays indexed as `frame * bin_count + bin`.
#[derive(Debug, Clone)]
pub struct WorldFeatures {
    pub frame_count: usize,
    pub bin_count: usize,
    pub f0: Vec<f64>,
    pub spectral: Vec<f64>,
    pub aperiodicity: Vec<f64>,
}

impl WorldFeatures {
    pub fn try_new(frame_count: usize, bin_count: usize) -> Result<Self, FeatureError> {
        let frame_bins = frame_count
            .checked_mul(bin_count)
            .ok_or(FeatureError::SizeOverflow)?;
        Ok(Self {
            frame_count,
            bin_count,
            f0: vec![0.0; frame_count],
            spectral: vec![0.0; frame_bins],
            aperiodicity: vec![0.0; frame_bins],
        })
    }

    pub fn validate(&self) -> Result<(), FeatureError> {
        let frame_bins = self
            .frame_count
            .checked_mul(self.bin_count)
            .ok_or(FeatureError::SizeOverflow)?;
        if self.f0.len() != self.frame_count {
            return Err(FeatureError::Length {
                field: "f0",
                expected: self.frame_count,
                actual: self.f0.len(),
            });
        }
        for (field, actual) in [
            ("spectral", self.spectral.len()),
            ("aperiodicity", self.aperiodicity.len()),
        ] {
            if actual != frame_bins {
                return Err(FeatureError::Length {
                    field,
                    expected: frame_bins,
                    actual,
                });
            }
        }
        Ok(())
    }

    pub fn frame(&self, index: usize) -> Option<SpectralFrame<'_>> {
        if index >= self.frame_count {
            return None;
        }
        let start = index * self.bin_count;
        Some(SpectralFrame {
            f0: self.f0[index],
            envelope: &self.spectral[start..start + self.bin_count],
            aperiodicity: &self.aperiodicity[start..start + self.bin_count],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureError {
    SizeOverflow,
    Length {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
}

/// Borrowed view over one row of [`WorldFeatures`].
pub struct SpectralFrame<'a> {
    pub f0: f64,
    pub envelope: &'a [f64],
    pub aperiodicity: &'a [f64],
}

/// Cooperative cancellation contract shared by native and browser adapters.
pub trait Cancellation {
    fn is_cancelled(&self) -> bool;
}

impl Cancellation for () {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_use_contiguous_rows_and_borrowed_views() {
        let mut features = WorldFeatures::try_new(2, 3).unwrap();
        features.f0 = vec![100.0, 200.0];
        features
            .spectral
            .copy_from_slice(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        features
            .aperiodicity
            .copy_from_slice(&[6.0, 5.0, 4.0, 3.0, 2.0, 1.0]);
        features.validate().unwrap();
        let second = features.frame(1).unwrap();
        assert_eq!(second.f0, 200.0);
        assert_eq!(second.envelope, &[4.0, 5.0, 6.0]);
        assert_eq!(second.aperiodicity, &[3.0, 2.0, 1.0]);
        assert!(features.frame(2).is_none());
    }

    #[test]
    fn features_reject_inconsistent_storage() {
        let mut features = WorldFeatures::try_new(2, 3).unwrap();
        features.spectral.pop();
        assert!(matches!(
            features.validate(),
            Err(FeatureError::Length {
                field: "spectral",
                ..
            })
        ));
    }

    #[test]
    fn features_reject_size_overflow() {
        assert!(matches!(
            WorldFeatures::try_new(usize::MAX, 2),
            Err(FeatureError::SizeOverflow)
        ));
    }
}
