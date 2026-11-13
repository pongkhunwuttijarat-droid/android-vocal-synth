#!/usr/bin/env python3
"""Estimate sung pitch at .ustx note onsets in a rendered WAV and report the
deviation from the note's tone, in cents.

Used to sanity-check that a full-project render is *musically* right (right
notes, right pitch) rather than merely non-silent: the repo has no sample-level
OpenUtau reference render for these fixtures, so this is the honest end-to-end
check available.

    python3 scripts/check_render_pitch.py \
        --wav out.wav --project test-ustx-example/golden-the-end-of-hutsune-miku.ustx

Exit code 0 when every measurable onset is within --tol cents, 1 otherwise.
Onsets with no audio (rests, or a note shorter than the analysis window) are
reported as SKIP and do not fail the run.
"""

import argparse
import array
import math
import re
import sys
import wave


def tick_to_sec(tick, bpm):
    # .ustx time base: PPQ = 480 ticks per beat, constant regardless of bpm.
    return tick / 480.0 * 60.0 / bpm


def load_mono(path):
    w = wave.open(path)
    if w.getsampwidth() != 2:
        raise SystemExit(f"{path}: only 16-bit PCM is handled")
    a = array.array("h")
    a.frombytes(w.readframes(w.getnframes()))
    if w.getnchannels() == 2:
        a = a[0::2]
    return a, w.getframerate()


def estimate_f0(seg, sr, fmin=60.0, fmax=1000.0):
    """Hann-windowed autocorrelation with an octave-error guard.

    Unnormalized autocorrelation on a Hann window behaves well and does not
    degenerate at long lags the way normalized cross-correlation does (its
    denominator is dominated by the window's near-zero tails). Plain
    autocorrelation happily locks onto 2f0/3f0 on vowels, so we take the
    *first local maximum* that reaches 90% of the global best — the usual
    "first strong peak" rule — and refine it by parabolic interpolation.
    """
    n = len(seg)
    if n < 1024:
        return None
    size = min(2048, n)
    win = [0.5 - 0.5 * math.cos(2 * math.pi * i / (size - 1)) for i in range(size)]
    x = [seg[i] * win[i] for i in range(size)]
    if sum(v * v for v in x) <= 0:
        return None
    lag_min = max(2, int(sr / fmax))
    lag_max = min(int(sr / fmin), size - 1)
    if lag_max <= lag_min:
        return None
    acf = [0.0] * (lag_max + 1)
    for lag in range(lag_min, lag_max + 1):
        acf[lag] = sum(x[i] * x[i + lag] for i in range(size - lag))
    best = max(acf[lag_min:lag_max + 1])
    if best <= 0:
        return None
    threshold = best * 0.9
    lag = None
    for l in range(lag_min + 1, lag_max):
        if acf[l] > acf[l - 1] and acf[l] >= acf[l + 1] and acf[l] >= threshold:
            lag = l
            break
    if lag is None:
        lag = max(range(lag_min, lag_max + 1), key=lambda l: acf[l])
    y0, y1, y2 = acf[lag - 1], acf[lag], acf[lag + 1] if lag + 1 <= lag_max else 0.0
    denom = 2 * (2 * y1 - y0 - y2)
    shift = (y2 - y0) / denom if denom else 0.0
    return sr / (lag + shift)


def parse_notes(path, track=None):
    """Pull (position, duration, tone, lyric) out of a .ustx.

    Uses PyYAML when available and falls back to an indentation-aware scan of the
    note blocks otherwise.

    `track` keeps only the voice parts that belong to that track number: a
    multitrack project holds hundreds of notes per track, and measuring track
    2's notes against a render of track 0 reports pitches that were never sung.
    """
    text = open(path, encoding="utf-8-sig").read()
    try:
        import yaml
    except ImportError:
        yaml = None

    if yaml is not None:
        doc = yaml.safe_load(text)
        out = []
        for part in doc.get("voice_parts") or []:
            if track is not None and int(part.get("track_no", 0)) != track:
                continue
            for n in part.get("notes") or []:
                out.append({
                    "position": int(n["position"]),
                    "duration": int(n["duration"]),
                    "tone": int(n["tone"]),
                    "lyric": str(n.get("lyric", "")),
                })
        return out

    if track is not None:
        raise SystemExit("--track needs PyYAML for the fallback parser")
    notes, cur = [], None
    for line in text.splitlines():
        # Note keys sit at exactly 4 spaces of indent — one level below the
        # part's own keys (2 spaces) and one below `notes:`.
        if re.match(r"^    position: -?\d+$", line):
            cur = {"position": int(line.split(":")[1])}
            continue
        if cur is None:
            continue
        m = re.match(r"^    (duration|tone|lyric): (.*)$", line)
        if not m:
            cur = None
            continue
        key, raw = m.group(1), m.group(2).strip()
        if key == "lyric":
            cur["lyric"] = raw.strip("'\"")
            if "tone" in cur and "duration" in cur:
                notes.append(cur)
                cur = None
        else:
            cur[key] = int(raw)
    return notes


def project_bpm(path):
    """The tempo that actually plays: the first `tempos` entry, else `bpm:`."""
    text = open(path, encoding="utf-8-sig").read()
    tempo = re.search(r"^tempos:\s*\n\s*- position: -?\d+\s*\n\s*bpm: *([\d.]+)",
                      text, re.M)
    if tempo:
        return float(tempo.group(1))
    default = re.search(r"^bpm: *([\d.]+)", text, re.M)
    if not default:
        raise SystemExit(f"{path}: no bpm found")
    return float(default.group(1))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--wav", required=True)
    ap.add_argument("--project", required=True)
    ap.add_argument("--track", type=int, default=None,
                    help="only consider voice parts on this track number")
    ap.add_argument("--bpm", type=float, default=None,
                    help="default: read the tempos list from the .ustx")
    ap.add_argument("--offset-ms", type=float, default=50.0,
                    help="skip past the note onset (attack / crossfade); capped at "
                         "30%% of the note duration")
    ap.add_argument("--min-window-ms", type=float, default=25.0,
                    help="floor for the analysis window (default: half the note)")
    ap.add_argument("--silence-floor", type=int, default=64,
                    help="peak |sample| below which a window counts as silent")
    ap.add_argument("--tol", type=float, default=60.0, help="cents")
    ap.add_argument("--limit", type=int, default=20)
    ap.add_argument("--stride", type=int, default=1, help="check every Nth note")
    ap.add_argument("--min-notes", type=int, default=100,
                    help="fail if the .ustx parser found fewer notes than this")
    args = ap.parse_args()

    bpm = args.bpm if args.bpm is not None else project_bpm(args.project)
    samples, sr = load_mono(args.wav)
    notes = parse_notes(args.project, args.track)
    if len(notes) < args.min_notes:
        print(f"only {len(notes)} notes parsed from {args.project} — refusing to "
              f"report a clean run (expected >= {args.min_notes})",
              file=sys.stderr)
        return 1

    checked = notes[:: max(1, args.stride)][: args.limit]
    rows, bad, skipped = [], 0, 0
    for n in checked:
        expect = 440 * 2 ** ((n["tone"] - 69) / 12.0)
        onset = tick_to_sec(n["position"], bpm)
        dur_ms = n["duration"] / 480.0 * 60.0 / bpm * 1000.0
        # Scale the analysis window to the note: a fixed 50 ms offset / 60 ms
        # window lands past the end of this project's 83 ms 十六分 run notes and
        # then measures the *next* note (or a rest), which reads as a wrong
        # pitch when the render is fine.
        off_ms = min(args.offset_ms, 0.30 * dur_ms)
        win_ms = max(0.50 * dur_ms, args.min_window_ms)
        start = int(onset * sr) + int(sr * off_ms / 1000)
        seg = samples[start:start + int(sr * win_ms / 1000)]
        if len(seg) < 1024 or max(abs(v) for v in seg) < args.silence_floor:
            rows.append((n, expect, None, None, dur_ms))
            skipped += 1
            continue
        f = estimate_f0(seg, sr)
        if f is None:
            rows.append((n, expect, None, None, dur_ms))
            skipped += 1
            continue
        cents = 1200 * math.log2(f / expect)
        rows.append((n, expect, f, cents, dur_ms))
        if abs(cents) > args.tol:
            bad += 1

    print(f"wav={args.wav}")
    print(f"project={args.project} bpm={bpm} notes={len(notes)} "
          f"track={args.track if args.track is not None else 'all'} sr={sr}")
    print(f"{'lyric':12s} {'tone':>4s} {'durMs':>6s} {'expectHz':>9s} {'measHz':>8s} {'cents':>8s}")
    for n, expect, f, c, dur_ms in rows:
        lyr = n.get("lyric", "")
        if f is None:
            print(f"{lyr:12s} {n['tone']:4d} {dur_ms:6.0f} {expect:9.1f} "
                  f"{'SKIP':>8s} {'-':>8s}")
            continue
        print(f"{lyr:12s} {n['tone']:4d} {dur_ms:6.0f} {expect:9.1f} {f:8.1f} {c:+8.1f}")
    measured = [abs(c) for _, _, f, c, _ in rows if f is not None]
    if measured:
        measured.sort()
        print(f"\n|cents| median={measured[len(measured) // 2]:.1f} "
              f"p90={measured[int(len(measured) * 0.9) - 1]:.1f} "
              f"max={measured[-1]:.1f}")
    print(f"checked={len(rows)}  out_of_tolerance={bad}  "
          f"skipped(no audio)={skipped}  tol=+/-{args.tol} cents")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())