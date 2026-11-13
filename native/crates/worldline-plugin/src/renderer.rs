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

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use feed::render_input::RenderInput;
use voicebank::WavData;
use worldline_sys::{PhraseSynth, PreparedCache, SynthRequest, WorldlineStreamStats};

use crate::capabilities::WorldlineCapabilities;
use crate::convert::{self, PhonemeRequest};
use crate::error::Error;

/// Preferred output sample rate of the worldline library.
pub const DEFAULT_SAMPLE_RATE: u32 = 44100;
/// Frame size of the per-frame expression curves, in ms.
///
/// OpenUtau's C# v1 renderer samples at 10 ms. R2 supplies its own
/// 512/44100 ms frame explicitly.
pub const DEFAULT_FRAME_MS: f64 = 10.0;

const ASSET_CACHE_CAPACITY: usize = 512;
const PREPARED_CACHE_CAPACITY: i32 = 512;

struct BoundedCache<V> {
    cap: usize,
    map: HashMap<String, V>,
    order: VecDeque<String>,
}

impl<V> BoundedCache<V> {
    fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&self, key: &str) -> Option<&V> {
        self.map.get(key)
    }

    fn insert(&mut self, key: String, val: V) {
        if !self.map.contains_key(&key) {
            if self.map.len() >= self.cap {
                while let Some(oldest) = self.order.pop_front() {
                    if self.map.remove(&oldest).is_some() {
                        break;
                    }
                }
            }
            self.order.push_back(key.clone());
        }
        self.map.insert(key, val);
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

/// Sample-based renderer plugin: `feed::RenderInput` → PCM.
pub struct WorldlineRenderer {
    /// Keeps the dlopen'd `Library` (and a live C++ handle) alive for the
    /// renderer's lifetime; see the module docs for why per-render
    /// instances are spawned from [`Self::so_path`] instead.
    synth: PhraseSynth,
    /// Shared thread-safe C++ LRU cache of prepared phoneme `Model` data.
    prepared_cache: Option<PreparedCache>,
    /// Bounded cache of decoded mono WAV samples keyed by `wav_path`.
    wav_cache: Mutex<BoundedCache<Arc<(u32, Vec<f64>)>>>,
    /// Bounded cache of raw OpenUtau `.frq` bytes keyed by `wav_path`.
    frq_cache: Mutex<BoundedCache<Option<Arc<Vec<u8>>>>>,
    /// Where to spawn fresh `PhraseSynth` instances from.
    so_path: PathBuf,
    /// Output sample rate (mono).
    sample_rate: u32,
    /// Frame size of the per-frame expression curves (ms).
    frame_ms: f64,
}

struct PreparedPhraseBuild {
    synth: PhraseSynth,
    request_count: usize,
    unique_wavs: usize,
    unique_frqs: usize,
    request_build: Duration,
    curve_build: Duration,
    synth_open: Duration,
    wav_load: Duration,
    validation_time: Duration,
    frq_load: Duration,
    request_queue: Duration,
    set_curves: Duration,
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
        let prepared_cache = PreparedCache::open(so_path.as_ref(), PREPARED_CACHE_CAPACITY).ok();
        // DIAGNOSTIC (Sprint 2.3.4): env WORLD_FRAME_MS overrides the
        // SetCurves sampling frame (C# v1 uses 10.0).
        let frame_ms = std::env::var_os("WORLD_FRAME_MS")
            .and_then(|v| v.to_str().and_then(|s| s.parse::<f64>().ok()))
            .unwrap_or(DEFAULT_FRAME_MS);
        Ok(WorldlineRenderer {
            synth,
            prepared_cache,
            wav_cache: Mutex::new(BoundedCache::new(ASSET_CACHE_CAPACITY)),
            frq_cache: Mutex::new(BoundedCache::new(ASSET_CACHE_CAPACITY)),
            so_path: so_path.as_ref().to_path_buf(),
            sample_rate: DEFAULT_SAMPLE_RATE,
            frame_ms,
        })
    }

    /// Clear cached WAV samples, `.frq` bytes, and C++ prepared phoneme models.
    pub fn clear_caches(&self) {
        if let Some(ref cache) = self.prepared_cache {
            cache.clear();
        }
        if let Ok(mut w) = self.wav_cache.lock() {
            w.clear();
        }
        if let Ok(mut f) = self.frq_cache.lock() {
            f.clear();
        }
    }

    /// Number of phoneme models currently held in the C++ prepared-model cache.
    pub fn prepared_cache_len(&self) -> usize {
        self.prepared_cache.as_ref().map(|c| c.len()).unwrap_or(0)
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

    fn load_wav_cached(&self, wav_path: &str) -> Result<Arc<(u32, Vec<f64>)>, Error> {
        if let Ok(cache) = self.wav_cache.lock() {
            if let Some(hit) = cache.get(wav_path) {
                return Ok(Arc::clone(hit));
            }
        }
        let wav: WavData = voicebank::read_wav(Path::new(wav_path)).map_err(|source| Error::Wav {
            path: wav_path.to_string(),
            source,
        })?;
        let mono: Vec<f64> = wav.to_mono().into_iter().map(|s| s as f64).collect();
        let entry = Arc::new((wav.sample_rate, mono));
        if let Ok(mut cache) = self.wav_cache.lock() {
            cache.insert(wav_path.to_string(), Arc::clone(&entry));
        }
        Ok(entry)
    }

    fn load_frq_cached(&self, wav_path: &str) -> Option<Arc<Vec<u8>>> {
        if let Ok(cache) = self.frq_cache.lock() {
            if let Some(hit) = cache.get(wav_path) {
                return hit.clone();
            }
        }
        let path = Path::new(wav_path);
        let entry = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|stem| {
                let frq_path = path.with_file_name(format!("{stem}_wav.frq"));
                let data = std::fs::read(&frq_path).ok()?;
                if data.is_empty() {
                    None
                } else {
                    Some(Arc::new(data))
                }
            });
        if let Ok(mut cache) = self.frq_cache.lock() {
            cache.insert(wav_path.to_string(), entry.clone());
        }
        entry
    }

    fn build_phrase_synth(
        &self,
        input: &RenderInput,
    ) -> Result<Option<PreparedPhraseBuild>, Error> {
        let stage_start = Instant::now();
        let requests = convert::build_requests(input)?;
        let request_build = stage_start.elapsed();
        if requests.is_empty() {
            return Ok(None);
        }
        let stage_start = Instant::now();
        let curves = convert::sample_phrase_curves(input, self.frame_ms);
        let curve_build = stage_start.elapsed();
        // Fresh PhraseSynth per phrase: the C++ object accumulates requests.
        let stage_start = Instant::now();
        let synth = PhraseSynth::open(&self.so_path)?;
        synth.set_prepared_cache(self.prepared_cache.as_ref());
        let synth_open = stage_start.elapsed();

        let mut phrase_wavs: HashMap<String, Arc<(u32, Vec<f64>)>> = HashMap::new();
        let mut phrase_frqs: HashMap<String, Option<Arc<Vec<u8>>>> = HashMap::new();
        let mut wav_load = Duration::ZERO;
        let mut validation_time = Duration::ZERO;
        let mut frq_load = Duration::ZERO;
        let mut request_queue = Duration::ZERO;
        for request in &requests {
            let stage_start = Instant::now();
            let wav_arc = if let Some(hit) = phrase_wavs.get(&request.wav_path) {
                Arc::clone(hit)
            } else {
                let loaded = self.load_wav_cached(&request.wav_path)?;
                phrase_wavs.insert(request.wav_path.clone(), Arc::clone(&loaded));
                loaded
            };
            wav_load += stage_start.elapsed();

            let stage_start = Instant::now();
            let (sample_rate, samples) = &*wav_arc;
            convert::validate_request(request, *sample_rate, samples.len())?;
            validation_time += stage_start.elapsed();

            let stage_start = Instant::now();
            let frq_arc = if let Some(hit) = phrase_frqs.get(&request.wav_path) {
                hit.clone()
            } else {
                let loaded = self.load_frq_cached(&request.wav_path);
                phrase_frqs.insert(request.wav_path.clone(), loaded.clone());
                loaded
            };
            frq_load += stage_start.elapsed();

            let synth_request = to_synth_request(
                request,
                *sample_rate,
                samples,
                frq_arc.as_deref().map(|v| v.as_slice()),
            );
            let stage_start = Instant::now();
            synth.add_request(
                &synth_request,
                request.pos_ms,
                request.skip_ms,
                request.length_ms,
                request.fade_in_ms,
                request.fade_out_ms,
            );
            request_queue += stage_start.elapsed();
        }
        let stage_start = Instant::now();
        synth.set_curves(
            &curves.f0,
            Some(&curves.gender),
            Some(&curves.tension),
            Some(&curves.breathiness),
            Some(&curves.voicing),
        );
        let set_curves = stage_start.elapsed();
        let unique_frqs = phrase_frqs.values().filter(|v| v.is_some()).count();
        Ok(Some(PreparedPhraseBuild {
            synth,
            request_count: requests.len(),
            unique_wavs: phrase_wavs.len(),
            unique_frqs,
            request_build,
            curve_build,
            synth_open,
            wav_load,
            validation_time,
            frq_load,
            request_queue,
            set_curves,
        }))
    }

    /// Render one phrase to mono f32 PCM at [`DEFAULT_SAMPLE_RATE`].
    ///
    /// Converts every phoneme into a `SynthRequest` (C# `ResamplerItem` +
    /// `SynthRequestWrapper` semantics), places them on the phrase
    /// timeline with the C# `WorldlineRenderer` timing, sets the per-frame
    /// expression curves and synthesizes.
    pub fn render_phrase(&self, input: &RenderInput) -> Result<Vec<f32>, Error> {
        let call_start = Instant::now();
        let profile = std::env::var_os("WORLDLINE_PROFILE_PREP").is_some();
        let Some(built) = self.build_phrase_synth(input)? else {
            return Ok(Vec::new());
        };
        let stage_start = Instant::now();
        let output = built.synth.synth();
        let synth_call = stage_start.elapsed();
        if profile {
            eprintln!(
                "RENDERER_PROFILE mode=offline requests={} unique_wavs={} unique_frqs={} build_requests_ms={:.3} curve_grid_ms={:.3} open_ms={:.3} wav_lookup_decode_ms={:.3} validation_ms={:.3} frq_lookup_ms={:.3} queue_copy_ms={:.3} set_curves_ms={:.3} synth_call_ms={:.3} total_ms={:.3}",
                built.request_count, built.unique_wavs, built.unique_frqs,
                built.request_build.as_secs_f64() * 1000.0,
                built.curve_build.as_secs_f64() * 1000.0,
                built.synth_open.as_secs_f64() * 1000.0,
                built.wav_load.as_secs_f64() * 1000.0,
                built.validation_time.as_secs_f64() * 1000.0,
                built.frq_load.as_secs_f64() * 1000.0,
                built.request_queue.as_secs_f64() * 1000.0,
                built.set_curves.as_secs_f64() * 1000.0,
                synth_call.as_secs_f64() * 1000.0,
                call_start.elapsed().as_secs_f64() * 1000.0,
            );
        }
        Ok(output)
    }

    /// Render a phrase and synchronously deliver finalized PCM blocks from
    /// the incremental WORLD pipeline. The legacy `render_phrase` remains the
    /// offline baseline and its output is bit-for-bit identical.
    pub fn render_phrase_stream<F>(
        &self,
        input: &RenderInput,
        block_samples: usize,
        callback: F,
    ) -> Result<usize, Error>
    where
        F: FnMut(&[f32]) -> Result<(), String>,
    {
        let (total, _stats) =
            self.render_phrase_stream_with_stats(input, block_samples, callback)?;
        Ok(total)
    }

    /// Render a phrase via the incremental WORLD pipeline and return detailed
    /// streaming latency, concurrency, and peak memory metrics.
    pub fn render_phrase_stream_with_stats<F>(
        &self,
        input: &RenderInput,
        block_samples: usize,
        mut callback: F,
    ) -> Result<(usize, WorldlineStreamStats), Error>
    where
        F: FnMut(&[f32]) -> Result<(), String>,
    {
        let call_start = Instant::now();
        let profile = std::env::var_os("WORLDLINE_PROFILE_PREP").is_some();
        let Some(built) = self.build_phrase_synth(input)? else {
            return Ok((0, WorldlineStreamStats::default()));
        };
        let first_callback = std::cell::Cell::new(None::<Duration>);
        let stage_start = Instant::now();
        let (total, stats) = built
            .synth
            .synth_stream_with_stats(block_samples, |samples| {
                if first_callback.get().is_none() {
                    first_callback.set(Some(stage_start.elapsed()));
                }
                callback(samples)
            })
            .map_err(Error::StreamCallback)?;
        let synth_call = stage_start.elapsed();
        if profile {
            eprintln!(
                "RENDERER_PROFILE mode=stream requests={} unique_wavs={} unique_frqs={} build_requests_ms={:.3} curve_grid_ms={:.3} open_ms={:.3} wav_lookup_decode_ms={:.3} validation_ms={:.3} frq_lookup_ms={:.3} queue_copy_ms={:.3} set_curves_ms={:.3} stream_to_first_callback_ms={:.3} stream_call_ms={:.3} total_ms={:.3}",
                built.request_count, built.unique_wavs, built.unique_frqs,
                built.request_build.as_secs_f64() * 1000.0,
                built.curve_build.as_secs_f64() * 1000.0,
                built.synth_open.as_secs_f64() * 1000.0,
                built.wav_load.as_secs_f64() * 1000.0,
                built.validation_time.as_secs_f64() * 1000.0,
                built.frq_load.as_secs_f64() * 1000.0,
                built.request_queue.as_secs_f64() * 1000.0,
                built.set_curves.as_secs_f64() * 1000.0,
                first_callback.get().unwrap_or(synth_call).as_secs_f64() * 1000.0,
                synth_call.as_secs_f64() * 1000.0,
                call_start.elapsed().as_secs_f64() * 1000.0,
            );
        }
        Ok((total, stats))
    }
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
        Some(f) => (
            f.len() as i32,
            f.as_ptr() as *const std::os::raw::c_char as *mut std::os::raw::c_char,
        ),
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
