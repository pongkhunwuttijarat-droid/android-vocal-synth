#!/usr/bin/env python3
"""Probe per-block content caching after a localized R2 feature edit.

This is an isolated experiment over a real project's extracted R2 features.
It deliberately compares a naive local-input cache key with actual full-model
PCM output. It does not modify the project or production renderer.
"""
import argparse
import hashlib
import json
import time
from pathlib import Path

import numpy as np
import onnxruntime as ort


def load_features(folder: Path):
    frames, bins, _ = map(int, (folder / "shape.txt").read_text().split())
    f0 = np.fromfile(folder / "f0.f32le", "<f4").reshape(1, frames)
    sp = np.fromfile(folder / "sp.f32le", "<f4").reshape(1, frames, bins)
    ap = np.fromfile(folder / "ap.f32le", "<f4").reshape(1, frames, bins)
    return f0, sp, ap


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def similarity(a: np.ndarray, b: np.ndarray):
    d = a.astype(np.float64) - b.astype(np.float64)
    den = np.linalg.norm(a.astype(np.float64)) * np.linalg.norm(b.astype(np.float64))
    corr = float(np.dot(a.astype(np.float64), b.astype(np.float64)) / max(den, 1e-30))
    return corr, float(np.sqrt(np.mean(d * d))), float(np.max(np.abs(d)))


def infer(f0, sp, ap, mel_session, voc_session):
    mel = mel_session.run(None, {"f0": f0, "sp_env": sp, "ap": ap})[0]
    return voc_session.run(None, {"mel": mel, "f0": f0})[0].reshape(-1)


def local_key(f0, sp, ap, block, core, context, model_key):
    start = max(0, block * core - context)
    end = min(f0.shape[1], (block + 1) * core + context)
    digest = hashlib.sha256(model_key.encode())
    for values in (f0[:, start:end], sp[:, start:end], ap[:, start:end]):
        digest.update(np.ascontiguousarray(values).tobytes())
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--features", type=Path, required=True)
    parser.add_argument("--models", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--edit-frame", type=int, default=320)
    parser.add_argument("--edit-length", type=int, default=16)
    parser.add_argument("--core-frames", type=int, default=64)
    parser.add_argument("--context-frames", type=int, default=96,
                        help="conservative finite-context window used by the naive cache key")
    args = parser.parse_args()

    mel_path = args.models / "mel.onnx"
    voc_path = args.models / "pc-nsf-hifigan" / "pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx"
    f0, sp, ap = load_features(args.features)
    edited_f0 = f0.copy()
    start = max(0, min(args.edit_frame, f0.shape[1] - 1))
    end = min(f0.shape[1], start + max(1, args.edit_length))
    voiced = edited_f0[0, start:end] > 0
    if not np.any(voiced):
        candidates = np.flatnonzero(edited_f0[0] > 0)
        if not len(candidates):
            raise RuntimeError("fixture contains no voiced F0 frames")
        start = int(candidates[len(candidates) // 2])
        end = min(f0.shape[1], start + max(1, args.edit_length))
        voiced = edited_f0[0, start:end] > 0
    edited_f0[0, start:end][voiced] *= np.float32(1.01)

    options = ort.SessionOptions()
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1
    providers = ["CUDAExecutionProvider", "CPUExecutionProvider"]
    setup = time.perf_counter()
    mel_session = ort.InferenceSession(str(mel_path), options, providers=providers)
    voc_session = ort.InferenceSession(str(voc_path), options, providers=providers)
    setup_ms = (time.perf_counter() - setup) * 1000
    if mel_session.get_providers()[0] != "CUDAExecutionProvider" or \
            voc_session.get_providers()[0] != "CUDAExecutionProvider":
        raise RuntimeError("CUDA provider unavailable; refusing to report GPU timings")

    warm_f0 = np.zeros((1, 128), np.float32)
    warm_sp = np.full((1, 128, sp.shape[2]), 1e-12, np.float32)
    warm_ap = np.ones((1, 128, ap.shape[2]), np.float32)
    infer(warm_f0, warm_sp, warm_ap, mel_session, voc_session)

    t0 = time.perf_counter()
    baseline_pcm = infer(f0, sp, ap, mel_session, voc_session)
    baseline_ms = (time.perf_counter() - t0) * 1000
    t1 = time.perf_counter()
    edited_pcm = infer(edited_f0, sp, ap, mel_session, voc_session)
    edited_ms = (time.perf_counter() - t1) * 1000
    if len(baseline_pcm) != len(edited_pcm):
        raise RuntimeError("edit changed output length; this probe only models fixed-length edits")

    frame_count = f0.shape[1]
    blocks = (frame_count + args.core_frames - 1) // args.core_frames
    model_key = f"{sha256(mel_path)}:{sha256(voc_path)}:44100:{args.core_frames}"
    before_hits, after_hits, invalidated_context = [], [], []
    audio_rows = []
    edit_block = start // args.core_frames
    for block in range(blocks):
        old_key = local_key(f0, sp, ap, block, args.core_frames,
                            args.context_frames, model_key)
        new_key = local_key(edited_f0, sp, ap, block, args.core_frames,
                            args.context_frames, model_key)
        pcm_start = block * args.core_frames * 512
        pcm_end = min(len(baseline_pcm), (block + 1) * args.core_frames * 512)
        corr, rmse, peak = similarity(baseline_pcm[pcm_start:pcm_end],
                                      edited_pcm[pcm_start:pcm_end])
        row = {"block": block, "seconds": round(pcm_start / 44100, 3),
               "local_key_hit": old_key == new_key, "corr": corr,
               "rmse": rmse, "max_abs": peak}
        audio_rows.append(row)
        if old_key == new_key and block < edit_block:
            before_hits.append(block)
        elif old_key == new_key:
            after_hits.append(block)
        else:
            invalidated_context.append(block)

    first_changed_block = next(
        (row["block"] for row in audio_rows if row["max_abs"] > 0.0), blocks
    )
    exact_prefix_blocks = first_changed_block
    prefix_samples = min(len(baseline_pcm), first_changed_block * args.core_frames * 512)
    prefix_reuse_pcm = np.concatenate((baseline_pcm[:prefix_samples], edited_pcm[prefix_samples:]))
    prefix_reuse_error = float(np.max(np.abs(prefix_reuse_pcm - edited_pcm)))

    args.out.mkdir(parents=True, exist_ok=True)
    report = {
        "fixture": str(args.features), "frames": frame_count,
        "audio_seconds": round(frame_count * 512 / 44100, 3),
        "edit": {"frame_start": start, "frame_end_exclusive": end,
                 "seconds_start": round(start * 512 / 44100, 3),
                 "f0_change": "multiply voiced frames by 1.01"},
        "block_frames": args.core_frames,
        "local_key_context_frames_each_side": args.context_frames,
        "session_setup_ms": setup_ms, "baseline_full_inference_ms": baseline_ms,
        "edited_full_inference_ms": edited_ms,
        "unchanged_prefix_pcm_cache_candidates": before_hits,
        "exact_cached_prefix_blocks": exact_prefix_blocks,
        "first_pcm_changed_block": first_changed_block,
        "prefix_reuse_reconstruction_max_abs": prefix_reuse_error,
        "unchanged_suffix_pcm_cache_candidates_by_local_input_only": after_hits,
        "local_input_window_invalidated_blocks": invalidated_context,
        "pcm_by_block": audio_rows,
        "interpretation": "A local-input key omits cumulative phase state. Suffix candidates whose PCM changed need a phase-state dependency in their key or must be rerendered.",
    }
    (args.out / "cache-edit-report.json").write_text(json.dumps(report, indent=2) + "\n")
    np.save(args.out / "edited_pcm.npy", edited_pcm)
    np.save(args.out / "prefix-reuse-edited-pcm.npy", prefix_reuse_pcm)
    print(json.dumps({k: report[k] for k in (
        "frames", "audio_seconds", "edit", "session_setup_ms",
        "baseline_full_inference_ms", "edited_full_inference_ms",
        "unchanged_prefix_pcm_cache_candidates",
        "exact_cached_prefix_blocks", "first_pcm_changed_block",
        "prefix_reuse_reconstruction_max_abs",
        "unchanged_suffix_pcm_cache_candidates_by_local_input_only",
        "local_input_window_invalidated_blocks")}, indent=2))
    for row in audio_rows:
        print(f"PCM_BLOCK {row['block']:02d} t={row['seconds']:.3f}s "
              f"local_key_hit={row['local_key_hit']} corr={row['corr']:.9f} "
              f"rmse={row['rmse']:.7g} max_abs={row['max_abs']:.7g}")


if __name__ == "__main__":
    main()
