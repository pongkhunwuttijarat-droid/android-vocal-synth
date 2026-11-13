//! DiffSinger duration predictor — port of the C# `DiffSingerBasePhonemizer`
//! duration flow (linguistic.onnx + dur.onnx + the note-bound stretch):
//! the bank's `dsdur/` (or the `linguistic`/`dur` dsconfig fields). When a
//! bank has it, the predicted durations replace the note-timed ones — the
//! acoustic model then sees EXACTLY what the OpenUtau backend feeds it.

use std::path::Path;
use std::sync::Mutex;

use feed::render_input::RenderInput;
use ort::session::Session;

use crate::config::DsConfig;

/// ms per tick (120 bpm × 480 ticks/beat — the feed's default tempo).
pub fn ms_per_tick(_frame_ms: f64) -> f64 {
    60000.0 / (120.0 * 480.0)
}

/// The loaded dsdur (linguistic + duration sessions).
pub struct DsDur {
    linguistic: Mutex<Session>,
    duration: Mutex<Session>,
    /// The duration model's speaker embed (the dsdur's `speakers` list —
    /// e.g. embeds/variance_no7.emb, hidden_size f32s).
    spk_embed: Option<Vec<f32>>,
    /// The duration model's input names (spk_embed detection).
    dur_inputs: Vec<String>,
}

fn open_session(path: &Path, what: &str) -> Result<Session, String> {
    if !path.is_file() {
        return Err(format!("{what} model not found: {}", path.display()));
    }
    let mut builder = Session::builder().map_err(|e| format!("{what} builder: {e}"))?;
    // Android GPU/NPU via the NNAPI EP (CPU fallback per-op — the
    // diffusion models' exotic ops fall back automatically).
    #[cfg(target_os = "android")]
    {
        builder = builder
            .with_execution_providers([ort::execution_providers::NNAPI::default().build()])
            .map_err(|e| format!("{what} NNAPI EP: {e}"))?;
    }
    builder
        .commit_from_file(path)
        .map_err(|e| format!("load {what} {}: {e}", path.display()))
}

impl DsDur {
    /// Load the dsdur: the bank declares it either in the MAIN
    /// dsconfig.yaml (`linguistic` + `dur`) or — the modern convention —
    /// in the `dsdur/dsconfig.yaml` (paths relative to the dsdur dir,
    /// e.g. `linguistic: ../dsmain/linguistic.onnx`). Banks without a
    /// duration predictor get `None` and keep the note-timed durations.
    pub fn load(model_dir: &Path, config: &DsConfig) -> Result<Option<Self>, String> {
        // The dsdur/ subdir's own dsconfig (modern banks) — also carries
        // the dsdur speaker embed list.
        let sub = model_dir.join("dsdur");
        let sub_config: Option<DsConfig> = serde_yaml::from_str(
            &std::fs::read_to_string(sub.join("dsconfig.yaml"))
                .map_err(|_| String::new())
                .unwrap_or_default(),
        )
        .ok();
        let (ling, dur) = match (&config.linguistic, &config.dur) {
            (Some(l), Some(d)) => (model_dir.join(l), model_dir.join(d)),
            _ => match sub_config.as_ref() {
                Some(c) if c.linguistic.is_some() && c.dur.is_some() => (
                    sub.join(c.linguistic.as_deref().unwrap_or("")),
                    sub.join(c.dur.as_deref().unwrap_or("")),
                ),
                _ => return Ok(None),
            },
        };
        let linguistic = open_session(&ling, "linguistic")?;
        let duration = open_session(&dur, "duration")?;
        let dur_inputs: Vec<String> = duration
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        // The dsdur's own speaker embed (the dsdur/dsconfig speakers list
        // — "<dir>/<name>.emb" relative to the dsdur dir).
        let spk_embed = match &sub_config {
            Some(c) => c
                .speakers
                .first()
                .map(|spk| {
                    let path = sub.join(format!("{spk}.emb"));
                    let bytes = std::fs::read(&path)
                        .map_err(|e| format!("dsdur speaker embed {}: {e}", path.display()))?;
                    let hidden = c.hidden_size.max(1) as usize;
                    if bytes.len() < hidden * 4 {
                        return Err(format!(
                            "dsdur speaker embed {}: {} bytes, expected {}",
                            path.display(),
                            bytes.len(),
                            hidden * 4
                        ));
                    }
                    let mut v = Vec::with_capacity(hidden);
                    for i in 0..hidden {
                        v.push(f32::from_le_bytes([
                            bytes[i * 4],
                            bytes[i * 4 + 1],
                            bytes[i * 4 + 2],
                            bytes[i * 4 + 3],
                        ]));
                    }
                    Ok(v)
                })
                .transpose()?,
            None => None,
        };
        Ok(Some(DsDur {
            linguistic: Mutex::new(linguistic),
            duration: Mutex::new(duration),
            spk_embed,
            dur_inputs,
        }))
    }

    /// Predict the per-phoneme durations (ms) — the C# flow:
    /// tokens/word_div/word_dur → linguistic → encoder_out/x_masks →
    /// +ph_midi → dur → frames → stretch to the note bounds.
    /// `tokens` = the padded tokens ([SP] + phonemes, no tail token),
    /// `tones` = the matching per-token MIDI tones (the C#
    /// SelectMany(Repeat(tone, count)) — the SP's tone included), and
    /// `token_counts` = the per-phoneme dict/letter-split widths.
    pub fn predict(
        &self,
        input: &RenderInput,
        tokens: &[i64],
        tones: &[i64],
        token_counts: &[usize],
        table: &std::collections::HashMap<String, i64>,
        vowels: &[String],
        frame_ms: f64,
    ) -> Result<Vec<f64>, String> {
        if tokens.len() != tones.len() {
            return Err(format!(
                "dsdur: {} tokens vs {} tones",
                tokens.len(),
                tones.len()
            ));
        }
        if input.phonemes.len() != token_counts.len() {
            return Err(format!(
                "dsdur: {} phonemes vs {} token counts",
                input.phonemes.len(),
                token_counts.len()
            ));
        }
        // --- C#-exact groups (ProcessPart/ProcessWord): vowel tokens
        // start a NEW note group; consonants join the CURRENT group (the
        // C# appends each word's onset to the PREVIOUS note's group).
        // Token 0 (SP) forms the leading group.
        let mut tok_to_sym: std::collections::HashMap<i64, String> =
            std::collections::HashMap::new();
        for (k, v) in table {
            tok_to_sym.entry(*v).or_insert_with(|| k.clone());
        }
        let mut groups: Vec<Vec<usize>> = vec![vec![0]];
        let mut group_counts: Vec<usize> = vec![1];
        let mut group_pos_ms: Vec<f64> = vec![-1.0 * ms_per_tick(frame_ms)]; // C# SP group @ tick -1
        let mut cur = 0usize;
        let mut ti = 1usize; // token 0 = the SP
        for (pi, ph) in input.phonemes.iter().enumerate() {
            let n = token_counts[pi];
            for k in 0..n {
                let t = tokens[ti + k];
                let sym = tok_to_sym.get(&t).cloned().unwrap_or_default();
                if vowels.contains(&sym) {
                    groups.push(vec![ti + k]);
                    group_counts.push(1);
                    group_pos_ms.push(ph.position_ms);
                    cur = groups.len() - 1;
                } else {
                    groups[cur].push(ti + k);
                    group_counts[cur] += 1;
                }
            }
            ti += n;
        }
        if groups.is_empty() {
            return Err("dsdur: no phonemes".to_string());
        }
        let n = tokens.len();

        // word_div: the C# counts per group INCLUDING the leading SP group
        // ([1] + the per-note DICT-token counts); word_dur: frames between
        // word_div: the counts per group (the C# word_div — the SP group
        // + each note group); word_dur: the frames between the group
        // positions (framesBetweenTickPos — the SP group sits at tick -1).
        let word_div: Vec<i64> = group_counts.iter().map(|&c| c as i64).collect();
        let frames_at = |ms_v: f64| (ms_v / frame_ms) as i64; // C# (int) — truncation, not floor
                                                              // The phrase end (the C# endTick): the last phoneme's end.
        let phrase_end_ms = input
            .phonemes
            .last()
            .map(|p| p.position_ms + p.duration_ms)
            .unwrap_or(0.0);
        let mut word_dur: Vec<i64> = Vec::with_capacity(groups.len());
        for w in group_pos_ms.windows(2) {
            word_dur.push(frames_at(w[1]) - frames_at(w[0]));
        }
        if word_dur.is_empty() {
            word_dur.push(1);
        }
        let n = tokens.len();
        let w = word_div.len(); // the padded word count (== word_dur after padding)
        let mut wd = word_dur.clone();
        if wd.len() < word_div.len() {
            // The last window = the final group → the TAIL (the phrase
            // end — the C# endTick group at the last note's end).
            let last_group_ms = group_pos_ms.last().copied().unwrap_or(0.0);
            wd.push(frames_at(phrase_end_ms) - frames_at(last_group_ms));
        }
        wd.truncate(word_div.len());
        let word_dur: Vec<i64> = wd;

        // --- linguistic: tokens[1,N], word_div[1,W], word_dur[1,W] ---
        let n = tokens.len();
        let w = word_dur.len().max(1);
        let mut wd = word_dur.clone();
        if wd.is_empty() {
            wd.push(1);
        }
        let tokens_val =
            ort::value::Tensor::from_array(([1usize, n], tokens.to_vec().into_boxed_slice()))
                .map_err(|e| format!("tokens tensor: {e}"))?;
        let div_val =
            ort::value::Tensor::from_array(([1usize, w], word_div.clone().into_boxed_slice()))
                .map_err(|e| format!("word_div tensor: {e}"))?;
        let dur_val = ort::value::Tensor::from_array(([1usize, w], wd.into_boxed_slice()))
            .map_err(|e| format!("word_dur tensor: {e}"))?;
        let mut ling = self
            .linguistic
            .lock()
            .map_err(|e| format!("linguistic lock: {e}"))?;
        let l_out = ling
            .run(ort::inputs![tokens_val, div_val, dur_val])
            .map_err(|e| format!("linguistic run: {e}"))?;
        // encoder_out: the FIRST output (the C# filters by name).
        let encoder = l_out[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("encoder extract: {e}"))?;
        let enc_shape: Vec<usize> = encoder.0.iter().map(|&d| d as usize).collect();
        let enc_dim: Vec<i64> = if enc_shape.len() == 3 {
            vec![enc_shape[1] as i64, enc_shape[2] as i64]
        } else {
            vec![n as i64, enc_shape.last().copied().unwrap_or(256) as i64]
        };
        let enc_data: Vec<f32> = encoder.1.to_vec();
        let enc_val = ort::value::Tensor::from_array((
            [1usize, enc_dim[0] as usize, enc_dim[1] as usize],
            enc_data.into_boxed_slice(),
        ))
        .map_err(|e| format!("encoder tensor: {e}"))?;
        // x_masks: the SECOND output.
        let masks = l_out[1]
            .try_extract_tensor::<bool>()
            .map_err(|e| format!("masks extract: {e}"))?;
        let masks_data: Vec<bool> = masks.1.to_vec();
        let masks_val = ort::value::Tensor::from_array((
            [1usize, masks_data.len()],
            masks_data.into_boxed_slice(),
        ))
        .map_err(|e| format!("masks tensor: {e}"))?;

        // --- ph_midi: the tone per TOKEN (the C# SelectMany repeat) ---
        let midi_data: Vec<i64> = tones.to_vec();
        let midi_val = ort::value::Tensor::from_array((
            [1usize, midi_data.len()],
            midi_data.into_boxed_slice(),
        ))
        .map_err(|e| format!("ph_midi tensor: {e}"))?;

        // --- dur: encoder_out, x_masks, ph_midi (+ spk_embed) → frames ---
        let has_dur_spk = self.dur_inputs.iter().any(|n| n == "spk_embed");
        let mut dur_inputs_vec: Vec<(
            std::borrow::Cow<'static, str>,
            ort::session::SessionInputValue,
        )> = vec![
            ("encoder_out".into(), enc_val.into()),
            ("x_masks".into(), masks_val.into()),
            ("ph_midi".into(), midi_val.into()),
        ];
        if has_dur_spk {
            let embed = self.spk_embed.as_ref().ok_or_else(|| {
                "duration model wants spk_embed but the bank has no dsdur speaker embed".to_string()
            })?;
            // Per-frame: [1, N, H] (the C# broadcasts per token).
            let mut spk = Vec::with_capacity(n * embed.len());
            for _ in 0..n {
                spk.extend_from_slice(embed);
            }
            let v =
                ort::value::Tensor::from_array(([1usize, n, embed.len()], spk.into_boxed_slice()))
                    .map_err(|e| format!("dsdur spk_embed tensor: {e}"))?;
            dur_inputs_vec.push(("spk_embed".into(), v.into()));
        }
        let mut dur_s = self
            .duration
            .lock()
            .map_err(|e| format!("duration lock: {e}"))?;
        let d_out = dur_s
            .run(dur_inputs_vec)
            .map_err(|e| format!("duration run: {e}"))?;
        let frames_out = d_out[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("durations extract: {e}"))?;
        let predicted_frames: Vec<f32> = frames_out.1.to_vec();
        // The C# expects the same length as the tokens (SP + phonemes).
        if predicted_frames.len() < n {
            return Err(format!(
                "duration model returned {} frames for {n} tokens",
                predicted_frames.len()
            ));
        }

        // --- stretch (C# lines 440-458) → token positions (ms) ---
        // align points: (token-index boundary, position-ms) — the C#
        // cumsum of the group counts zipped with the NEXT group's
        // position; the final boundary at the last note's end.
        let tail_end_ms = input
            .phonemes
            .last()
            .map(|p| p.position_ms + p.duration_ms)
            .unwrap_or(0.0);
        let mut align_points: Vec<(usize, f64)> = Vec::with_capacity(groups.len());
        let mut cum = 0usize;
        for (i, _g) in groups.iter().enumerate() {
            cum += group_counts[i];
            let next_pos = if i + 1 < group_pos_ms.len() {
                group_pos_ms[i + 1]
            } else {
                tail_end_ms
            };
            align_points.push((cum, next_pos));
        }
        if align_points.is_empty() {
            return Err("dsdur: no align points".to_string());
        }
        if std::env::var("WORLD_DBG_DSDUR").is_ok() {
            eprintln!(
                "[dsdur] align={:?} group_pos_ms={:?} group_counts={:?}",
                align_points, group_pos_ms, group_counts
            );
        }
        let duration_frames: Vec<f64> = predicted_frames.iter().map(|&v| v as f64).collect();
        // positions: the ms position of every phoneme (the C# stretch).
        let stretch = |source: &[f64], ratio: f64, end_pos: f64| -> Vec<f64> {
            let start = end_pos - source.iter().sum::<f64>() * ratio;
            let mut result = Vec::with_capacity(source.len());
            let mut acc = start;
            for &x in source {
                acc += x * ratio;
                result.push(acc);
            }
            result
        };
        let mut positions: Vec<f64> = Vec::with_capacity(input.phonemes.len());
        // First group: the phonemes BEFORE the first align point stretch
        // with frameMs (the C#: alignGroup [1..first-1], ratio frameMs).
        let first_count = align_points[0].0;
        let first_group: Vec<f64> = duration_frames
            .iter()
            .skip(1)
            .take(first_count.saturating_sub(1))
            .copied()
            .collect();
        // The C#'s first group (the onset) stretches with ratio=frameMs
        // from the PHRASE START — its duration includes the leading
        // (preutter): the C# s@-103ms (start) for a 92.9ms leading + the
        // predicted s frame.
        let first_group_start = align_points[0].1
            - input.phrase.leading_ms
            - first_group.iter().sum::<f64>() * frame_ms;
        positions.extend(stretch(&first_group, frame_ms, align_points[0].1));
        for pair in align_points.windows(2) {
            let (curr, next) = (pair[0], pair[1]);
            let group: Vec<f64> = duration_frames
                .iter()
                .skip(curr.0)
                .take(next.0 - curr.0)
                .copied()
                .collect();
            let sum: f64 = group.iter().sum();
            let ratio = if sum > 0.0 {
                (next.1 - curr.1) / sum
            } else {
                frame_ms
            };
            positions.extend(stretch(&group, ratio, next.1));
        }
        // Convert the positions to per-phoneme durations (ms). The
        // positions are the phoneme END times (the C# stretch emits
        // ends): d[i] = pos[i] − pos[i−1], the first = pos[0] − the
        // first group's stretch start.
        let mut durations = Vec::with_capacity(positions.len());
        let mut prev = first_group_start;
        for pos in positions {
            durations.push((pos - prev).max(0.0));
            prev = pos;
        }
        Ok(durations)
    }
}
