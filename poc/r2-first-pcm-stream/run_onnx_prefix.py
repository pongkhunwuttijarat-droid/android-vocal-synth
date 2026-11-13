#!/usr/bin/env python3
"""Out-of-tree R2 ONNX chunk/phase-carry experiment.

Reads f0/sp/ap float32 arrays previously dumped by the temporary Rust R2
feature extractor. It does not modify the repository models. Required:
onnx, onnxruntime-gpu, numpy. See T050 for the measured fixture and limits.
"""
import argparse
import time
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
from onnx import TensorProto, helper


def instrument(source: Path, node_name: str, state_name: str, output: Path) -> None:
    model = onnx.load(source)
    cumsum = next(n for n in model.graph.node if n.op_type == "CumSum" and n.name == node_name)
    original_output = cumsum.output[0]
    raw_output = original_output + "_raw"
    cumsum.output[0] = raw_output
    index = list(model.graph.node).index(cumsum) + 1
    model.graph.node.insert(index, helper.make_node(
        "Add", [raw_output, "phase_offset"], [original_output], name="PhaseCarryAdd"
    ))
    model.graph.node.append(helper.make_node(
        "Identity", [original_output], [state_name], name="ExposePhaseState"
    ))
    model.graph.input.append(helper.make_tensor_value_info(
        "phase_offset", TensorProto.FLOAT, []
    ))
    model.graph.output.append(helper.make_tensor_value_info(
        state_name, TensorProto.FLOAT, [1, "phase_len", 1]
    ))
    onnx.checker.check_model(model)
    onnx.save(model, output)


def metrics(reference: np.ndarray, candidate: np.ndarray) -> str:
    a = reference.reshape(-1).astype(np.float64)
    b = candidate.reshape(-1).astype(np.float64)
    delta = a - b
    corr = np.dot(a, b) / max(np.linalg.norm(a) * np.linalg.norm(b), 1e-30)
    return (f"max_abs={np.max(np.abs(delta)):.9g} corr={corr:.9f} "
            f"rmse={np.sqrt(np.mean(delta * delta)):.9g}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--models", type=Path, default=Path("models/r2"))
    parser.add_argument("--features", type=Path,
                        default=Path("/tmp/rusted-utau-incremental-pipeline-poc/actual-features"))
    parser.add_argument("--work", type=Path,
                        default=Path("/tmp/rusted-utau-incremental-pipeline-poc/chunk-models"))
    parser.add_argument("--prefix-features", type=Path, required=True)
    parser.add_argument("--first-wav", type=Path, default=Path("/tmp/rusted-utau-incremental-pipeline-poc/r2-first-pcm.wav"))
    args = parser.parse_args()
    args.work.mkdir(parents=True, exist_ok=True)
    full_n, sp, _ = map(int, (args.features / "shape.txt").read_text().split())
    full_f0 = np.fromfile(args.features / "f0.f32le", "<f4").reshape(1, full_n)
    full_sp = np.fromfile(args.features / "sp.f32le", "<f4").reshape(1, full_n, sp)
    full_ap = np.fromfile(args.features / "ap.f32le", "<f4").reshape(1, full_n, sp)
    n, psp, _ = map(int, (args.prefix_features / "shape.txt").read_text().split())
    assert psp == sp
    f0 = np.fromfile(args.prefix_features / "f0.f32le", "<f4").reshape(1, n)
    sp_env = np.fromfile(args.prefix_features / "sp.f32le", "<f4").reshape(1, n, sp)
    ap = np.fromfile(args.prefix_features / "ap.f32le", "<f4").reshape(1, n, sp)
    mel_source = args.models / "mel.onnx"
    voc_source = args.models / "pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx"
    mel_graph = args.work / "mel-phase-carry.onnx"
    voc_graph = args.work / "vocoder-phase-carry.onnx"
    instrument(mel_source, "/harmonic_synthsizer/CumSum", "phase_state", mel_graph)
    instrument(voc_source, "/generator/CumSum", "phase_state", voc_graph)

    options = ort.SessionOptions()
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1
    providers = ["CUDAExecutionProvider", "CPUExecutionProvider"]
    session_start = time.perf_counter()
    mel_session = ort.InferenceSession(str(mel_graph), options, providers=providers)
    voc_session = ort.InferenceSession(str(voc_graph), options, providers=providers)
    session_setup_ms = (time.perf_counter() - session_start) * 1000

    zero = np.array(0, np.float32)
    print(f"features full_frames={full_n} prefix_frames={n} prefix_audio_s={n*512/44100:.3f}")
    print(f"chunk_session_setup_ms={session_setup_ms:.2f}")
    first_results = []

    for mel_core, mel_context, voc_core, voc_context in [
        (64, 32, 64, 64),
        (128, 32, 128, 64),
    ]:
        mel_states = np.full(n, np.nan, np.float32)
        mel_out = np.zeros((1, n, 128), np.float32)
        voc_states = np.full(n, np.nan, np.float32)
        pcm_out = np.zeros((1, n * 512), np.float32)
        mel_ready = 0
        next_voc = 0
        first_pcm_ms = None
        first_block = None
        pcm_ready = []
        start = time.perf_counter()
        mel_ms = voc_ms = 0.0

        for c0 in range(0, n, mel_core):
            c1 = min(n, c0 + mel_core)
            s = max(0, c0 - mel_context)
            e = min(n, c1 + mel_context)
            s = (s // 16) * 16
            e = min(n, ((e + 15) // 16) * 16)
            offset = np.float32(mel_states[s - 1] if s else 0.0)
            t = time.perf_counter()
            block, state = mel_session.run(None, {
                "f0": f0[:, s:e], "sp_env": sp_env[:, s:e, :],
                "ap": ap[:, s:e, :], "phase_offset": np.array(offset, np.float32),
            })
            mel_ms += (time.perf_counter() - t) * 1000
            mel_out[:, c0:c1, :] = block[:, c0 - s:c1 - s, :]
            mel_ready = c1
            state = state.reshape(-1)
            mel_states[s:s + len(state)] = state

            while next_voc < n and min(n, next_voc + voc_core + voc_context) <= mel_ready:
                v0 = next_voc
                v1 = min(n, v0 + voc_core)
                vs = max(0, v0 - voc_context)
                ve = min(n, v1 + voc_context)
                voffset = np.float32(voc_states[vs - 1] if vs else 0.0)
                t = time.perf_counter()
                block, state = voc_session.run(None, {
                    "mel": mel_out[:, vs:ve, :], "f0": f0[:, vs:ve],
                    "phase_offset": np.array(voffset, np.float32),
                })
                voc_ms += (time.perf_counter() - t) * 1000
                pcm_out[:, v0 * 512:v1 * 512] = block[:, (v0 - vs) * 512:(v1 - vs) * 512]
                state = state.reshape(-1)
                voc_states[vs:vs + len(state)] = state
                next_voc = v1
                pcm_ready.append(((time.perf_counter() - start) * 1000, (v1 - v0) * 512 / 44100))
                if first_pcm_ms is None:
                    first_pcm_ms = (time.perf_counter() - start) * 1000
                    first_block = pcm_out[:, v0 * 512:v1 * 512].copy()

        while next_voc < n:
            v0 = next_voc
            v1 = min(n, v0 + voc_core)
            vs = max(0, v0 - voc_context)
            ve = min(n, v1 + voc_context)
            voffset = np.float32(voc_states[vs - 1] if vs else 0.0)
            t = time.perf_counter()
            block, state = voc_session.run(None, {
                "mel": mel_out[:, vs:ve, :], "f0": f0[:, vs:ve],
                "phase_offset": np.array(voffset, np.float32),
            })
            voc_ms += (time.perf_counter() - t) * 1000
            pcm_out[:, v0 * 512:v1 * 512] = block[:, (v0 - vs) * 512:(v1 - vs) * 512]
            state = state.reshape(-1)
            voc_states[vs:vs + len(state)] = state
            next_voc = v1
            pcm_ready.append(((time.perf_counter() - start) * 1000, (v1 - v0) * 512 / 44100))

        import wave
        first = first_block.reshape(-1)
        pcm16 = np.clip(first, -1, 1)
        pcm16 = (pcm16 * 32767).astype("<i2")
        chunk_wav = args.first_wav.with_name(f"{args.first_wav.stem}-{voc_core}f{args.first_wav.suffix}")
        with wave.open(str(chunk_wav), "wb") as wf:
            wf.setnchannels(1); wf.setsampwidth(2); wf.setframerate(44100); wf.writeframes(pcm16.tobytes())
        deadline = pcm_ready[0][0]
        emitted_before = 0.0
        late_blocks = 0
        max_late_ms = 0.0
        emitted_before = pcm_ready[0][1]
        for ready_ms, block_s in pcm_ready[1:]:
            lateness = ready_ms - (pcm_ready[0][0] + emitted_before * 1000.0)
            if lateness > 0.0:
                late_blocks += 1
                max_late_ms = max(max_late_ms, lateness)
            emitted_before += block_s
        print("stream", f"mel_block={mel_core} mel_context={mel_context}",
              f"vocoder_block={voc_core} vocoder_context={voc_context}",
              f"first_model_pcm_ms={first_pcm_ms:.2f}",
              f"model_total_ms={(time.perf_counter() - start) * 1000:.2f}",
              f"mel_ms={mel_ms:.2f}", f"vocoder_ms={voc_ms:.2f}",
              f"block_audio_s={voc_core * 512 / 44100:.3f}",
              f"pcm_blocks={len(pcm_ready)} simulated_late_blocks={late_blocks} max_late_ms={max_late_ms:.2f}",
              f"wav={chunk_wav}")
        first_results.append((voc_core, first.copy(), chunk_wav))

    ref_start = time.perf_counter()
    original_mel = ort.InferenceSession(str(mel_source), options, providers=providers)
    original_voc = ort.InferenceSession(str(voc_source), options, providers=providers)
    full_mel = original_mel.run(None, {"f0": full_f0, "sp_env": full_sp, "ap": full_ap})[0]
    full_pcm = original_voc.run(None, {"mel": full_mel, "f0": full_f0})[0]
    reference_ms = (time.perf_counter() - ref_start) * 1000
    for voc_core, first, wav_path in first_results:
        ref = full_pcm.reshape(-1)[:len(first)]
        print("first_block_parity", f"vocoder_block={voc_core}", metrics(ref, first), f"wav={wav_path}")
    print(f"NOTE: chunk sessions were created before timing the prefix-to-first-PCM path; full-reference session/inference was run afterward. Rust prefix preparation is measured separately by the companion runner. full_reference_ms={reference_ms:.2f}")


if __name__ == "__main__":
    main()
