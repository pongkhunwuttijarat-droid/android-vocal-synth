#!/usr/bin/env python3
"""Compare the f0 *trajectory* of two renders over a time window.

Note-onset sampling is a weak parity test on this project: many notes are
short and carry pitch glides, so a single window at a fixed offset into a note
can land on a glide and read hundreds of cents off. Comparing the continuous
f0 curve over the same time range answers the real question — "at this
moment, did the two renders sing the same pitch?" — without depending on where
inside a note the window happened to fall.

    python3 scripts/compare_f0_curve.py \
        --reference ref.wav --ours ours.wav --from 219.0 --to 223.5 --step-ms 20

Exit code 0 when the median |difference| stays within --tol cents, 1 otherwise.
Frames where either render has no measurable pitch are reported as SKIP.
"""

import argparse
import math
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from check_render_pitch import estimate_f0, load_mono  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", required=True)
    ap.add_argument("--ours", required=True)
    ap.add_argument("--from", dest="start", type=float, required=True,
                    help="window start, seconds")
    ap.add_argument("--to", dest="end", type=float, required=True)
    ap.add_argument("--step-ms", type=float, default=20.0)
    ap.add_argument("--window-ms", type=float, default=40.0)
    ap.add_argument("--silence-floor", type=int, default=200)
    ap.add_argument("--tol", type=float, default=60.0, help="cents, on the MEDIAN")
    args = ap.parse_args()

    ref, ref_sr = load_mono(args.reference)
    ours, our_sr = load_mono(args.ours)

    step = args.step_ms / 1000.0
    win = args.window_ms / 1000.0
    rows = []
    t = args.start
    while t + win <= args.end:
        def at(buf, sr):
            seg = buf[int(t * sr):int((t + win) * sr)]
            if len(seg) < 1024 or max(abs(v) for v in seg) < args.silence_floor:
                return None
            return estimate_f0(seg, sr)
        fr, fo = at(ref, ref_sr), at(ours, our_sr)
        if fr is not None and fo is not None:
            rows.append((t, fr, fo, 1200 * math.log2(fo / fr)))
        t += step

    print(f"reference={args.reference}")
    print(f"ours={args.ours}")
    print(f"window={args.start:.2f}..{args.end:.2f}s step={args.step_ms}ms "
          f"win={args.window_ms}ms compared={len(rows)}")
    print(f"{'t(s)':>8s} {'refHz':>9s} {'ourHz':>9s} {'diffCents':>10s}")
    for t, fr, fo, d in rows:
        print(f"{t:8.3f} {fr:9.1f} {fo:9.1f} {d:+10.1f}")

    if not rows:
        print("no comparable frames — cannot judge", file=sys.stderr)
        return 1
    diffs = sorted(abs(d) for *_, d in rows)
    med = diffs[len(diffs) // 2]
    p90 = diffs[int(len(diffs) * 0.9) - 1]
    print(f"\n|diff| median={med:.1f} p90={p90:.1f} max={diffs[-1]:.1f} cents")
    return 0 if med <= args.tol else 1


if __name__ == "__main__":
    sys.exit(main())