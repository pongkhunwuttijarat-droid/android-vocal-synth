#!/usr/bin/env python3
"""Live Rust-feature producer -> persistent ONNX -> bounded PCM consumer POC.

The Rust child publishes monotonically growing feature snapshots and continues
phoneme analysis while this process runs the next eligible model chunks. Model
sessions are loaded before the play/TTFS clock starts. Playback is represented
by an ordered in-memory PCM consumer with a real-time deadline simulation.
"""
import argparse
import subprocess
import sys
import time
import wave
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
from onnx import TensorProto, helper


def instrument(source: Path, node_name: str, state_name: str, output: Path) -> None:
    model = onnx.load(source)
    node = next(n for n in model.graph.node if n.op_type == "CumSum" and n.name == node_name)
    raw = node.output[0] + "_raw"
    node.output[0] = raw
    index = list(model.graph.node).index(node) + 1
    model.graph.node.insert(index, helper.make_node("Add", [raw, "phase_offset"],
                                                     [raw.removesuffix("_raw")], name="PhaseCarryAdd"))
    output_name = raw.removesuffix("_raw")
    model.graph.node.append(helper.make_node("Identity", [output_name], [state_name],
                                              name="ExposePhaseState"))
    model.graph.input.append(helper.make_tensor_value_info("phase_offset", TensorProto.FLOAT, []))
    model.graph.output.append(helper.make_tensor_value_info(state_name, TensorProto.FLOAT,
                                                             [1, "phase_len", 1]))
    onnx.checker.check_model(model)
    onnx.save(model, output)


def metrics(a: np.ndarray, b: np.ndarray) -> str:
    x = a.reshape(-1).astype(np.float64)
    y = b.reshape(-1).astype(np.float64)
    d = x - y
    corr = np.dot(x, y) / max(np.linalg.norm(x) * np.linalg.norm(y), 1e-30)
    return f"corr={corr:.9f} rmse={np.sqrt(np.mean(d*d)):.8g} max_abs={np.max(np.abs(d)):.8g}"


def read_features(folder: Path):
    n, sp, _ = map(int, (folder / "shape.txt").read_text().split())
    f0 = np.fromfile(folder / "f0.f32le", "<f4").reshape(1, n)
    env = np.fromfile(folder / "sp.f32le", "<f4").reshape(1, n, sp)
    ap = np.fromfile(folder / "ap.f32le", "<f4").reshape(1, n, sp)
    return f0, env, ap


def write_wav(path: Path, pcm: np.ndarray) -> None:
    samples = np.clip(pcm.reshape(-1), -1.0, 1.0)
    pcm16 = (samples * 32767.0).astype("<i2")
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(44100)
        wav.writeframes(pcm16.tobytes())


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--rust-bin", type=Path, required=True)
    p.add_argument("--project", type=Path, required=True)
    p.add_argument("--voicebank", type=Path, required=True)
    p.add_argument("--worldline", type=Path, required=True)
    p.add_argument("--models", type=Path, default=Path("models/r2"))
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--track", type=int, default=0)
    p.add_argument("--phrase-ms", type=float, default=0.0)
    p.add_argument("--prefix-requests", type=int, default=8)
    p.add_argument("--publish-every", type=int, default=8)
    p.add_argument("--mel-core", type=int, default=64)
    p.add_argument("--mel-context", type=int, default=32)
    p.add_argument("--vocoder-core", type=int, default=64)
    p.add_argument("--vocoder-context", type=int, default=64)
    p.add_argument("--first-pcm-frames", type=int, default=0,
                    help="optional short bootstrap block before switching to steady-state context")
    p.add_argument("--steady-mel-core", type=int, default=64)
    p.add_argument("--steady-vocoder-core", type=int, default=64)
    p.add_argument("--prep-workers", type=int, default=2)
    p.add_argument("--skip-warmup", action="store_true",
                    help="include each ONNX session's first-call setup in the Play-to-PCM measurement")
    args = p.parse_args()

    args.out.mkdir(parents=True, exist_ok=True)
    work = args.out / "graphs"
    work.mkdir(exist_ok=True)
    mel_graph = work / "mel-phase-carry.onnx"
    voc_graph = work / "vocoder-phase-carry.onnx"
    mel_source = args.models / "mel.onnx"
    voc_source = args.models / "pc-nsf-hifigan" / "pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx"
    instrument(mel_source, "/harmonic_synthsizer/CumSum", "phase_state", mel_graph)
    instrument(voc_source, "/generator/CumSum", "phase_state", voc_graph)

    options = ort.SessionOptions()
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1
    providers = ["CUDAExecutionProvider", "CPUExecutionProvider"]
    setup_t = time.perf_counter()
    mel_session = ort.InferenceSession(str(mel_graph), options, providers=providers)
    voc_session = ort.InferenceSession(str(voc_graph), options, providers=providers)
    setup_ms = (time.perf_counter() - setup_t) * 1000
    active_providers = f"mel={mel_session.get_providers()[0]} vocoder={voc_session.get_providers()[0]}"
    print(f"sessions_ready_ms={setup_ms:.2f} {active_providers}", flush=True)
    if mel_session.get_providers()[0] != "CUDAExecutionProvider" or \
            voc_session.get_providers()[0] != "CUDAExecutionProvider":
        raise RuntimeError("CUDA provider unavailable; refusing to report GPU TTFS")
    warmup_ms = 0.0
    if not args.skip_warmup:
        warm_t = time.perf_counter()
        warm_f0 = np.zeros((1, 128), np.float32)
        warm_sp = np.full((1, 128, 1025), 1e-12, np.float32)
        warm_ap = np.ones((1, 128, 1025), np.float32)
        warm_mel = mel_session.run(None, {
            "f0": warm_f0, "sp_env": warm_sp, "ap": warm_ap,
            "phase_offset": np.array(0.0, np.float32),
        })[0]
        voc_session.run(None, {
            "mel": warm_mel, "f0": warm_f0,
            "phase_offset": np.array(0.0, np.float32),
        })
        warmup_ms = (time.perf_counter() - warm_t) * 1000
        print(f"session_warmup_ms={warmup_ms:.2f}", flush=True)

    # The Play/TTFS clock begins after persistent model sessions are ready.
    t_play = time.perf_counter()
    proc = subprocess.Popen([
        str(args.rust_bin), str(args.project), str(args.voicebank), str(args.worldline),
        str(args.out / "features"), str(args.track), str(args.phrase_ms),
        str(args.prefix_requests), str(args.publish_every), str(args.prep_workers),
    ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)

    f0 = env = ap = None
    total_frames = stable_frames = 0
    expected_frames = 0
    mel_out = mel_state = mel_ready = None
    voc_state = pcm_parts = None
    mel_cursor = voc_cursor = 0
    block_times = []
    block_sizes = []
    first_pcm_ms = None
    fast_boot = args.first_pcm_frames > 0
    final_snapshot = None
    rust_lines = []

    def consume_ready(final: bool) -> None:
        nonlocal mel_out, mel_state, mel_ready, voc_state, pcm_parts
        nonlocal mel_cursor, voc_cursor, first_pcm_ms, total_frames
        nonlocal expected_frames, fast_boot
        if f0 is None:
            return
        first_allocation = mel_out is None
        if first_allocation or mel_out.shape[1] < total_frames:
            old_n = 0 if mel_out is None else mel_out.shape[1]
            next_mel = np.zeros((1, total_frames, 128), np.float32)
            next_mel_state = np.full(total_frames, np.nan, np.float32)
            next_voc_state = np.full(total_frames, np.nan, np.float32)
            if old_n:
                next_mel[:, :old_n] = mel_out
                next_mel_state[:old_n] = mel_state
                next_voc_state[:old_n] = voc_state
            mel_out = next_mel
            mel_state = next_mel_state
            voc_state = next_voc_state
            if first_allocation:
                pcm_parts = []
                mel_ready = 0
        # Publish only feature rows proven stable by the earliest unprepared
        # phone start. Leave model right-context outside this watermark.
        if fast_boot and not final:
            mel_context = args.mel_context
            voc_context = args.vocoder_context
            mel_core = args.mel_core
            voc_core = args.vocoder_core
        else:
            mel_context = max(args.mel_context, 32)
            voc_context = max(args.vocoder_context, 64)
            mel_core = args.steady_mel_core
            voc_core = args.steady_vocoder_core
        mel_limit = stable_frames if final else max(0, stable_frames - mel_context)
        while mel_cursor < mel_limit:
            c0 = mel_cursor
            c1 = min(total_frames, c0 + mel_core)
            if not final and c1 > mel_limit:
                break
            s = max(0, c0 - mel_context)
            s = (s // 16) * 16
            requested_end = min(total_frames, c1 + mel_context)
            if final and requested_end == total_frames:
                # Match whole-phrase inference's natural convolution boundary;
                # do not turn the phrase end into an artificial aligned pad.
                aligned_length = requested_end - s
            else:
                aligned_length = ((requested_end - s + 15) // 16) * 16
            aligned_end = s + aligned_length
            # Never feed rows beyond the stable watermark: fill the final
            # alignment tail with the model's neutral WORLD feature values.
            read_end = min(aligned_end, stable_frames, total_frames)
            f0_in = f0[:, s:read_end]
            env_in = env[:, s:read_end, :]
            ap_in = ap[:, s:read_end, :]
            pad_frames = aligned_length - (read_end - s)
            if pad_frames:
                f0_in = np.pad(f0_in, ((0, 0), (0, pad_frames)))
                env_in = np.pad(env_in, ((0, 0), (0, pad_frames), (0, 0)), constant_values=1e-12)
                ap_in = np.pad(ap_in, ((0, 0), (0, pad_frames), (0, 0)), constant_values=1.0)
            offset = np.float32(mel_state[ s - 1] if s else 0.0)
            block, state = mel_session.run(None, {
                "f0": f0_in, "sp_env": env_in, "ap": ap_in,
                "phase_offset": np.array(offset, np.float32),
            })
            mel_out[:, c0:c1, :] = block[:, c0 - s:c1 - s, :]
            state = state.reshape(-1)
            mel_state[s:min(total_frames, s + len(state))] = state[:max(0, min(total_frames - s, len(state)))]
            mel_cursor = c1
            mel_ready = c1

            # Consume contiguous vocoder blocks only after their mel right
            # context is ready; final flush may clip context at phrase end.
            while voc_cursor < total_frames:
                v0 = voc_cursor
                requested_core = args.first_pcm_frames if fast_boot and first_pcm_ms is None else voc_core
                v1 = min(total_frames, v0 + requested_core)
                required = v1 + voc_context
                if not final and v1 > max(0, expected_frames - 64):
                    break
                if not final and required > mel_ready:
                    break
                vs = max(0, v0 - voc_context)
                ve = min(total_frames, required)
                if not final and ve > mel_ready:
                    break
                voffset = np.float32(voc_state[vs - 1] if vs else 0.0)
                block_pcm, state = voc_session.run(None, {
                    "mel": mel_out[:, vs:ve, :], "f0": f0[:, vs:ve],
                    "phase_offset": np.array(voffset, np.float32),
                })
                block_pcm = block_pcm.reshape(-1)[(v0 - vs) * 512:(v1 - vs) * 512].copy()
                state = state.reshape(-1)
                end_state = min(total_frames, vs + len(state))
                voc_state[vs:end_state] = state[:end_state - vs]
                pcm_parts.append(block_pcm)
                now = (time.perf_counter() - t_play) * 1000
                block_times.append(now)
                block_sizes.append(len(block_pcm))
                if first_pcm_ms is None:
                    first_pcm_ms = now
                    print(f"FIRST_PCM play_to_pcm_ms={first_pcm_ms:.2f} samples={len(block_pcm)} "
                          f"audio_ms={len(block_pcm) / 44100 * 1000:.2f} phones_watermark={stable_frames}", flush=True)
                voc_cursor = v1
                if fast_boot and first_pcm_ms is not None:
                    break
            if fast_boot and first_pcm_ms is not None:
                # Keep the first low-lookahead PCM, but redo its model context
                # before it is used to synthesize later steady-state chunks.
                fast_boot = False
                mel_cursor = 0
                mel_ready = 0
                voc_cursor = block_sizes[0] // 512
                break

    for line in proc.stdout:
        line = line.rstrip("\n")
        rust_lines.append(line)
        if line.startswith("LIVE_STAGE "):
            _, idx, processed, frame_count, safe_count, expected_count, folder = line.split(" ", 6)
            stage_f0, stage_env, stage_ap = read_features(Path(folder))
            if f0 is not None and stage_f0.shape[1] < f0.shape[1]:
                raise RuntimeError("feature watermark regressed")
            f0, env, ap = stage_f0, stage_env, stage_ap
            total_frames = int(frame_count)
            stable_frames = min(int(safe_count), total_frames)
            expected_frames = int(expected_count)
            final_snapshot = (f0.copy(), env.copy(), ap.copy())
            consume_ready(final=False)
            print(f"FEATURE_STAGE index={idx} requests={processed} frames={frame_count} "
                  f"stable_frames={stable_frames} pcm_blocks={len(block_times)} "
                  f"producer_consumer_elapsed_ms={(time.perf_counter()-t_play)*1000:.2f}", flush=True)
        elif line.startswith("LIVE_DONE "):
            if final_snapshot is None:
                raise RuntimeError("Rust producer ended without a feature stage")
            if expected_frames != total_frames:
                raise RuntimeError(f"planned final frame count {expected_frames} != produced {total_frames}")
            # Do not stream the phrase tail until the final feature boundary is
            # known. Recompute enough left context with the full model windows
            # before flushing it, then synthesize that held tail with full
            # contexts instead of the shorter live window.
            consume_ready(final=True)
            print(line, flush=True)

    stderr = proc.stderr.read() if proc.stderr else ""
    code = proc.wait()
    if stderr:
        print(stderr, file=sys.stderr, end="")
    if code:
        raise RuntimeError(f"Rust feature producer exited {code}")
    if f0 is None or first_pcm_ms is None:
        raise RuntimeError("no PCM arrived from the live feature stream")

    streamed = np.concatenate(pcm_parts)
    expected_samples = total_frames * 512
    if len(streamed) != expected_samples:
        raise RuntimeError(f"PCM incomplete: got {len(streamed)} samples, expected {expected_samples}")

    # Whole-phrase ONNX baseline runs only after the live stream is complete.
    ref_t = time.perf_counter()
    mel_ref = ort.InferenceSession(str(mel_source), options, providers=providers)
    voc_ref = ort.InferenceSession(str(voc_source), options, providers=providers)
    full_mel = mel_ref.run(None, {"f0": f0, "sp_env": env, "ap": ap})[0]
    full_pcm = voc_ref.run(None, {"mel": full_mel, "f0": f0})[0].reshape(-1)
    baseline_ms = (time.perf_counter() - ref_t) * 1000
    compare_n = min(len(streamed), len(full_pcm))
    np.save(args.out / "streamed_pcm.npy", streamed)
    np.save(args.out / "whole_reference_pcm.npy", full_pcm)
    write_wav(args.out / "streamed_pcm.wav", streamed)
    write_wav(args.out / "whole_reference_pcm.wav", full_pcm)
    write_wav(args.out / "first_pcm_block.wav", streamed[:block_sizes[0]])

    # Deadline simulation: first PCM starts the consumer clock; every later
    # block must arrive before the preceding PCM duration is exhausted.
    late = 0
    max_late = 0.0
    consumed_s = block_sizes[0] / 44100
    for ready_ms, samples in zip(block_times[1:], block_sizes[1:]):
        deadline_ms = first_pcm_ms + consumed_s * 1000
        lateness = ready_ms - deadline_ms
        if lateness > 0:
            late += 1
            max_late = max(max_late, lateness)
        consumed_s += samples / 44100
    print(f"STREAM_RESULT status=complete blocks={len(block_times)} samples={len(streamed)} "
          f"audio_s={len(streamed)/44100:.3f} pcm_vs_whole_{metrics(full_pcm[:compare_n], streamed[:compare_n])} "
          f"late_blocks={late} max_late_ms={max_late:.2f} "
          f"full_reference_after_stream_ms={baseline_ms:.2f} output={args.out}", flush=True)
    warm_note = "first inference warmed before Play" if not args.skip_warmup else "first inference included after Play"
    print(f"NOTE: {warm_note}; PCM is handed to an in-memory consumer and real-time deadlines are simulated, not played on a physical audio device.", flush=True)


if __name__ == "__main__":
    main()
