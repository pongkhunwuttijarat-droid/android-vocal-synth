#!/usr/bin/env python3
"""Compare two renders of the same .ustx part note-by-note.

The absolute deviation from the notated `tone` is not a fair parity test: a
singing voice is not a sine oscillator, and OpenUtau's own render sits tens of
cents off `tone` at note onsets (attack + pitch-bend into the note). What we
can hold the engine to is *agreement with OpenUtau*: measure f0 at the same
instant in both files and compare the two measurements.

    python3 scripts/compare_render_pitch.py \
        --reference Export/x_Track1_01.wav --ours /tmp/ours-t1.wav \
        --project song.ustx --track 1

Exit code 0 when the two renders agree within --tol cents on the notes where
both produced audio, 1 otherwise. Notes only one side could measure (a rest,
an unpitched attack) are reported as SKIP and do not fail the run.
"""

import argparse
import array
import math
import sys
import wave

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from check_render_pitch import estimate_f0, load_mono, parse_notes, project_bpm, tick_to_sec  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", required=True)
    ap.add_argument("--ours", required=True)
    ap.add_argument("--project", required=True)
    ap.add_argument("--track", type=int, required=True)
    ap.add_argument("--bpm", type=float, default=None)
    ap.add_argument("--offset-ms", type=float, default=50.0)
    ap.add_argument("--min-window-ms", type=float, default=25.0)
    ap.add_argument("--silence-floor", type=int, default=64)
    ap.add_argument("--tol", type=float, default=60.0, help="cents of DISAGREEMENT")
    ap.add_argument("--stride", type=int, default=1)
    args = ap.parse_args()

    bpm = args.bpm if args.bpm is not None else project_bpm(args.project)
    ref, ref_sr = load_mono(args.reference)
    ours, our_sr = load_mono(args.ours)
    notes = parse_notes(args.project, args.track)

    rows, agree, bad, skipped, ref_off, our_off = [], 0, 0, 0, [], []
    for i, n in enumerate(notes[:: max(1, args.stride)]):
        expect = 440 * 2 ** ((n["tone"] - 69) / 12.0)
        onset = tick_to_sec(n["position"], bpm)
        dur_ms = n["duration"] / 480.0 * 60.0 / bpm * 1000.0
        off_ms = min(args.offset_ms, 0.30 * dur_ms)
        win_ms = max(0.50 * dur_ms, args.min_window_ms)

        def measure(buf, sr):
            start = int(onset * sr) + int(sr * off_ms / 1000)
            seg = buf[start:start + int(sr * win_ms / 1000)]
            if len(seg) < 1024 or max(abs(v) for v in seg) < args.silence_floor:
                return None
            return estimate_f0(seg, sr)

        fr, fo = measure(ref, ref_sr), measure(ours, our_sr)
        if fr is None or fo is None:
            skipped += 1
            rows.append((n, expect, fr, fo, None))
            continue
        # Signed cents of each render from the notated tone.
        cr = 1200 * math.log2(fr / expect)
        co = 1200 * math.log2(fo / expect)
        # Signed cents of disagreement between the two renders.
        diff = co - cr
        ref_off.append(abs(cr))
        our_off.append(abs(co))
        rows.append((n, expect, fr, fo, diff))
        if abs(diff) <= args.tol:
            agree += 1
        else:
            bad += 1

    print(f"reference={args.reference} ({len(ref)/ref_sr:.2f}s @{ref_sr})")
    print(f"ours={args.ours} ({len(ours)/our_sr:.2f}s @{our_sr})")
    print(f"track={args.track} notes={len(notes)} bpm={bpm}")
    print(f"{'lyric':10s} {'tone':>4s} {'expectHz':>9s} {'refHz':>8s} {'ourHz':>8s} "
          f"{'refOff':>8s} {'ourOff':>8s} {'diff':>8s}")
    worst = []
    for n, expect, fr, fo, diff in rows:
        lyr = n.get("lyric", "")
        if fr is None or fo is None:
            print(f"{lyr:10s} {n['tone']:4d} {expect:9.1f} "
                  f"{'SKIP' if fr is None else f'{fr:8.1f}'} "
                  f"{'SKIP' if fo is None else f'{fo:8.1f}'} {'-':>8s} {'-':>8s} {'-':>8s}")
            continue
        cr = 1200 * math.log2(fr / expect)
        co = 1200 * math.log2(fo / expect)
        worst.append((abs(diff), lyr, n["tone"], cr, co, diff))
        print(f"{lyr:10s} {n['tone']:4d} {expect:9.1f} {fr:8.1f} {fo:8.1f} "
              f"{cr:+8.1f} {co:+8.1f} {diff:+8.1f}")

    measured = sorted(abs(d) for *_, d in worst)
    if measured:
        print(f"\n|disagreement| median={measured[len(measured)//2]:.1f} "
              f"p90={measured[int(len(measured)*0.9)-1]:.1f} max={measured[-1]:.1f} cents")
    for label, xs in (("reference offset from tone", ref_off), ("our offset from tone", our_off)):
        xs = sorted(xs)
        if xs:
            print(f"|{label}| median={xs[len(xs)//2]:.1f} p90={xs[int(len(xs)*0.9)-1]:.1f} "
                  f"max={xs[-1]:.1f} cents")

    worst.sort(reverse=True)
    if worst:
        print("\nlargest disagreements:")
        for ad, lyr, tone, cr, co, d in worst[:10]:
            print(f"  {lyr:10s} tone={tone} refOff={cr:+.1f} ourOff={co:+.1f} diff={d:+.1f}")

    print(f"\ncompared={len(worst)} agree(within {args.tol:.0f}c)={agree} "
          f"disagree={bad} skipped(one side silent)={skipped}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())