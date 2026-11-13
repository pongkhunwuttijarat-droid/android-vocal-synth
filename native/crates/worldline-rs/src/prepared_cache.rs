//! Exact in-memory memoization of per-phoneme WORLD analysis models.
//!
//! The cache stores the already-prepared F0/SP/AP arrays. Timeline placement
//! remains request-specific and is rebuilt for each phrase, so a hit can only
//! skip source analysis and feature extraction; it cannot change phrase timing.

use std::collections::{HashMap, VecDeque};

use sha2::{Digest, Sha256};
use worldline_plugin::PhonemeRequest;

use crate::model::Model;

#[derive(Clone)]
struct Features {
    sample_rate: i32,
    frame_ms: f64,
    fft_size: usize,
    f0: Vec<f64>,
    sp: Vec<f64>,
    ap: Vec<f64>,
}

impl Features {
    fn from_model(model: &Model) -> Self {
        Self {
            sample_rate: model.fs(),
            frame_ms: model.frame_ms(),
            fft_size: model.fft_size(),
            f0: model.f0().to_vec(),
            sp: model.sp().to_vec(),
            ap: model.ap().to_vec(),
        }
    }

    fn into_model(self) -> Model {
        let mut model = Model::new_synth(self.sample_rate, self.frame_ms, self.fft_size);
        model.set_analysis(self.f0, self.sp, self.ap, self.fft_size);
        model
    }

    fn byte_size(&self) -> usize {
        (self.f0.len() + self.sp.len() + self.ap.len()) * std::mem::size_of::<f64>()
    }
}

struct Entry {
    features: Features,
    byte_size: usize,
}

/// Bounded LRU cache of deterministic per-phoneme WORLD feature models.
///
/// The key hashes the source PCM, FRQ bytes, model frame size, and every
/// synthesis-request field that can affect analysis/remapping. Pure timeline
/// placement fields (`pos_ms`, `skip_ms`, and fades) are omitted; their
/// `ModelTiming` is regenerated on every cache hit.
#[derive(Default)]
pub struct PreparedModelCache {
    capacity_bytes: usize,
    memory_bytes: usize,
    entries: HashMap<[u8; 32], Entry>,
    order: VecDeque<[u8; 32]>,
}

impl PreparedModelCache {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity_bytes,
            ..Self::default()
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    pub(crate) fn key(
        request: &PhonemeRequest,
        samples: &[f64],
        sample_fs: i32,
        frq: Option<&[u8]>,
        frame_ms: f64,
    ) -> [u8; 32] {
        let mut h = Sha256::new();
        let bytes = |h: &mut Sha256, b: &[u8]| {
            h.update((b.len() as u64).to_le_bytes());
            h.update(b);
        };
        let f64v = |h: &mut Sha256, v: f64| h.update(v.to_bits().to_le_bytes());
        let i32v = |h: &mut Sha256, v: i32| h.update(v.to_le_bytes());

        bytes(&mut h, request.wav_path.as_bytes());
        bytes(&mut h, request.phoneme.as_bytes());
        i32v(&mut h, sample_fs);
        f64v(&mut h, frame_ms);
        h.update((samples.len() as u64).to_le_bytes());
        for sample in samples {
            f64v(&mut h, *sample);
        }
        match frq {
            Some(data) => {
                h.update([1]);
                bytes(&mut h, data);
            }
            None => h.update([0]),
        }

        // These fields can change the prepared model. Timeline placement and
        // overlap/fade fields are deliberately left out and rebuilt later.
        for value in [
            request.length_ms,
            request.con_vel,
            request.offset,
            request.required_length,
            request.consonant,
            request.cut_off,
            request.volume,
            request.modulation,
            request.tempo,
        ] {
            f64v(&mut h, value);
        }
        i32v(&mut h, request.tone);
        i32v(&mut h, request.flag_g);
        i32v(&mut h, request.flag_O);
        i32v(&mut h, request.flag_P);
        i32v(&mut h, request.flag_Mt);
        i32v(&mut h, request.flag_Mb);
        i32v(&mut h, request.flag_Mv);
        h.update((request.pitch_bend.len() as u64).to_le_bytes());
        for bend in &request.pitch_bend {
            i32v(&mut h, *bend);
        }
        h.finalize().into()
    }

    pub(crate) fn get(&mut self, key: &[u8; 32]) -> Option<Model> {
        let features = self.entries.get(key)?.features.clone();
        if let Some(i) = self.order.iter().position(|k| k == key) {
            self.order.remove(i);
        }
        self.order.push_back(*key);
        Some(features.into_model())
    }

    pub(crate) fn put(&mut self, key: [u8; 32], model: &Model) {
        if self.capacity_bytes == 0 {
            return;
        }
        let features = Features::from_model(model);
        let byte_size = features.byte_size();
        if byte_size > self.capacity_bytes {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(old.byte_size);
            if let Some(i) = self.order.iter().position(|k| *k == key) {
                self.order.remove(i);
            }
        }
        while self.memory_bytes + byte_size > self.capacity_bytes {
            let Some(old_key) = self.order.pop_front() else {
                break;
            };
            if let Some(old) = self.entries.remove(&old_key) {
                self.memory_bytes = self.memory_bytes.saturating_sub(old.byte_size);
            }
        }
        self.memory_bytes += byte_size;
        self.entries.insert(
            key,
            Entry {
                features,
                byte_size,
            },
        );
        self.order.push_back(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PhonemeRequest {
        PhonemeRequest {
            phoneme: "a".into(),
            wav_path: "voice.wav".into(),
            pos_ms: 10.0,
            skip_ms: 2.0,
            length_ms: 80.0,
            fade_in_ms: 5.0,
            fade_out_ms: 5.0,
            tone: 60,
            con_vel: 100.0,
            offset: 12.0,
            required_length: 100.0,
            consonant: 10.0,
            cut_off: 0.0,
            volume: 100.0,
            modulation: 0.0,
            tempo: 120.0,
            pitch_bend: vec![0, 10, -5],
            flag_g: 0,
            flag_O: 0,
            flag_P: 86,
            flag_Mt: 0,
            flag_Mb: 0,
            flag_Mv: 100,
        }
    }

    #[test]
    fn cached_feature_arrays_round_trip_exactly() {
        let mut cache = PreparedModelCache::new(4096);
        let mut model = Model::new_synth(44_100, 10.0, 8);
        model.set_analysis(
            vec![100.0, 123.5],
            (0..10).map(|i| i as f64 / 10.0).collect(),
            (0..10).map(|i| 1.0 - i as f64 / 20.0).collect(),
            8,
        );
        let key = [7; 32];
        cache.put(key, &model);
        let restored = cache.get(&key).expect("cache hit");
        assert_eq!(restored.fs(), model.fs());
        assert_eq!(restored.frame_ms(), model.frame_ms());
        assert_eq!(restored.fft_size(), model.fft_size());
        assert_eq!(restored.f0(), model.f0());
        assert_eq!(restored.sp(), model.sp());
        assert_eq!(restored.ap(), model.ap());
    }

    #[test]
    fn request_key_ignores_only_timeline_placement() {
        let req = request();
        let samples = [0.0, 0.25, -0.25];
        let key = PreparedModelCache::key(&req, &samples, 44_100, Some(&[1, 2]), 10.0);
        let mut shifted = req.clone();
        shifted.pos_ms += 500.0;
        shifted.skip_ms += 2.0;
        shifted.fade_in_ms += 1.0;
        shifted.fade_out_ms += 1.0;
        assert_eq!(
            key,
            PreparedModelCache::key(&shifted, &samples, 44_100, Some(&[1, 2]), 10.0)
        );
        shifted.offset += 1.0;
        assert_ne!(
            key,
            PreparedModelCache::key(&shifted, &samples, 44_100, Some(&[1, 2]), 10.0)
        );
        assert_ne!(
            key,
            PreparedModelCache::key(&req, &[0.0, 0.5, -0.25], 44_100, Some(&[1, 2]), 10.0)
        );
    }
}
