//! The `WorldlineRenderer`: `RenderInput` → `PhraseSynth` → mono PCM.
//!
//! Mirrors `OpenUtau.Core/Classic/WorldlineRenderer.cs` (v1 path): build
//! one [`PhonemeRequest`] per phoneme, `AddRequest` each with the
//! phrase-relative timing, `SetCurves` with the per-frame expression
//! curves, then `Synth`.
//!
//! The C++ `PhraseSynth` object accumulates requests and has no reset, so
//! — exactly like the C# renderer, which news up a `PhraseSynth` per
//! `Render` call — a fresh `PhraseSynth` is created for every
//! [`render_phrase`](WorldlineRenderer::render_phrase). The stored
//! `synth` keeps the dlopen'd `.so` loaded for the renderer's lifetime.

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

use feed::render_input::RenderInput;
use voicebank::WavData;
use worldline_sys::{PhraseSynth, SynthRequest};

use crate::capabilities::WorldlineCapabilities;
use crate::convert::{self, PhonemeRequest};
use crate::error::Error;

/// Preferred output sample rate of the worldline library.
pub const DEFAULT_SAMPLE_RATE: u32 = 44100;
/// Frame size of the per-frame expression curves, in ms.
///
/// The Sprint 2.1 contract picks 11.6 ms (the v2 renderer frame:
/// `512/44100 × 1000`); the C# v1 renderer uses 10 ms, which matches the
/// C++ analysis frame. The C++ side stretches the curves to its own 10 ms
/// grid, so this only affects sampling density.
pub const DEFAULT_FRAME_MS: f64 = 11.6;

/// Sample-based renderer plugin: `feed::RenderInput` → PCM.
pub struct WorldlineRenderer {
    /// Keeps the dlopen'd `Library` (and a live C++ handle) alive for the
    /// renderer's lifetime; see the module docs for why per-render
    /// instances are spawned from [`Self::so_path`] instead.
    synth: PhraseSynth,
    /// Where to spawn fresh `PhraseSynth` instances from.
    so_path: PathBuf,
    /// Output sample rate (mono).
    sample_rate: u32,
    /// Frame size of the per-frame expression curves (ms).
    frame_ms: f64,
}

impl WorldlineRenderer {
    /// dlopen `so_path` (validating the library and creating a reference
    /// `PhraseSynth`) and build the renderer.
    ///
    /// # Safety model
    ///
    /// Loading an untrusted or ABI-mismatched `.so` can crash the process
    /// (see `worldline-sys::PhraseSynth::open`).
    pub fn open(so_path: impl AsRef<Path>) -> Result<Self, Error> {
        let synth = PhraseSynth::open(so_path.as_ref())?;
        // DIAGNOSTIC (Sprint 2.3.4): env WORLD_FRAME_MS overrides the
        // SetCurves sampling frame (C# v1 uses 10.0).
        let frame_ms = std::env::var_os("WORLD_FRAME_MS")
            .and_then(|v| v.to_str().and_then(|s| s.parse::<f64>().ok()))
            .unwrap_or(DEFAULT_FRAME_MS);
        Ok(WorldlineRenderer {
            synth,
            so_path: so_path.as_ref().to_path_buf(),
            sample_rate: DEFAULT_SAMPLE_RATE,
            frame_ms,
        })
    }

    /// The static capabilities of the worldline renderer.
    pub fn capabilities(&self) -> &'static WorldlineCapabilities {
        WorldlineCapabilities::get()
    }

    /// Raw handle of the reference `PhraseSynth` (keeps the `.so` loaded).
    pub fn raw_handle(&self) -> *mut c_void {
        self.synth.raw_handle()
    }

    /// Output sample rate (mono 44.1 kHz).
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frame size of the per-frame expression curves (ms).
    pub fn frame_ms(&self) -> f64 {
        self.frame_ms
    }

    /// Render one phrase to mono f32 PCM at [`DEFAULT_SAMPLE_RATE`].
    ///
    /// Converts every phoneme into a `SynthRequest` (C# `ResamplerItem` +
    /// `SynthRequestWrapper` semantics), places them on the phrase
    /// timeline with the C# `WorldlineRenderer` timing, sets the per-frame
    /// expression curves and synthesizes.
    ///
    /// # Errors
    ///
    /// Fails on missing/mismatched `sample_based` data, unreadable wavs,
    /// oto regions that exceed the wav (the C# `CutOffExceedDuration` /
    /// `CutOffBeforeOffset` errors), or `.so` load failures.
    pub fn render_phrase(&self, input: &RenderInput) -> Result<Vec<f32>, Error> {
        let requests = convert::build_requests(input)?;
        if requests.is_empty() {
            // The C++ Synth indexes models_[0]; never call it empty.
            return Ok(Vec::new());
        }
        let curves = convert::sample_phrase_curves(input, self.frame_ms);
        // Fresh PhraseSynth per phrase: the C++ object has no reset and
        // accumulates requests (same as the C# per-render allocation).
        let synth = PhraseSynth::open(&self.so_path)?;
        let mut wavs: HashMap<String, (u32, Vec<f64>)> = HashMap::new();
        // Frq bytes must outlive the AddRequest calls (the FFI struct only
        // carries a pointer; PhraseSynthAddRequest copies the struct, and
        // the FrqEstimator copies the f0 data during AddRequest).
        let mut frqs: HashMap<String, Vec<u8>> = HashMap::new();
        for request in &requests {
            let (sample_rate, samples) = load_wav(&mut wavs, request)?;
            convert::validate_request(request, *sample_rate, samples.len())?;
            let frq = load_frq(&mut frqs, request);
            let synth_request =
                to_synth_request(request, *sample_rate, samples, frq.map(|v| v.as_slice()));
            synth.add_request(
                &synth_request,
                request.pos_ms,
                request.skip_ms,
                request.length_ms,
                request.fade_in_ms,
                request.fade_out_ms,
            );
        }
        synth.set_curves(
            &curves.f0,
            Some(&curves.gender),
            Some(&curves.tension),
            Some(&curves.breathiness),
            Some(&curves.voicing),
        );
        Ok(synth.synth())
    }
}

/// Load (and cache per phrase) the wav of `request` as mono f64 samples,
/// like C# `SynthRequestWrapper` (`Wave.GetSamples(...).ToMono(1, 0)`).
fn load_wav<'a>(
    wavs: &'a mut HashMap<String, (u32, Vec<f64>)>,
    request: &PhonemeRequest,
) -> Result<&'a (u32, Vec<f64>), Error> {
    if !wavs.contains_key(&request.wav_path) {
        let wav: WavData =
            voicebank::read_wav(Path::new(&request.wav_path)).map_err(|source| Error::Wav {
                path: request.wav_path.clone(),
                source,
            })?;
        let mono: Vec<f64> = wav.to_mono().into_iter().map(|s| s as f64).collect();
        wavs.insert(request.wav_path.clone(), (wav.sample_rate, mono));
    }
    Ok(wavs.get(&request.wav_path).expect("just inserted"))
}

/// Load the OpenUtau .frq file next to `request`'s wav (`{stem}_wav.frq`),
/// if present. The C++ FrqEstimator uses its per-frame f0 instead of the
/// noisy pyin estimate — plosive bursts (unvoiced, f0≈0 from pyin) keep
/// their real f0 from the frq and stop being rendered as silence.
fn load_frq<'a>(
    frqs: &'a mut HashMap<String, Vec<u8>>,
    request: &PhonemeRequest,
) -> Option<&'a Vec<u8>> {
    let wav_path = Path::new(&request.wav_path);
    let stem = wav_path.file_stem()?.to_str()?;
    let frq_path = wav_path.with_file_name(format!("{stem}_wav.frq"));
    if !frqs.contains_key(&request.wav_path) {
        let data = std::fs::read(&frq_path).unwrap_or_default();
        if !data.is_empty() {
            frqs.insert(request.wav_path.clone(), data);
        }
    }
    frqs.get(&request.wav_path)
}

/// Fill the FFI `SynthRequest` from a converted request. `samples` stays
/// borrowed by the caller; `PhraseSynthAddRequest` copies the struct.
/// `frq` (the OpenUtau .frq bytes, when the voicebank ships one) is also
/// borrowed — it must outlive the `add_request` call.
fn to_synth_request(
    request: &PhonemeRequest,
    sample_rate: u32,
    samples: &[f64],
    frq: Option<&[u8]>,
) -> SynthRequest {
    let (frq_length, frq_ptr) = match frq {
        // `c_char` is i8 on Linux but u8 on Android — cast through it so
        // the FFI struct type-checks on every target.
        Some(f) => (f.len() as i32, f.as_ptr() as *const std::os::raw::c_char as *mut std::os::raw::c_char),
        None => (0, std::ptr::null_mut()),
    };
    SynthRequest {
        sample_fs: sample_rate as i32,
        sample_length: samples.len() as i32,
        sample: samples.as_ptr() as *mut f64,
        frq_length,
        frq: frq_ptr,
        tone: request.tone,
        con_vel: request.con_vel,
        offset: request.offset,
        required_length: request.required_length,
        consonant: request.consonant,
        cut_off: request.cut_off,
        volume: request.volume,
        modulation: request.modulation,
        tempo: request.tempo,
        pitch_bend_length: request.pitch_bend.len() as i32,
        pitch_bend: request.pitch_bend.as_ptr() as *mut i32,
        flag_g: request.flag_g,
        flag_O: request.flag_O,
        flag_P: request.flag_P,
        flag_Mt: request.flag_Mt,
        flag_Mb: request.flag_Mb,
        flag_Mv: request.flag_Mv,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_request_filled_from_phoneme_request() {
        let request = PhonemeRequest {
            phoneme: "a".into(),
            wav_path: "/vb/a.wav".into(),
            pos_ms: 10.0,
            skip_ms: 5.0,
            length_ms: 100.0,
            fade_in_ms: 5.0,
            fade_out_ms: 35.0,
            tone: 62,
            con_vel: 120.0,
            offset: 12.5,
            required_length: 150.0,
            consonant: 30.0,
            cut_off: -200.0,
            volume: 80.0,
            modulation: 0.0,
            tempo: 120.0,
            pitch_bend: vec![0, 10, -5],
            flag_g: 0,
            flag_O: 0,
            flag_P: 86,
            flag_Mt: 0,
            flag_Mb: 0,
            flag_Mv: 100,
        };
        let samples = [0.1f64, -0.2, 0.3];
        let sr = to_synth_request(&request, 44100, &samples, None);
        assert_eq!(sr.sample_fs, 44100);
        assert_eq!(sr.sample_length, 3);
        assert_eq!(sr.frq_length, 0);
        assert!(sr.frq.is_null());
        assert_eq!(sr.tone, 62);
        assert_eq!(sr.con_vel, 120.0);
        assert_eq!(sr.offset, 12.5);
        assert_eq!(sr.required_length, 150.0);
        assert_eq!(sr.consonant, 30.0);
        assert_eq!(sr.cut_off, -200.0);
        assert_eq!(sr.volume, 80.0);
        assert_eq!(sr.modulation, 0.0);
        assert_eq!(sr.tempo, 120.0);
        assert_eq!(sr.pitch_bend_length, 3);
        assert_eq!(sr.flag_P, 86);
        assert_eq!(sr.flag_Mv, 100);
        assert!(sr.frq.is_null());
        assert!(sr.frq_length == 0);
        // The request is copied by PhraseSynthAddRequest; the pointers
        // must be readable (samples are still alive here).
        unsafe {
            assert_eq!(*sr.sample, 0.1);
            assert_eq!(*sr.pitch_bend, 0);
        }
    }
}
