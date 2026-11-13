use std::{path::Path, time::Instant};

use synth_cli::pipeline::{self, PhonemizerKind};
use worldline_plugin::r2::{self, Features};

fn dump(dir: &Path, name: &str, f: &Features) -> Result<(), Box<dyn std::error::Error>> {
    let out = dir.join(name);
    std::fs::create_dir_all(&out)?;
    std::fs::write(
        out.join("shape.txt"),
        format!("{} {} 1\n", f.f0.len(), f.sp_size),
    )?;
    for (name, values) in [
        ("f0", f.f0.as_slice()),
        ("sp", f.sp_env.as_slice()),
        ("ap", f.ap.as_slice()),
    ] {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for value in values {
            bytes.extend_from_slice(&(*value as f32).to_le_bytes());
        }
        std::fs::write(out.join(format!("{name}.f32le")), bytes)?;
    }
    Ok(())
}

fn analyze_batch(
    requests: &[worldline_plugin::PhonemeRequest],
    so_path: &Path,
    start: usize,
    end: usize,
    workers: usize,
) -> Result<Vec<worldline_plugin::r2::Segment>, Box<dyn std::error::Error>> {
    let worker_count = workers.max(1).min(end.saturating_sub(start).max(1));
    let mut ordered: Vec<Option<r2::Segment>> =
        std::iter::repeat_with(|| None).take(end - start).collect();
    let mut handles = Vec::new();
    for worker in 0..worker_count {
        let path = so_path.to_path_buf();
        let jobs: Vec<(usize, worldline_plugin::PhonemeRequest)> = (start..end)
            .filter(|index| (index - start) % worker_count == worker)
            .map(|index| (index, requests[index].clone()))
            .collect();
        handles.push(std::thread::spawn(move || -> Result<_, String> {
            let analysis = worldline_sys::Analysis::open(&path).map_err(|e| e.to_string())?;
            let mut completed = Vec::with_capacity(jobs.len());
            for (index, request) in jobs {
                let wav = voicebank::read_wav(Path::new(&request.wav_path))
                    .map_err(|e| format!("read {}: {e}", request.wav_path))?;
                let samples: Vec<f32> = wav.to_mono().into_iter().map(|x| x as f32).collect();
                let segment = r2::analyze_segment(&analysis, &request, &samples, 44100, 512, 2048)
                    .map_err(|e| format!("analyze {}: {e}", request.phoneme))?;
                completed.push((index, segment));
            }
            Ok(completed)
        }));
    }
    for handle in handles {
        let completed = handle
            .join()
            .map_err(|_| std::io::Error::other("feature worker panicked"))?
            .map_err(std::io::Error::other)?;
        for (index, segment) in completed {
            ordered[index - start] = Some(segment);
        }
    }
    ordered
        .into_iter()
        .map(|segment| {
            segment.ok_or_else(|| std::io::Error::other("feature worker omitted a request").into())
        })
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let project_path = args.next().ok_or("usage: <project.ustx> <voicebank> <worldline.so> <out-dir> [track] [phrase-start-ms] [prefix-requests] [publish-every] [prep-workers]")?;
    let bank_path = args.next().ok_or("missing voicebank")?;
    let so_path = args.next().ok_or("missing libworldline.so")?;
    let out_dir = std::path::PathBuf::from(args.next().ok_or("missing output directory")?);
    let track_no = args
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let target_ms = args
        .next()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let prefix_phones = args
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4)
        .max(1);
    let publish_every = args
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(prefix_phones)
        .max(1);
    let prep_workers = args
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(2)
        .max(1);

    let t0 = Instant::now();
    let project = pipeline::load_project(Path::new(&project_path))?;
    let bank = pipeline::load_voicebank(Path::new(&bank_path))?;
    let track = &project.tracks[track_no];
    let part = project
        .parts
        .iter()
        .find_map(|p| match p {
            domain::UPart::Voice(v) if v.track_no == track_no as i32 => Some(v),
            _ => None,
        })
        .ok_or("voice part missing for track")?;
    let phrases = pipeline::build_phrase_inputs(
        &project,
        track,
        part,
        &bank,
        PhonemizerKind::from_track(track),
    )?;
    let input = phrases
        .iter()
        .min_by(|a, b| {
            (a.input.phrase.position_ms - target_ms)
                .abs()
                .total_cmp(&(b.input.phrase.position_ms - target_ms).abs())
        })
        .ok_or("no phrase")?
        .input
        .clone();
    let requests = worldline_plugin::convert::build_requests(&input)?;
    if prefix_phones > requests.len() {
        return Err(format!(
            "prefix requests ({prefix_phones}) exceeds phrase request count ({})",
            requests.len()
        )
        .into());
    }
    let frame_ms = 512.0 * 1000.0 / 44100.0;
    let expected_frames = requests
        .iter()
        .map(|request| {
            ((request.pos_ms + request.length_ms) / frame_ms)
                .round()
                .max(0.0) as usize
                + 1
        })
        .max()
        .unwrap_or(0);
    let prep = Instant::now();
    let mut segments = Vec::with_capacity(requests.len());
    let mut prefix = None;
    let mut stage_index = 0usize;
    let mut processed = 0usize;
    while processed < requests.len() {
        let end = (processed + publish_every).min(requests.len());
        let batch = analyze_batch(&requests, Path::new(&so_path), processed, end, prep_workers)?;
        segments.extend(batch);
        processed = end;
        {
            let mut f = r2::synth_features(&segments, 2048);
            override_pitch(&input, &mut f);
            if prefix.is_none() && processed >= prefix_phones {
                dump(&out_dir, "prefix", &f)?;
                eprintln!(
                    "prefix prep: {} requests in {:.3} ms",
                    processed,
                    prep.elapsed().as_secs_f64() * 1000.0
                );
                prefix = Some(Features {
                    f0: f.f0.clone(),
                    sp_env: f.sp_env.clone(),
                    ap: f.ap.clone(),
                    sp_size: f.sp_size,
                });
            }
            let stage_name = format!("stage-{stage_index:04}");
            dump(&out_dir, &stage_name, &f)?;
            let stable_frames = requests
                .iter()
                .skip(processed)
                .map(|future| (future.pos_ms / frame_ms).round().max(0.0) as usize)
                .min()
                .unwrap_or(f.f0.len())
                .min(f.f0.len());
            println!(
                "LIVE_STAGE {stage_index} {processed} {} {stable_frames} {expected_frames} {}",
                f.f0.len(),
                out_dir.join(stage_name).display()
            );
            use std::io::Write;
            std::io::stdout().flush()?;
            stage_index += 1;
        }
    }
    let mut full = r2::synth_features(&segments, 2048);
    override_pitch(&input, &mut full);
    let prefix = prefix.ok_or("prefix-phones exceeds phrase phoneme count")?;
    dump(&out_dir, "full", &full)?;
    println!("LIVE_DONE phrase_start_ms={:.3} phrase_duration_ms={:.3} requests={} prefix_requests={} prefix_frames={} full_frames={} prep_all_ms={:.3} setup_load_plan_ms={:.3}",
        input.phrase.position_ms,input.phrase.duration_ms,requests.len(),prefix_phones,prefix.f0.len(),full.f0.len(),prep.elapsed().as_secs_f64()*1000.0,t0.elapsed().as_secs_f64()*1000.0-prep.elapsed().as_secs_f64()*1000.0);
    Ok(())
}

fn override_pitch(input: &feed::render_input::RenderInput, features: &mut Features) {
    let floor = 3.0 * 44100.0 / (2048.0 - 3.0);
    let curves = worldline_plugin::convert::sample_phrase_curves(input, 11.6);
    for (i, f0) in features.f0.iter_mut().enumerate() {
        if *f0 > floor {
            if let Some(value) = curves.f0.get(i) {
                if *value > 0.0 {
                    *f0 = *value;
                }
            }
        }
    }
}
