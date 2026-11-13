#!/usr/bin/env python3
"""Truncate test/golden/badapple.ustx at the point OpenUtau still reaches.

The OpenUtau reference harness (`golden-renderer phrase`) builds its render
phrase list from a part whose async phonemizer is still settling, so on large
projects it silently renders only part of the project (measured: 97 of ~150
phrases, 308.46 s of a 401.74 s span). This fixture keeps only the notes whose
`position` is below a tick cutoff so both engines render the same span.

Usage: python3 scripts/make-badapple-trunc.py [out.ustx] [cutoff_ticks]
"""
import re
import sys

SRC = "test/golden/badapple.ustx"
DEFAULT_OUT = "out/bench/badapple-trunc.ustx"
# 401.62 s * (138 bpm / 60) * 480 ticks-per-beat
DEFAULT_CUT = 443367

note_start = re.compile(r"^  - position: (\d+)$")


def main() -> int:
    out_path = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_OUT
    cut = int(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_CUT

    lines = open(SRC, encoding="utf-8", errors="replace").read().split("\n")
    starts = [i for i, l in enumerate(lines) if note_start.match(l)]
    if not starts:
        print("no note blocks found", file=sys.stderr)
        return 1

    kept, dropped = [], 0
    for k, i in enumerate(starts):
        end = starts[k + 1] if k + 1 < len(starts) else len(lines)
        if int(note_start.match(lines[i]).group(1)) < cut:
            kept.extend(lines[i:end])
        else:
            dropped += 1

    text = "\n".join(lines[: starts[0]] + kept)
    open(out_path, "w", encoding="utf-8").write(text)

    pairs = [(int(a), int(b)) for a, b in
             re.findall(r"-\s*position:\s*(\d+)\s*\n\s*duration:\s*(\d+)", text)]
    span = max(p + d for p, d in pairs)
    print(f"{out_path}: {len(pairs)} notes kept, {dropped} dropped, "
          f"span {span} ticks = {span / 480 * 60 / 138:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
