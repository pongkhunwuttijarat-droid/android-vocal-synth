//! Browser entry point for the Lilt render engine.
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

use domain::UProject;

mod project;
pub use project::ProjectDoc;
use runtime::mixer::{AudioChunk, MixInput, TrackSpec};
use voicebank::Voicebank;
use worldline_rs::MemorySource;

/// Engine discriminator for chunk hashing — "worldline-r-rs", the same value
/// the native `r1-rs` engine uses, so a chunk hashed on either side matches.
const CACHE_SALT: u64 = 0x726c31_7273_0004;

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
    warnings: Vec<String>,
    plan: Option<TrackPlan>,
    /// Rust-owned project document (edits go through reversible commands).
    doc: Option<ProjectDoc>,
}

#[wasm_bindgen]
impl Engine {
    /// Create an empty engine.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine {
            voicebank: None,
            source: MemorySource::new(),
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
        self.voicebank.as_ref().map(|v| v.oto_map.len()).unwrap_or(0)
    }

    /// Non-fatal warnings from the last voicebank load.
    pub fn warnings(&self) -> String {
        self.warnings.join("\n")
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
        self.source
            .insert_wav_bytes(wav_path, wav_bytes)
            
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
        let wav = voicebank::parse_wav(wav_bytes)
            .map_err(|e| format!("parse wav {wav_path}: {e}"))?;
        let frq_ok = matches!(voicebank::parse_frq(frq_bytes), Ok(f) if !f.f0.is_empty());
        let frq = if frq_ok { Some(frq_bytes.to_vec()) } else { None };
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
        let project = synth_cli::pipeline::parse_project(ustx_yaml)
            ?;
        let voicebank = self
            .voicebank
            .as_ref()
            .ok_or_else(|| "no voicebank installed (call set_voicebank)".to_string())?;
        let renderer = MemoryRenderer {
            source: self.source.clone(),
        };
        render_track(&project, voicebank, &renderer, track)
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
        let project = synth_cli::pipeline::parse_project(ustx_yaml)
            ?;
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
            synth_cli::pipeline::plan_track_chunks(&project, voicebank, track, kind)
                ?;
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
        let doc = ProjectDoc::new();
        let yaml = doc.yaml();
        self.doc = Some(doc);
        self.plan = None;
        yaml
    }

    /// Load a `.ustx` YAML project and make it current. Returns its YAML back
    /// (normalized), so a host can round-trip and compare.
    pub fn project_load_yaml(&mut self, yaml: &str) -> Result<String, String> {
        let doc = ProjectDoc::from_yaml(yaml)?;
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
    pub fn project_snapshot(&self) -> String {
        self.doc.as_ref().map(|d| d.snapshot().to_string()).unwrap_or_default()
    }

    /// Apply one JSON command. Returns the undo-step name on success.
    pub fn apply_command(&mut self, json: &str) -> Result<String, String> {
        let doc = self
            .doc
            .as_mut()
            .ok_or_else(|| "no project (call project_new/project_load_yaml)".to_string())?;
        let name = doc.apply(json)?;
        // Any successful edit invalidates the cached chunk plan.
        self.plan = None;
        Ok(name.unwrap_or_default())
    }

    pub fn undo(&mut self) -> Result<String, String> {
        let doc = self
            .doc
            .as_mut()
            .ok_or_else(|| "no project".to_string())?;
        let name = doc.undo()?;
        self.plan = None;
        Ok(name.unwrap_or_default())
    }

    pub fn redo(&mut self) -> Result<String, String> {
        let doc = self
            .doc
            .as_mut()
            .ok_or_else(|| "no project".to_string())?;
        let name = doc.redo()?;
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
            .map(|d| d.notes_json(track))
            .ok_or_else(|| "no project".to_string())
    }

    /// Expression curves of `track` as JSON.
    pub fn curves_json(&self, track: i32) -> Result<String, String> {
        self.doc
            .as_ref()
            .map(|d| d.curves_json(track))
            .ok_or_else(|| "no project".to_string())
    }

    /// Track list (name/singer/mute/solo/volume/pan) as JSON.
    pub fn tracks_json(&self) -> Result<String, String> {
        self.doc
            .as_ref()
            .map(|d| d.tracks_json())
            .ok_or_else(|| "no project".to_string())
    }

    /// Project metadata (name/bpm/meter/resolution/counts) as JSON.
    pub fn meta_json(&self) -> Result<String, String> {
        self.doc
            .as_ref()
            .map(|d| d.meta_json())
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

    /// Add one rendered chunk at its timeline position.
    ///
    /// `leading_ms` must match the plan (it is the chunk's crossfade zone);
    /// `position_ms` is where the audio starts minus the leading.
    pub fn add_chunk(&mut self, samples: &[f32], position_ms: f64, leading_ms: f64) {
        self.inputs.push(MixInput {
            chunk: AudioChunk {
                samples: samples.to_vec(),
                position_ms,
                leading_ms,
                hash: self.inputs.len() as u64,
            },
            track: TrackSpec::default(),
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
    let track_ref = project
        .tracks
        .get(track as usize)
        .ok_or_else(|| format!("track {track} not found"))?;
    let kind = synth_cli::pipeline::PhonemizerKind::from_track(track_ref);
    let mut cache = None;
    let cancel = AtomicBool::new(false);
    let report = synth_cli::pipeline::render_project(
        project,
        voicebank,
        renderer,
        track,
        kind,
        false,
        &mut cache,
        None,
        Some(&cancel),
    )
    ?;
    if report.samples.is_empty() && !report.skipped.is_empty() {
        return Err(format!(
            "no audio produced: {}",
            report.skipped.join("; ")
        ));
    }
    Ok(report.samples)
}
