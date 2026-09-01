#!/bin/bash
# Full eval pipeline: extract (long chains) -> train MoE -> render ALL ->
# compare vs teacher (avg / worst-case). ALL OUTPUTS IN THE REPO (stable).
set -e
cd /home/seal/project/android-voice-synth/native
P=/home/seal/project/android-voice-synth/test/poc/eval
mkdir -p $P/teacher-wavs $P/student-all

echo "[1/4] extract (long-chain set + teacher wavs)..."
./target/debug/train-prep \
  ../test/golden/teto-english/library \
  build/build-linux/libworldline.so \
  ../app/android/app/src/main/assets/engine/models/mel.onnx \
  ../app/android/app/src/main/assets/engine/models/pc_nsf_hifigan.onnx \
  $P/train-set.jsonl --max-singles 40 --chain 16 --chain-max 12 \
  --tones "60,64" --durs "400,700" --dump-wav $P/teacher-wavs 2>&1 | tail -1

echo "[2/4] train MoE (40 epochs)..."
./target/debug/moe-train --moe $P/train-set.jsonl $P/moe-model \
  --epochs 40 --expert-epochs 10 --batch 512 --lr 1e-3 --backend wgpu 2>&1 | grep -E "\[moe\]|OK" | tail -8

echo "[3/4] render ALL phrases..."
./target/debug/moe-train --render-all $P/train-set.jsonl $P/moe-model \
  ../app/android/app/src/main/assets/engine/models/pc_nsf_hifigan.onnx \
  $P/student-all --gain 1.2 --backend wgpu 2>&1 | tail -1

echo "[4/4] compare vs teacher (avg + worst)..."
python3 - $P <<'EOF'
import sys, wave, struct, math, json, os
import numpy as np
P = sys.argv[1]
def load(p):
    with wave.open(p) as w:
        raw = w.readframes(w.getnframes())
    return np.array(struct.unpack('<%dh' % (len(raw)//2), raw), dtype=np.float64)/32768.0
names = {}
for l in open(f'{P}/train-set.jsonl'):
    e = json.loads(l)
    names[len(names)] = ' '.join(e['aliases'])
n = len(names)
rows = []
for i in range(n):
    tp = f'{P}/teacher-wavs/{i:05d}.wav'
    sp = f'{P}/student-all/{i:05d}.wav'
    if not (os.path.exists(tp) and os.path.exists(sp)):
        continue
    t = load(tp); s = load(sp)
    m = min(len(t), len(s))
    tn, sn = t[:m], s[:m]
    best = max(range(-100, 101), key=lambda d: np.corrcoef(np.roll(tn, d)[60:m-60], sn[60:m-60])[0,1])
    c = np.corrcoef(np.roll(tn, best)[60:m-60], sn[60:m-60])[0,1]
    rt = math.sqrt(np.mean(tn**2)); rs = math.sqrt(np.mean(sn**2))
    rows.append((i, c, rs/rt))
corrs = sorted(rows, key=lambda r: r[1])
allc = [r[1] for r in rows]; allr = [r[2] for r in rows]
print(f"phrases: {len(rows)}")
print(f"corr  mean={np.mean(allc):.4f} median={np.median(allc):.4f} min={min(allc):.4f} max={max(allc):.4f}")
print(f"rms ratio mean={np.mean(allr):.3f} median={np.median(allr):.3f} (1.0 = same volume)")
print("WORST 5:")
for i, c, r in corrs[:5]:
    print(f"  #{i} corr={c:.3f} rms={r:.2f}  {names[i]}")
print("BEST 3:")
for i, c, r in corrs[-3:][::-1]:
    print(f"  #{i} corr={c:.3f} rms={r:.2f}  {names[i]}")
EOF
echo "DONE -> $P"
