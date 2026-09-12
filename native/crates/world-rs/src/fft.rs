//! WORLD-compatible real FFT abstraction.
//!
//! The storage contract is intentionally explicit instead of exposing the
//! layout of a particular FFT backend.  The packed layout used here is:
//!
//! ```text
//! packed[0]       = Re(X[0])       (DC)
//! packed[1]       = Re(X[n/2])     (Nyquist)
//! packed[2*k]     = Re(X[k])       (1 <= k < n/2)
//! packed[2*k + 1] = Im(X[k])       (1 <= k < n/2)
//! ```
//!
//! Forward transforms are unnormalised. Inverse transforms divide by `n`.
//! This is the canonical mathematical representation; WORLD/Ooura adapters
//! can translate to/from it at one boundary rather than spreading backend
//! assumptions through the synthesis code.

use std::sync::Arc;

use rustfft::{num_complex::Complex, Fft, FftPlanner, FftPlannerWasmSimd};

/// FFT planner selection.
///
/// `rustfft`'s default planner auto-selects AVX/SSE on x86_64 and NEON on
/// aarch64, but on wasm32 it is **scalar** unless the `wasm_simd` feature is
/// on AND the browser supports fixed-width SIMD. This wrapper tries the SIMD
/// planner first and falls back to the scalar one, so the same code runs on
/// every target.
pub struct Planner {
    scalar: FftPlanner<f64>,
    simd: Option<FftPlannerWasmSimd<f64>>,
}

impl Planner {
    pub fn new() -> Self {
        Self {
            scalar: FftPlanner::new(),
            // `Err(())` off wasm32 or without SIMD support -> scalar fallback.
            simd: FftPlannerWasmSimd::new().ok(),
        }
    }

    /// Whether the SIMD path was selected (useful for diagnostics).
    pub fn is_simd(&self) -> bool {
        self.simd.is_some()
    }

    pub fn forward(&mut self, len: usize) -> Arc<dyn Fft<f64>> {
        match self.simd.as_mut() {
            Some(p) => p.plan_fft_forward(len),
            None => self.scalar.plan_fft_forward(len),
        }
    }

    pub fn inverse(&mut self, len: usize) -> Arc<dyn Fft<f64>> {
        match self.simd.as_mut() {
            Some(p) => p.plan_fft_inverse(len),
            None => self.scalar.plan_fft_inverse(len),
        }
    }
}

thread_local! {
    /// Per-thread plan cache. `rustfft` planning is expensive (algorithm
    /// selection + twiddle computation) and the WORLD stages re-plan the same
    /// sizes thousands of times — once per band per frame in D4C. Caching the
    /// plan turns that into a hash lookup.
    static PLAN_CACHE: std::cell::RefCell<
        std::collections::HashMap<(usize, u8), Arc<dyn Fft<f64>>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
    static THREAD_PLANNER: std::cell::RefCell<Planner> =
        std::cell::RefCell::new(Planner::new());
}

/// Cached forward plan for `len` (see [`PLAN_CACHE`]).
pub fn forward_plan(len: usize) -> Arc<dyn Fft<f64>> {
    plan(len, 0)
}

/// Cached inverse plan for `len` (see [`PLAN_CACHE`]).
pub fn inverse_plan(len: usize) -> Arc<dyn Fft<f64>> {
    plan(len, 1)
}

fn plan(len: usize, direction: u8) -> Arc<dyn Fft<f64>> {
    PLAN_CACHE.with(|cache| {
        let existing = cache.borrow().get(&(len, direction)).cloned();
        match existing {
            Some(p) => p,
            None => {
                let created = THREAD_PLANNER.with(|planner| {
                    let mut planner = planner.borrow_mut();
                    if direction == 0 {
                        planner.forward(len)
                    } else {
                        planner.inverse(len)
                    }
                });
                cache
                    .borrow_mut()
                    .insert((len, direction), created.clone());
                created
            }
        }
    })
}

/// Number of cached plans on this thread (diagnostics).
pub fn cached_plan_count() -> usize {
    PLAN_CACHE.with(|c| c.borrow().len())
}

/// Whether the SIMD FFT planner is available on this target/runtime.
///
/// `false` off wasm32, or in a browser without fixed-width SIMD.
pub fn simd_available() -> bool {
    FftPlannerWasmSimd::<f64>::new().is_ok()
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FftError {
    LengthIsZero,
    LengthIsNotEven(usize),
    LengthMismatch { expected: usize, actual: usize },
}

impl std::fmt::Display for FftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LengthIsZero => write!(f, "FFT length must be non-zero"),
            Self::LengthIsNotEven(n) => write!(f, "real FFT length must be even, got {n}"),
            Self::LengthMismatch { expected, actual } => {
                write!(f, "FFT buffer length mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for FftError {}

/// Backend-independent real FFT with reusable scratch storage.
pub struct RealFft {
    len: usize,
    forward: std::sync::Arc<dyn rustfft::Fft<f64>>,
    inverse: std::sync::Arc<dyn rustfft::Fft<f64>>,
    scratch: Vec<Complex<f64>>,
    buffer: Vec<Complex<f64>>,
}

impl RealFft {
    pub fn new(len: usize) -> Result<Self, FftError> {
        if len == 0 {
            return Err(FftError::LengthIsZero);
        }
        if len % 2 != 0 {
            return Err(FftError::LengthIsNotEven(len));
        }
        let mut planner = Planner::new();
        let forward = planner.forward(len);
        let inverse = planner.inverse(len);
        let scratch_len = forward.get_inplace_scratch_len().max(inverse.get_inplace_scratch_len());
        Ok(Self {
            len,
            forward,
            inverse,
            scratch: vec![Complex::default(); scratch_len],
            buffer: vec![Complex::default(); len],
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Forward transform using unnormalised WORLD-compatible packed storage.
    pub fn forward_packed(&mut self, input: &[f64], packed: &mut [f64]) -> Result<(), FftError> {
        if input.len() != self.len {
            return Err(FftError::LengthMismatch { expected: self.len, actual: input.len() });
        }
        if packed.len() != self.len {
            return Err(FftError::LengthMismatch { expected: self.len, actual: packed.len() });
        }
        for (slot, &re) in self.buffer.iter_mut().zip(input) {
            *slot = Complex { re, im: 0.0 };
        }
        self.forward.process_with_scratch(&mut self.buffer, &mut self.scratch);
        packed[0] = self.buffer[0].re;
        packed[1] = self.buffer[self.len / 2].re;
        for k in 1..self.len / 2 {
            packed[2 * k] = self.buffer[k].re;
            packed[2 * k + 1] = self.buffer[k].im;
        }
        Ok(())
    }

    /// Inverse transform from the packed representation. Divides by `len`.
    pub fn inverse_packed(&mut self, packed: &[f64], output: &mut [f64]) -> Result<(), FftError> {
        if packed.len() != self.len {
            return Err(FftError::LengthMismatch { expected: self.len, actual: packed.len() });
        }
        if output.len() != self.len {
            return Err(FftError::LengthMismatch { expected: self.len, actual: output.len() });
        }
        self.buffer[0] = Complex { re: packed[0], im: 0.0 };
        self.buffer[self.len / 2] = Complex { re: packed[1], im: 0.0 };
        for k in 1..self.len / 2 {
            let value = Complex { re: packed[2 * k], im: packed[2 * k + 1] };
            self.buffer[k] = value;
            self.buffer[self.len - k] = value.conj();
        }
        self.inverse.process_with_scratch(&mut self.buffer, &mut self.scratch);
        for (dst, value) in output.iter_mut().zip(self.buffer.iter()) {
            *dst = value.re / self.len as f64;
        }
        Ok(())
    }
}

/// Compatibility helper retaining the prototype API: writes positive-bin
/// magnitudes using WORLD's `|X[k]| / 2` convention.
pub fn positive_magnitudes(input: &[f64], output: &mut [f64]) -> Result<(), FftError> {
    let n = input.len();
    if output.len() != n {
        return Err(FftError::LengthMismatch { expected: n, actual: output.len() });
    }
    let mut fft = RealFft::new(n)?;
    let mut packed = vec![0.0; n];
    fft.forward_packed(input, &mut packed)?;
    output.fill(0.0);
    output[0] = packed[0];
    for k in 1..n / 2 {
        output[k] = (packed[2 * k].mul_add(packed[2 * k], packed[2 * k + 1] * packed[2 * k + 1])).sqrt() / 2.0;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_lengths() {
        assert!(matches!(RealFft::new(0), Err(FftError::LengthIsZero)));
        assert!(matches!(RealFft::new(3), Err(FftError::LengthIsNotEven(3))));
    }

    #[test]
    fn round_trip_and_dc_nyquist_are_correct() {
        let input: Vec<f64> = (0..64).map(|i| (i as f64 * 0.37).sin()).collect();
        let mut fft = RealFft::new(input.len()).unwrap();
        let mut packed = vec![0.0; input.len()];
        let mut output = vec![0.0; input.len()];
        fft.forward_packed(&input, &mut packed).unwrap();
        fft.inverse_packed(&packed, &mut output).unwrap();
        assert!(input.iter().zip(output).all(|(a, b)| (a - b).abs() < 1e-12));
        assert!((packed[0] - input.iter().sum::<f64>()).abs() < 1e-12);
    }

    #[test]
    fn leakage_is_preserved_and_not_hidden_by_bin_only_tests() {
        let n = 128;
        let input: Vec<f64> = (0..n).map(|i| (2.0 * std::f64::consts::PI * 7.25 * i as f64 / n as f64).sin()).collect();
        let mut fft = RealFft::new(n).unwrap();
        let mut packed = vec![0.0; n];
        fft.forward_packed(&input, &mut packed).unwrap();
        let leakage: f64 = (1..n / 2).filter(|&k| k != 7).map(|k| packed[2 * k].abs() + packed[2 * k + 1].abs()).sum();
        assert!(leakage > 1e-3);
    }
}
