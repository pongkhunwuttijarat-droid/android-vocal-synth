#!/usr/bin/env python3
"""Build a tiny FAKE DiffSinger model to exercise the Rust E2E chain
(load -> tensors -> acoustic onnx -> mel -> vocoder onnx -> samples)
without a real voicebank. Crude models (Unsqueeze+Tile), NOT trained.
Dynamic Expand is avoided — onnxruntime rejects Expand with a
dynamically-computed shape.
"""
import os
import onnx
from onnx import helper, TensorProto

OUT = "/tmp/ds-fake"
os.makedirs(f"{OUT}/vocoder", exist_ok=True)


def build_acoustic():
    tokens = helper.make_tensor_value_info("tokens", TensorProto.INT64, ["batch", "N"])
    durations = helper.make_tensor_value_info("durations", TensorProto.INT64, ["batch", "N"])
    f0 = helper.make_tensor_value_info("f0", TensorProto.FLOAT, ["batch", "T"])
    mel = helper.make_tensor_value_info("mel", TensorProto.FLOAT, ["batch", "T", 4])

    # mel = Tile(Unsqueeze(f0, [2]), [1,1,4]) — static repeats only.
    unsq = helper.make_node("Unsqueeze", ["f0"], ["f0u"], axes=[2])
    reps = helper.make_node("Constant", [], ["reps"], value=helper.make_tensor(
        "reps", TensorProto.INT64, [3], [1, 1, 4]))
    tile = helper.make_node("Tile", ["f0u", "reps"], ["mel"])

    graph = helper.make_graph(
        [unsq, reps, tile],
        "fake_acoustic",
        [tokens, durations, f0],
        [mel],
    )
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 11)])
    onnx.checker.check_model(model)
    onnx.save(model, f"{OUT}/acoustic.onnx")


def build_vocoder():
    hop = 512
    mel = helper.make_tensor_value_info("mel", TensorProto.FLOAT, ["batch", "T", 4])
    f0 = helper.make_tensor_value_info("f0", TensorProto.FLOAT, ["batch", "T"])
    wave = helper.make_tensor_value_info("waveform", TensorProto.FLOAT, ["batch", "L"])

    # mean = ReduceMean(mel, axis=2) -> [1,T]
    mean = helper.make_node("ReduceMean", ["mel"], ["mean"], axes=[2], keepdims=0)
    # waveform = Tile(mean, [1, hop]) -> [1, T*hop] (static repeats)
    reps = helper.make_node("Constant", [], ["reps"], value=helper.make_tensor(
        "reps", TensorProto.INT64, [2], [1, hop]))
    tile = helper.make_node("Tile", ["mean", "reps"], ["waveform"])

    graph = helper.make_graph(
        [mean, reps, tile],
        "fake_vocoder",
        [mel, f0],
        [wave],
    )
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 11)])
    onnx.checker.check_model(model)
    onnx.save(model, f"{OUT}/vocoder/model.onnx")


build_acoustic()
build_vocoder()

with open(f"{OUT}/phonemes.txt", "w") as f:
    f.write("SP\nl\nA\n3\nR\n")

with open(f"{OUT}/dsconfig.yaml", "w") as f:
    f.write("""phonemes: phonemes.txt
acoustic: acoustic.onnx
vocoder: vocoder
hidden_size: 4
num_mel_bins: 4
mel_base: "10"
mel_scale: slaney
""")

with open(f"{OUT}/vocoder/vocoder.yaml", "w") as f:
    f.write("""name: fake-vocoder
model: model.onnx
num_mel_bins: 4
pitch_controllable: false
""")

for root, _, files in os.walk(OUT):
    for fn in files:
        p = os.path.join(root, fn)
        print(f"  {p} ({os.path.getsize(p)} B)")
print("OK: fake DiffSinger model at", OUT)
