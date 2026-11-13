//! Browser entry point for the Rusted UTAU render engine.
//!
//! The whole engine stack (`domain` → `phonemizer` → `phrase` → `feed` →
//! `worldline-rs` → `runtime::mixer`) compiles to
//! `wasm32-unknown-unknown`. This crate is the thin JS-facing surface: the
//! host page loads a voicebank (`oto.ini` + wav bytes) and a `.ustx`
//! project into the engine, then asks for PCM.
//!
//! Design notes:
//! - **No filesystem.** Voicebank samples are pushed in from JS (OPFS /
//!   fetch) into a [`worldline_rs::MemorySource`]; the engine never calls
//!   `std::fs`.
//! - **No dlopen.** The renderer is `worldline-rs` (pure Rust WORLD), which
//!   is sample-parity with the C++ `.so` (see
//!   `docs/architecture/worldline-rust-port.md`).
//! - **Same pipeline as native.** Rendering goes through
//!   `synth_cli::pipeline`, so chunking / phrase grouping / mixing behave
//!   exactly like the CLI and server.

use std::sync::atomic::AtomicBool;

use wasm_bindgen::prelude::*;

/// Last Rust panic message seen in this instance.
///
/// wasm surfaces a panic as a bare `RuntimeError: unreachable` with no
/// message (the default profile is `panic = "abort"`), so the hook stashes
/// the formatted panic for the host to read back after catching the error.
static LAST_PANIC: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Install the panic hook (runs on module init).
#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        if let Ok(mut slot) = LAST_PANIC.lock() {
            *slot = Some(format!("{info}"));
        }
    }));
}

/// The last panic message, or `""` when none has been recorded.
#[wasm_bindgen]
pub fn last_panic() -> String {
    LAST_PANIC
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_default()
}

use domain::{UPart, UProject};
use project_session::Session;

mod project;
pub use project::ProjectDoc;
mod r2_features;
use runtime::mixer::{AudioChunk, MixInput, TrackSpec};
use voicebank::Voicebank;
use worldline_rs::MemorySource;

/// Engine discriminator for chunk hashing — "worldline-r-rs", the same value
/// the native `r1-rs` engine uses, so a chunk hashed on either side matches.
const CACHE_SALT: u64 = 0x726c31_7273_0004;

#[cfg(target_arch = "wasm32")]
static SESSION_SEED_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn session_seed() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        // `SystemTime::now()` panics on wasm32-unknown-unknown. Mix browser
        // time, JS entropy and a per-instance sequence so repeated session
        // opens in the same millisecond still get distinct seed streams.
        let time = js_sys::Date::now().max(0.0) as u64;
        let random_hi = (js_sys::Math::random() * u32::MAX as f64) as u64;
        let random_lo = (js_sys::Math::random() * u32::MAX as f64) as u64;
        let sequence = SESSION_SEED_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return time.rotate_left(23) ^ (random_hi << 32) ^ random_lo ^ sequence;
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x5EED)
    }
}

/// A renderer over in-memory samples.
pub struct MemoryRenderer {
    source: MemorySource,
}

impl MemoryRenderer {
    /// Build a renderer over `source`.
    pub fn new(source: MemorySource) -> Self {
        Self { source }
    }
}

impl synth_cli::pipeline::PhraseRenderer for MemoryRenderer {
    fn render_phrase(&self, input: &feed::render_input::RenderInput) -> Result<Vec<f32>, String> {
        worldline_rs::render_phrase_with_source(
            input,
            worldline_plugin::DEFAULT_FRAME_MS,
            worldline_rs::model_frame_ms(),
            &self.source,
        )
    }

    fn cache_salt(&self) -> u64 {
        CACHE_SALT
    }

    fn parallel_safe(&self) -> bool {
        // wasm builds have no thread pool (the `parallel` feature is off),
        // so this is only a hint; the pipeline renders sequentially.
        false
    }
}

/// A planned, renderable chunk — the unit of parallel work.
///
/// `chunk_count` / `chunk_meta` describe the plan; `render_chunk` renders one
/// entry. A host fans the indices out across Web Workers and mixes the
/// results in index order with [`Mixer`], which reproduces
/// `render_project` byte-for-byte because the chunk render and the mix are
/// the same Rust code the single-threaded path uses.
struct TrackPlan {
    phrases: Vec<synth_cli::pipeline::PhraseInput>,
    chunks: Vec<synth_cli::pipeline::PlannedChunk>,
    skipped: Vec<String>,
}

/// The browser-side engine instance.
#[wasm_bindgen]
pub struct Engine {
    voicebank: Option<Voicebank>,
    source: MemorySource,
    media_source: media::MemoryMediaSource,
    warnings: Vec<String>,
    plan: Option<TrackPlan>,
    /// Rust-owned project document (edits go through reversible commands).
    doc: Option<Session>,
}

#[wasm_bindgen]
impl Engine {
    /// Create an empty engine.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine {
            voicebank: None,
            source: MemorySource::new(),
            media_source: media::MemoryMediaSource::new(),
            warnings: Vec::new(),
            plan: None,
            doc: None,
        }
    }

    /// Install a voicebank from `oto.ini` bytes (UTF-8/Shift-JIS
    /// auto-detected by the parser).
    ///
    /// `oto_dir` is the logical directory that holds this `oto.ini` and its
    /// wavs (in a UTAU bank: `voice`). Sample keys are
    /// `"{oto_dir}/{wav}"`, so the host stores each wav under exactly the
    /// key [`Self::wav_keys`] reports.
    pub fn set_voicebank(&mut self, id: &str, name: &str, oto_dir: &str, oto_ini: &[u8]) {
        let root = if oto_dir.is_empty() { "/vb" } else { oto_dir };
        let vb = Voicebank::from_oto_bytes(id, name, root, root, oto_ini);
        self.warnings = vb.warnings.clone();
        self.voicebank = Some(vb);
    }

    /// Number of alias entries the installed voicebank exposes.
    pub fn alias_count(&self) -> usize {
        self.voicebank
            .as_ref()
            .map(|v| v.oto_map.len())
            .unwrap_or(0)
    }

    /// Non-fatal warnings from the last voicebank load.
    pub fn warnings(&self) -> String {
        self.warnings.join("\n")
    }

    /// Register imported audio bytes in the engine-owned host media seam and
    /// return the decoded source duration in milliseconds.
    pub fn session_register_media(&mut self, media_ref: &str, bytes: &[u8]) -> Result<f64, String> {
        if media_ref.trim().is_empty() || media_ref.contains("..") || media_ref.starts_with('/') {
            return Err("media_ref must be a safe project-relative key".to_string());
        }
        let info = media::probe(bytes)?;
        self.media_source
            .insert(media_ref.to_string(), bytes.to_vec());
        Ok(info.duration_ms)
    }

    /// Every wav path the installed voicebank needs, newline-separated.
    ///
    /// The host loads these (from OPFS / a fetched archive) and passes each
    /// to [`Self::add_sample`] under exactly this key — the phrase builder
    /// derives `wav_path` the same way, so the keys must match.
    pub fn wav_keys(&self) -> String {
        let Some(vb) = self.voicebank.as_ref() else {
            return String::new();
        };
        let mut keys: Vec<String> = vb
            .otos
            .iter()
            .map(|oto| vb.wav_path(oto).to_string_lossy().into_owned())
            .collect();
        keys.sort();
        keys.dedup();
        keys.join("\n")
    }

    /// Load one voicebank sample (wav only — F0 falls back to the estimator).
    ///
    /// `wav_path` must be one of [`Self::wav_keys`].
    pub fn add_sample(&mut self, wav_path: &str, wav_bytes: &[u8]) -> Result<(), String> {
        self.source.insert_wav_bytes(wav_path, wav_bytes)
    }

    /// Load one voicebank sample together with its `.frq` pitch data.
    ///
    /// UTAU banks ship `<stem>_wav.frq` next to every wav, and the reference
    /// renderer ALWAYS prefers it over F0 estimation. Without it the engine
    /// still renders, but the pitch track (and therefore the audio) differs
    /// from the native path — so a host that has the frq bytes should pass
    /// them. Returns `true` when the frq parsed and was attached.
    pub fn add_sample_with_frq(
        &mut self,
        wav_path: &str,
        wav_bytes: &[u8],
        frq_bytes: &[u8],
    ) -> Result<bool, String> {
        let wav =
            voicebank::parse_wav(wav_bytes).map_err(|e| format!("parse wav {wav_path}: {e}"))?;
        let frq_ok = matches!(voicebank::parse_frq(frq_bytes), Ok(f) if !f.f0.is_empty());
        let frq = if frq_ok {
            Some(frq_bytes.to_vec())
        } else {
            None
        };
        self.source.insert(wav_path, wav, frq);
        Ok(frq_ok)
    }

    /// Number of samples currently loaded.
    pub fn sample_count(&self) -> usize {
        self.source.len()
    }

    /// Which FFT backend the engine is using: `"wasm-simd"` or `"scalar"`.
    ///
    /// The scalar path is what a browser without fixed-width SIMD (or a
    /// build without `target-feature=+simd128`) gets.
    pub fn fft_backend(&self) -> String {
        if world_rs::fft::simd_available() {
            "wasm-simd".to_string()
        } else {
            "scalar".to_string()
        }
    }

    /// Render a whole project to mono f32 PCM at 44.1 kHz.
    ///
    /// `ustx_yaml` is the project text; `track` selects the track.
    pub fn render_project(&self, ustx_yaml: &str, track: i32) -> Result<Vec<f32>, String> {
        let project = synth_cli::pipeline::parse_project(ustx_yaml)?;
        let voicebank = self
            .voicebank
            .as_ref()
            .ok_or_else(|| "no voicebank installed (call set_voicebank)".to_string())?;
        let renderer = MemoryRenderer {
            source: self.source.clone(),
        };
        render_track(&project, voicebank, &renderer, track)
    }

    /// Render directly from the authoritative Rust session, including any
    /// committed pronunciation choices, without asking the host to serialize
    /// or reconstruct project state.
    pub fn session_render_project(&self, track: i32) -> Result<Vec<f32>, String> {
        let session = self.doc.as_ref().ok_or_else(|| "no project".to_string())?;
        let project = synth_cli::pipeline::parse_project(&session.yaml())?;
        let voicebank = self
            .voicebank
            .as_ref()
            .ok_or_else(|| "no voicebank installed (call set_voicebank)".to_string())?;
        let renderer = MemoryRenderer {
            source: self.source.clone(),
        };
        let choices = session.render_pronunciations_for_track(track as usize);
        render_track_with_pronunciations_and_media(
            &project,
            voicebank,
            &renderer,
            track,
            Some(&choices),
            &self.media_source,
        )
    }

    /// Read the exact sample WAV keys needed by the authoritative Session's
    /// sample-based render inputs, including committed pronunciation choices.
    /// `track_indices_json` is an array of project track indices. The returned
    /// session/revision fence lets hosts reject keys computed for a stale edit.
    /// This is a WASM host helper; it does not add a logical Session command.
    pub fn session_render_sample_keys_json(
        &self,
        track_indices_json: &str,
    ) -> Result<String, String> {
        let session = self.doc.as_ref().ok_or_else(|| "no project".to_string())?;
        let tracks: Vec<usize> = serde_json::from_str(track_indices_json)
            .map_err(|error| format!("invalid track index list: {error}"))?;
        let project = session.project();
        let voicebank = self
            .voicebank
            .as_ref()
            .ok_or_else(|| "no voicebank installed (call set_voicebank)".to_string())?;
        let mut keys = std::collections::BTreeSet::new();
        for track_index in tracks {
            let track = project
                .tracks
                .get(track_index)
                .ok_or_else(|| format!("track {track_index} not found"))?;
            let kind = synth_cli::pipeline::PhonemizerKind::from_track(track);
            let choices = session.render_pronunciations_for_track(track_index);
            let mut voice_part_index = 0usize;
            for part in &project.parts {
                let UPart::Voice(voice_part) = part else {
                    continue;
                };
                if voice_part.track_no != track_index as i32 {
                    continue;
                }
                let part_choices = choices.get(voice_part_index).map(Vec::as_slice);
                voice_part_index += 1;
                for phrase in synth_cli::pipeline::build_phrase_inputs_with_choices(
                    project,
                    track,
                    voice_part,
                    voicebank,
                    kind,
                    part_choices,
                )? {
                    let Some(sample_based) = phrase.input.sample_based else {
                        continue;
                    };
                    // The render pipeline skips a phrase if any phoneme lacks
                    // an OTO mapping, so do not pin samples for that phrase.
                    if sample_based.oto.len() != phrase.input.phonemes.len() {
                        continue;
                    }
                    keys.extend(sample_based.oto.into_iter().map(|oto| oto.wav_path));
                }
            }
        }
        serde_json::to_string(&serde_json::json!({
            "sessionId": session.session_id(),
            "revision": session.revision(),
            "sampleKeys": keys.into_iter().collect::<Vec<_>>(),
        }))
        .map_err(|error| error.to_string())
    }

    /// Render the authoritative project session as OpenUtau's whole-project
    /// stereo mixdown. Wave-part bytes must be registered with the host media
    /// adapter; every registered media reference is resolved by the shared
    /// Rust pipeline and missing assets are reported by the engine.
    pub fn session_render_mixdown(&self) -> Result<Vec<f32>, String> {
        let session = self.doc.as_ref().ok_or_else(|| "no project".to_string())?;
        let project = synth_cli::pipeline::parse_project(&session.yaml())?;
        let voicebank = self
            .voicebank
            .as_ref()
            .ok_or_else(|| "no voicebank installed (call set_voicebank)".to_string())?;
        let renderer = MemoryRenderer {
            source: self.source.clone(),
        };
        let mut cache = None;
        let cancel = AtomicBool::new(false);
        let pronunciations = (0..project.tracks.len())
            .map(|track| session.render_pronunciations_for_track(track))
            .collect::<Vec<_>>();
        let report = synth_cli::pipeline::render_mixdown_with_pronunciations(
            &project,
            voicebank,
            &renderer,
            None,
            false,
            &mut cache,
            &self.media_source,
            None,
            Some(&cancel),
            None,
            Some(&pronunciations),
        )?;
        Ok(report.samples)
    }

    /// Render and return a 16-bit PCM WAV container (ready for `Blob` /
    /// `AudioContext.decodeAudioData`).
    pub fn render_project_wav(&self, ustx_yaml: &str, track: i32) -> Result<Vec<u8>, String> {
        let samples = self.render_project(ustx_yaml, track)?;
        Ok(wavwriter::write_wav_16(&samples, 44100))
    }

    // -----------------------------------------------------------------
    // Chunk-level API — for fanning a render out across Web Workers
    // -----------------------------------------------------------------
    //
    // wasm has no shared-memory threads here (no `wasm-bindgen-rayon`), but
    // the pipeline's parallelism is coarse-grained: each chunk renders
    // independently from the same deterministically-planned work list. So a
    // host can run N Web Workers, give each one a slice of the chunk indices,
    // and mix the results on the main thread — no shared memory, no
    // COOP/COEP headers, works on GitHub Pages and Safari.

    /// Plan the track's chunks and cache the plan in this engine instance.
    /// Returns the chunk count.
    pub fn plan_project(&mut self, ustx_yaml: &str, track: i32) -> Result<usize, String> {
        let project = synth_cli::pipeline::parse_project(ustx_yaml)?;
        let voicebank = self
            .voicebank
            .as_ref()
            .ok_or_else(|| "no voicebank installed (call set_voicebank)".to_string())?;
        let track_ref = project
            .tracks
            .get(track as usize)
            .ok_or_else(|| format!("track {track} not found"))?;
        let kind = synth_cli::pipeline::PhonemizerKind::from_track(track_ref);
        let (phrases, chunks, skipped) =
            synth_cli::pipeline::plan_track_chunks(&project, voicebank, track, kind)?;
        let count = chunks.len();
        self.plan = Some(TrackPlan {
            phrases,
            chunks,
            skipped,
        });
        Ok(count)
    }

    /// The planned chunks as JSON:
    /// `[{index, hash, position_ms, leading_ms, start_ms, end_ms, own_start, own_end}]`.
    ///
    /// `hash` is the Rust chunk hash (`u64`, emitted as a string because JSON
    /// numbers are f64). It covers the chunk's phonemes *including context* and
    /// the engine salt, so a host can cache rendered chunks by it and re-render
    /// only the entries whose hash changed after an edit. The mixer gets its
    /// positions through [`Mixer::add_chunk`].
    pub fn chunk_meta_json(&self) -> String {
        let Some(plan) = self.plan.as_ref() else {
            return "[]".to_string();
        };
        let items: Vec<String> = plan
            .chunks
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let input = &plan.phrases[c.phrase_index].input;
                let hash = c.chunk.hash(input, CACHE_SALT);
                format!(
                    "{{\"index\":{i},\"hash\":\"{hash}\",\"position_ms\":{:.6},\"leading_ms\":{:.6},\"start_ms\":{:.6},\"end_ms\":{:.6},\"own_start\":{},\"own_end\":{}}}",
                    c.position_ms,
                    c.leading_ms,
                    c.chunk.start_ms,
                    c.chunk.end_ms,
                    c.chunk.own_range.start,
                    c.chunk.own_range.end
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    /// Phrases/parts that could not be planned (newline-separated).
    pub fn plan_skipped(&self) -> String {
        self.plan
            .as_ref()
            .map(|p| p.skipped.join("\n"))
            .unwrap_or_default()
    }

    /// Render ONE planned chunk and return its PCM.
    ///
    /// This is the worker-side call: a worker plans the same project (the plan
    /// is deterministic) and renders the indices it was given. Returns the
    /// chunk's trimmed audio; `Mixer::add_chunk` knows where it goes.
    pub fn render_chunk(&self, index: usize) -> Result<Vec<f32>, String> {
        let plan = self
            .plan
            .as_ref()
            .ok_or_else(|| "no plan (call plan_project first)".to_string())?;
        let planned = plan
            .chunks
            .get(index)
            .ok_or_else(|| format!("chunk {index} out of range"))?;
        let renderer = MemoryRenderer {
            source: self.source.clone(),
        };
        synth_cli::pipeline::render_planned_chunk(&plan.phrases, planned, &renderer)
    }

    /// Extract real WORLDLINE-R2 feature tensors for one planned chunk.
    ///
    /// The returned `R2WF` packet is consumed by the browser's ORT-WebGPU
    /// worker. This uses the pure-Rust WORLD kernel; samples with a shipped
    /// `.frq` use that pitch curve exactly, while the no-FRQ fallback is the
    /// Rust DIO implementation.
    pub fn render_r2_features_chunk(&self, index: usize) -> Result<Vec<u8>, String> {
        let plan = self
            .plan
            .as_ref()
            .ok_or_else(|| "no plan (call plan_project first)".to_string())?;
        let planned = plan
            .chunks
            .get(index)
            .ok_or_else(|| format!("chunk {index} out of range"))?;
        let phrase = &plan.phrases[planned.phrase_index];
        let input = synth_cli::pipeline::chunk_render_input(
            phrase,
            &planned.chunk,
            worldline_plugin::DEFAULT_FRAME_MS,
        );
        let ctx_first = phrase
            .input
            .phonemes
            .get(planned.chunk.ctx_range.start)
            .ok_or_else(|| format!("chunk {index} has empty context"))?;
        let first_own = phrase
            .input
            .phonemes
            .get(planned.chunk.own_range.start)
            .ok_or_else(|| format!("chunk {index} has empty own range"))?;
        let audio_start_ms = ctx_first.position_ms - ctx_first.leading_ms;
        let last_chunk = planned.chunk.own_range.end == phrase.input.phonemes.len();
        r2_features::packet_for_chunk(
            &input,
            &self.source,
            audio_start_ms,
            planned.chunk.start_ms,
            planned.chunk.end_ms,
            first_own.leading_ms,
            last_chunk,
        )
    }

    /// Wav keys one planned chunk actually needs, newline-separated.
    ///
    /// A chunk covers a few phonemes, so a worker handling only that chunk
    /// needs a handful of samples — not the whole bank. Call this after
    /// [`Self::plan_project`], load just those, then [`Self::render_chunk`].
    /// The chunk's *context* phonemes are included (they are rendered too).
    pub fn chunk_wav_keys(&self, index: usize) -> Result<String, String> {
        let plan = self
            .plan
            .as_ref()
            .ok_or_else(|| "no plan (call plan_project first)".to_string())?;
        let planned = plan
            .chunks
            .get(index)
            .ok_or_else(|| format!("chunk {index} out of range"))?;
        let phrase = &plan.phrases[planned.phrase_index];
        let Some(sb) = phrase.input.sample_based.as_ref() else {
            return Ok(String::new());
        };
        let mut keys: Vec<String> = sb
            .oto
            .get(planned.chunk.ctx_range.clone())
            .unwrap_or_default()
            .iter()
            .map(|oto| oto.wav_path.clone())
            .collect();
        keys.sort();
        keys.dedup();
        Ok(keys.join("\n"))
    }

    // -----------------------------------------------------------------
    // Project document (Rust owns the truth; JS owns the pixels)
    // -----------------------------------------------------------------

    /// Create a new empty project and make it current. Returns its YAML.
    pub fn project_new(&mut self) -> String {
        let doc = Session::new(session_seed());
        let yaml = doc.yaml();
        self.doc = Some(doc);
        self.plan = None;
        yaml
    }

    /// Load a `.ustx` YAML project and make it current. Returns its YAML back
    /// (normalized), so a host can round-trip and compare.
    pub fn project_load_yaml(&mut self, yaml: &str) -> Result<String, String> {
        let doc = Session::open(yaml, None, session_seed())?;
        let out = doc.yaml();
        self.doc = Some(doc);
        self.plan = None;
        Ok(out)
    }

    /// Current project as `.ustx` YAML (empty string when none is loaded).
    pub fn project_yaml(&self) -> String {
        self.doc.as_ref().map(|d| d.yaml()).unwrap_or_default()
    }

    /// YAML of the last known-good state (survives a rejected command).
    ///
    /// Takes `&mut self` because the cached snapshot is refreshed lazily after
    /// a mutation (serialization is far more expensive than an edit).
    pub fn project_snapshot(&mut self) -> String {
        self.doc
            .as_mut()
            .map(|session| session.yaml_snapshot().to_string())
            .unwrap_or_default()
    }

    /// Open a project session from canonical YAML and an optional identity
    /// sidecar (`rusted-utau.ids/1`). An empty sidecar string means no sidecar.
    /// Returns the same snapshot DTO as the native v2 session endpoint.
    pub fn session_open_yaml(&mut self, yaml: &str, sidecar_json: &str) -> Result<String, String> {
        self.session_open_yaml_with_sidecars(yaml, sidecar_json, "")
    }

    /// Open a Session using the existing ID sidecar and the separately
    /// versioned pronunciation sidecar. Empty sidecars mean absent files.
    pub fn session_open_yaml_with_sidecars(
        &mut self,
        yaml: &str,
        sidecar_json: &str,
        pronunciation_sidecar_json: &str,
    ) -> Result<String, String> {
        let sidecar = (!sidecar_json.is_empty()).then_some(sidecar_json);
        let pronunciation_sidecar =
            (!pronunciation_sidecar_json.is_empty()).then_some(pronunciation_sidecar_json);
        let session = Session::open_with_pronunciation_sidecar(
            yaml,
            sidecar,
            pronunciation_sidecar,
            session_seed(),
        )?;
        let snapshot = session.snapshot_json()?;
        self.doc = Some(session);
        self.plan = None;
        Ok(snapshot)
    }

    /// Create a new project from the optional `{ "name", "bpm" }` source
    /// fields used by native `source.type = "new"`.
    pub fn session_new_json(&mut self, metadata_json: &str) -> Result<String, String> {
        let metadata: serde_json::Value = serde_json::from_str(metadata_json).map_err(|error| {
            project_session::v2::SessionError::validation(format!(
                "invalid session metadata: {error}"
            ))
            .to_json()
        })?;
        let name = match metadata.get("name") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => Some(value.as_str().ok_or_else(|| {
                project_session::v2::SessionError::validation_field("name", "name must be a string")
                    .to_json()
            })?),
        };
        let bpm = match metadata.get("bpm") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => Some(value.as_f64().ok_or_else(|| {
                project_session::v2::SessionError::validation_field("bpm", "bpm must be a number")
                    .to_json()
            })?),
        };
        let session = Session::new_with_metadata(name, bpm, session_seed())
            .map_err(|error| project_session::v2::SessionError::validation(error).to_json())?;
        let snapshot = session.snapshot_json()?;
        self.doc = Some(session);
        self.plan = None;
        Ok(snapshot)
    }

    /// Close the current project Session while retaining voicebank/audio state.
    pub fn session_close(&mut self) {
        self.doc = None;
        self.plan = None;
    }

    /// Current opaque session ID, or an empty string when no session is open.
    pub fn session_id(&self) -> String {
        self.doc
            .as_ref()
            .map(|session| session.session_id().to_string())
            .unwrap_or_default()
    }

    /// Current committed revision, or zero when no session is open.
    pub fn session_revision(&self) -> u64 {
        self.doc.as_ref().map(Session::revision).unwrap_or(0)
    }

    /// Current full authoritative snapshot in the shared session schema.
    pub fn session_snapshot_json(&self) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .snapshot_json()
    }

    /// Warning events from opening this project, including sidecars dropped
    /// as malformed, stale, or belonging to another project.
    pub fn session_warnings_json(&self) -> Result<String, String> {
        let session = self.doc.as_ref().ok_or_else(|| "no project".to_string())?;
        let warnings: Vec<serde_json::Value> = session
            .warnings()
            .iter()
            .map(|warning| serde_json::json!({ "code": warning.code() }))
            .collect();
        serde_json::to_string(&warnings).map_err(|error| error.to_string())
    }

    /// Revision-tagged bounded project metadata/maps read shared with native HTTP.
    pub fn session_project_json(&self) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .project_json()
            .map_err(|error| error.to_json())
    }

    /// Revision-tagged bounded single-part read shared with native HTTP.
    pub fn session_part_json(&self, part_id: &str) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .part_json(part_id)
            .map_err(|error| error.to_json())
    }

    /// Capabilities implemented by the shared project-session command API.
    pub fn session_capabilities_json(&self) -> Result<String, String> {
        serde_json::to_string(&project_session::v2::capabilities())
            .map_err(|error| error.to_string())
    }

    /// Apply a stable-ID v2 command envelope. The returned JSON is the same
    /// logical mutation result as the native adapter, without HTTP fields.
    pub fn session_apply_v2(&mut self, request_json: &str) -> Result<String, String> {
        let session = self.doc.as_mut().ok_or_else(|| "no project".to_string())?;
        let result = session
            .apply_v2(request_json)
            .map_err(|error| error.to_json())?;
        self.plan = None;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    /// Begin an isolated v2 transaction group.
    pub fn session_group_begin(
        &mut self,
        expected_revision: u64,
        name: &str,
    ) -> Result<String, String> {
        let session = self.doc.as_mut().ok_or_else(|| "no project".to_string())?;
        let result = session
            .group_begin(expected_revision, name)
            .map_err(|error| error.to_json())?;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    /// Commit an isolated v2 transaction group.
    pub fn session_group_commit(
        &mut self,
        group_id: &str,
        expected_revision: u64,
    ) -> Result<String, String> {
        let session = self.doc.as_mut().ok_or_else(|| "no project".to_string())?;
        let result = session
            .group_commit(group_id, expected_revision)
            .map_err(|error| error.to_json())?;
        self.plan = None;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    /// Cancel an isolated v2 transaction group and return its current revision.
    pub fn session_group_cancel(&mut self, group_id: &str) -> Result<String, String> {
        let session = self.doc.as_mut().ok_or_else(|| "no project".to_string())?;
        session
            .group_cancel(group_id)
            .map_err(|error| error.to_json())?;
        serde_json::to_string(&serde_json::json!({
            "cancelled": true,
            "revision": session.revision(),
        }))
        .map_err(|error| error.to_string())
    }

    /// Undo through the shared v2 session and return the native logical result.
    pub fn session_undo_v2(&mut self, expected_revision: u64) -> Result<String, String> {
        self.session_history_v2(true, expected_revision)
    }

    /// Redo through the shared v2 session and return the native logical result.
    pub fn session_redo_v2(&mut self, expected_revision: u64) -> Result<String, String> {
        self.session_history_v2(false, expected_revision)
    }

    fn session_history_v2(&mut self, undo: bool, expected_revision: u64) -> Result<String, String> {
        let session = self.doc.as_mut().ok_or_else(|| "no project".to_string())?;
        let outcome = if undo {
            session.undo_v2(expected_revision)
        } else {
            session.redo_v2(expected_revision)
        }
        .map_err(|error| error.to_json())?;
        let result = match outcome {
            Some(outcome) => serde_json::json!({
                "revision": outcome.revision,
                "command_name": outcome.command_name,
                "changed": outcome.changed,
                "can_undo": outcome.can_undo,
                "can_redo": outcome.can_redo,
                "changes": [],
            }),
            None => serde_json::json!({
                "revision": session.revision(),
                "changed": false,
                "changes": [],
            }),
        };
        self.plan = None;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    /// Identity sidecar bytes to persist beside canonical YAML.
    pub fn session_save_sidecar(&mut self) -> Result<String, String> {
        self.doc
            .as_mut()
            .ok_or_else(|| "no project".to_string())?
            .save_sidecar()
    }

    /// Rusted UTAU pronunciation sidecar bytes to persist beside canonical YAML.
    pub fn session_save_pronunciation_sidecar(&self) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .save_pronunciation_sidecar()
    }

    /// Candidate composition through the shared Rust session semantics.
    pub fn session_phoneme_compose_v2(&self, request_json: &str) -> Result<String, String> {
        let session = self.doc.as_ref().ok_or_else(|| "no project".to_string())?;
        let composer = phonemizer::PhonemeComposer::new();
        session
            .phoneme_compose_json(request_json, &composer, self.voicebank.as_ref())
            .map_err(|error| error.to_json())
    }

    /// Canonical portable USTX export for the current Session.
    pub fn session_export_ustx(&mut self) -> Result<String, String> {
        Ok(self
            .doc
            .as_mut()
            .ok_or_else(|| "no project".to_string())?
            .yaml_snapshot()
            .to_string())
    }

    /// Prepare Save As bytes without changing live Session identity.
    pub fn session_prepare_save_as_json(&self) -> Result<String, String> {
        let payload = self
            .doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .prepare_save_as()
            .map_err(|error| project_session::v2::SessionError::validation(error).to_json())?;
        serde_json::to_string(&payload).map_err(|error| error.to_string())
    }

    /// Commit the identity fork after the host successfully persists both files.
    pub fn session_commit_save_as(&mut self) -> Result<String, String> {
        let session = self.doc.as_mut().ok_or_else(|| "no project".to_string())?;
        let project_id = session.commit_save_as_identity();
        serde_json::to_string(&serde_json::json!({
            "project_id": project_id,
            "revision": session.revision(),
        }))
        .map_err(|error| error.to_string())
    }

    /// Revision-tagged bounded read for a part's notes.
    pub fn session_part_notes_json(&self, part_id: &str) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .part_notes_json(part_id)
            .map_err(|error| error.to_json())
    }

    /// Revision-tagged bounded read for one curve.
    pub fn session_curve_json(&self, part_id: &str, curve_id: &str) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .curve_json(part_id, curve_id)
            .map_err(|error| error.to_json())
    }

    /// Revision-tagged read of one project's expression descriptor.
    pub fn session_expression_descriptor_json(&self, abbr: &str) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .expression_descriptor_json(abbr)
            .map_err(|error| error.to_json())
    }

    /// Revision-tagged curve base and descriptor context for pre-send stroke
    /// reduction. The subsequent command still goes through `session_apply_v2`.
    pub fn session_curve_stroke_context_json(
        &self,
        part_id: &str,
        curve_id: &str,
    ) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .curve_stroke_context_json(part_id, curve_id)
            .map_err(|error| error.to_json())
    }

    /// Revision-tagged descriptor/curve context, including a not-yet-created
    /// curve, for native/WASM parity before the first stroke.
    pub fn session_curve_stroke_context_for_abbr_json(
        &self,
        part_id: &str,
        abbr: &str,
        curve_id: Option<String>,
    ) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .curve_stroke_context_for_abbr_json(part_id, abbr, curve_id.as_deref())
            .map_err(|error| error.to_json())
    }

    /// Revision-tagged bounded read of track metadata.
    pub fn session_tracks_json(&self) -> Result<String, String> {
        self.doc
            .as_ref()
            .ok_or_else(|| "no project".to_string())?
            .tracks_json()
            .map_err(|error| error.to_json())
    }

    /// Apply one JSON command. Returns the undo-step name on success.
    pub fn apply_command(&mut self, json: &str) -> Result<String, String> {
        let doc = self
            .doc
            .as_mut()
            .ok_or_else(|| "no project (call project_new/project_load_yaml)".to_string())?;
        let name = doc.apply_legacy_command(json)?.unwrap_or_default();
        // Any successful edit invalidates the cached chunk plan.
        self.plan = None;
        Ok(name)
    }

    pub fn undo(&mut self) -> Result<String, String> {
        let doc = self.doc.as_mut().ok_or_else(|| "no project".to_string())?;
        let name = doc.undo()?.map(|outcome| outcome.command_name);
        self.plan = None;
        Ok(name.unwrap_or_default())
    }

    pub fn redo(&mut self) -> Result<String, String> {
        let doc = self.doc.as_mut().ok_or_else(|| "no project".to_string())?;
        let name = doc.redo()?.map(|outcome| outcome.command_name);
        self.plan = None;
        Ok(name.unwrap_or_default())
    }

    pub fn can_undo(&self) -> bool {
        self.doc.as_ref().map(|d| d.can_undo()).unwrap_or(false)
    }

    pub fn can_redo(&self) -> bool {
        self.doc.as_ref().map(|d| d.can_redo()).unwrap_or(false)
    }

    /// Notes of `track` as JSON (parts + notes) for the roll renderer.
    pub fn notes_json(&self, track: i32) -> Result<String, String> {
        self.doc
            .as_ref()
            .map(|d| d.legacy_notes_json(track))
            .ok_or_else(|| "no project".to_string())
    }

    /// Expression curves of `track` as JSON.
    pub fn curves_json(&self, track: i32) -> Result<String, String> {
        self.doc
            .as_ref()
            .map(|d| d.legacy_curves_json(track))
            .ok_or_else(|| "no project".to_string())
    }

    /// Track list (name/singer/mute/solo/volume/pan) as JSON.
    pub fn tracks_json(&self) -> Result<String, String> {
        self.doc
            .as_ref()
            .map(Session::legacy_tracks_json)
            .ok_or_else(|| "no project".to_string())
    }

    /// Project metadata (name/bpm/meter/resolution/counts) as JSON.
    pub fn meta_json(&self) -> Result<String, String> {
        self.doc
            .as_ref()
            .map(Session::legacy_meta_json)
            .ok_or_else(|| "no project".to_string())
    }

    /// Plan using the CURRENT document (no YAML text crossing the boundary).
    pub fn plan_document(&mut self, track: i32) -> Result<usize, String> {
        let yaml = self.project_yaml();
        if yaml.is_empty() {
            return Err("no project".to_string());
        }
        self.plan_project(&yaml, track)
    }

    /// Render every planned chunk sequentially (single-threaded baseline for
    /// the parallel path) and return the mixed PCM.
    pub fn render_planned_sequential(&self) -> Result<Vec<f32>, String> {
        let plan = self
            .plan
            .as_ref()
            .ok_or_else(|| "no plan (call plan_project first)".to_string())?;
        let renderer = MemoryRenderer {
            source: self.source.clone(),
        };
        let mut mixer = Mixer::new();
        for (index, planned) in plan.chunks.iter().enumerate() {
            let pcm = synth_cli::pipeline::render_planned_chunk(&plan.phrases, planned, &renderer)
                .map_err(|e| format!("chunk {index}: {e}"))?;
            mixer.add_chunk(&pcm, planned.position_ms, planned.leading_ms);
        }
        Ok(mixer.finish())
    }
}

/// Accumulates rendered chunks into the final mix.
///
/// A thin wrapper over `runtime::mixer::mix` so the browser never
/// reimplements the seam crossfade: adding chunks in index order produces
/// exactly what the single-threaded pipeline produces.
#[wasm_bindgen]
pub struct Mixer {
    inputs: Vec<MixInput>,
}

#[wasm_bindgen]
impl Mixer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Mixer {
        Mixer { inputs: Vec::new() }
    }

    /// Add one rendered chunk at its timeline position with default track
    /// parameters (0 dB, centre).
    ///
    /// `leading_ms` must match the plan (it is the chunk's crossfade zone);
    /// `position_ms` is where the audio starts minus the leading.
    pub fn add_chunk(&mut self, samples: &[f32], position_ms: f64, leading_ms: f64) {
        self.add_chunk_track(samples, position_ms, leading_ms, 0.0, 0.0);
    }

    /// Add one rendered chunk plus the track parameters it is mixed with.
    ///
    /// Volume/pan belong to the MIX stage, not to the chunk render, which is
    /// exactly why they are absent from `Chunk::hash`: moving a fader must
    /// re-mix the cached PCM and never re-render it. Passing them here is what
    /// makes the browser mix match the native one, where
    /// `runtime::mixer::mix` has always applied a `TrackSpec` per input.
    ///
    /// The post-mix FX chain is deliberately *not* applied here — see the
    /// streaming note in `Mixer::finish_from`.
    pub fn add_chunk_track(
        &mut self,
        samples: &[f32],
        position_ms: f64,
        leading_ms: f64,
        volume_db: f32,
        pan: f32,
    ) {
        self.inputs.push(MixInput {
            chunk: AudioChunk {
                samples: samples.to_vec(),
                position_ms,
                leading_ms,
                hash: self.inputs.len() as u64,
            },
            track: TrackSpec::new(volume_db, pan),
        });
    }

    /// Number of chunks added so far.
    pub fn chunk_count(&self) -> usize {
        self.inputs.len()
    }

    /// Mix everything added so far (mono f32 at 44.1 kHz).
    pub fn finish(&self) -> Vec<f32> {
        runtime::mixer::mix(&self.inputs).samples
    }

    /// Mix from chunk `start_index` to the end — the streaming playback entry
    /// point.
    ///
    /// A host that has rendered chunks `0..=start_index` can play this instead
    /// of waiting for the rest: the result is bit-identical to the tail of
    /// [`Mixer::finish`], so appending the next range reproduces one full mix.
    /// Chunks before `start_index` still participate in its crossfade, so a
    /// host must add them to the mixer too (it already has their PCM) — passing
    /// only the not-yet-played chunks will not give the same audio.
    ///
    /// `0` equals [`Mixer::finish`]; an index past the end yields empty audio.
    ///
    /// Track volume/pan are part of the mix, so a range mixed here already
    /// carries them. The post-mix FX chain (`mixer-fx`) is NOT: it is stateful
    /// (biquad memory + compressor envelope), so a range that starts mid-song
    /// cannot reproduce what a full render produces without processing the
    /// audio before it. A host that needs the FX must render from index 0 and
    /// keep only the tail it wants.
    pub fn finish_from(&self, start_index: usize) -> Vec<f32> {
        runtime::mixer::mix_from(&self.inputs, start_index).samples
    }

    /// Mix and return a 16-bit PCM WAV container.
    ///
    /// Worth having separately from [`Engine::render_project_wav`]: once a
    /// host has cached chunk PCM, exporting must not re-render the project.
    pub fn finish_wav(&self) -> Vec<u8> {
        wavwriter::write_wav_16(&self.finish(), 44100)
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Render one track through the shared pipeline.
fn render_track(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn synth_cli::pipeline::PhraseRenderer,
    track: i32,
) -> Result<Vec<f32>, String> {
    render_track_with_pronunciations(project, voicebank, renderer, track, None)
}

fn render_track_with_pronunciations(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn synth_cli::pipeline::PhraseRenderer,
    track: i32,
    pronunciations: Option<&[Vec<Option<domain::UPronunciation>>]>,
) -> Result<Vec<f32>, String> {
    let media = media::MemoryMediaSource::new();
    render_track_with_pronunciations_and_media(
        project,
        voicebank,
        renderer,
        track,
        pronunciations,
        &media,
    )
}

fn render_track_with_pronunciations_and_media(
    project: &UProject,
    voicebank: &Voicebank,
    renderer: &dyn synth_cli::pipeline::PhraseRenderer,
    track: i32,
    pronunciations: Option<&[Vec<Option<domain::UPronunciation>>]>,
    media: &dyn media::MediaSource,
) -> Result<Vec<f32>, String> {
    let track_ref = project
        .tracks
        .get(track as usize)
        .ok_or_else(|| format!("track {track} not found"))?;
    let kind = synth_cli::pipeline::PhonemizerKind::from_track(track_ref);
    let mut cache = None;
    let cancel = AtomicBool::new(false);
    let report = synth_cli::pipeline::render_track_with_media(
        project,
        voicebank,
        renderer,
        track,
        kind,
        false,
        &mut cache,
        media,
        None,
        Some(&cancel),
        None,
        None,
        pronunciations,
    )?;
    if report.samples.is_empty() && !report.skipped.is_empty() {
        return Err(format!("no audio produced: {}", report.skipped.join("; ")));
    }
    Ok(report.samples)
}
