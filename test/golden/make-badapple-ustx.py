#!/usr/bin/env python3
"""Generate `badapple.ustx` from the Flutter demo data.

The app ships the parsed Bad Apple!! UST as Dart
(`app/lib/data/badapple_demo.dart` — 867 notes, tempo 138); this turns it back
into a real project file so the native CLI, the server and the browser can all
render the same song. `Note.pitch` is semitones from C4 (tone = 60 + pitch) and
`position`/`duration` are beats (see `app/lib/editor_ustx.dart`).

    python3 test/golden/make-badapple-ustx.py
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SRC = ROOT / "app/lib/data/badapple_demo.dart"
OUT = (pathlib.Path(sys.argv[1]) if len(sys.argv) > 1
       else pathlib.Path(__file__).resolve().with_name("badapple.ustx"))

TICKS_PER_BEAT = 480
BPM = 138
NAME = "Bad Apple"

NOTE_RX = re.compile(
    r"Note\(lyric:\s*'((?:[^'\\]|\\.)*)',\s*pitch:\s*(-?\d+),"
    r"\s*position:\s*([0-9.]+),\s*duration:\s*([0-9.]+)\)"
)


def parse_notes(text):
    out = []
    for m in NOTE_RX.finditer(text):
        lyric, pitch, pos, dur = m.group(1), int(m.group(2)), float(m.group(3)), float(m.group(4))
        out.append((lyric.replace("\\'", "'"), 60 + pitch, round(pos * TICKS_PER_BEAT), round(dur * TICKS_PER_BEAT)))
    return out


def main():
    notes = parse_notes(SRC.read_text(encoding="utf-8"))
    if not notes:
        raise SystemExit("no notes parsed from " + str(SRC))
    last = max(p + d for _, _, p, d in notes)
    body = []
    # Every note needs a non-null pitch block: OpenUtau's UNote.Validate reads
    # pitch.snapFirst and crashes with NullReferenceException otherwise, so a
    # project without pitch blocks renders on our loader but not in OpenUtau.
    # Keep it identical to test/golden/one-note.ustx (flat, vibrato disabled).
    NOTE_PITCH = (
        "    pitch:\n"
        "      data:\n"
        "      - x: -1\n"
        "        y: 0\n"
        "        shape: io\n"
        "      - x: 1\n"
        "        y: 0\n"
        "        shape: io\n"
        "      snap_first: true\n"
        "    vibrato: {length: 0, period: 175, depth: 25, in: 10, out: 10, shift: 0, drift: 0}"
    )
    for lyric, tone, pos, dur in notes:
        body.append("  - position: %d\n    duration: %d\n    tone: %d\n    lyric: '%s'\n%s"
                    % (pos, max(dur, 1), tone, lyric.replace("'", "''"), NOTE_PITCH))
    yaml = "\n".join([
        "ustx_version: 0.9",
        "name: %s" % NAME,
        "comment: generated from app/lib/data/badapple_demo.dart",
        # NOTE: no output_dir/cache_dir — OpenUtau's Validate/CreateDirectory
        # throws on an empty path (the golden harness crashed on these once).
        "bpm: %d" % BPM,
        "beat_per_bar: 4",
        "beat_unit: 4",
        "resolution: 1",
        "expressions: {}",
        "time_signatures:",
        "- bar_position: 0",
        "  beat_per_bar: 4",
        "  beat_unit: 4",
        "tempos:",
        "- position: 0",
        "  bpm: %d" % BPM,
        "tracks:",
        "- track_name: %s" % NAME,
        "  singer: ''",
        "  phonemizer: ''",
        "voice_parts:",
        "- name: P",
        "  track_no: 0",
        "  position: 0",
        "  notes:",
        "\n".join(body),
        "",
    ])
    OUT.write_text(yaml, encoding="utf-8")
    print("%s: %d notes, %d ticks (%.1f s @ %d bpm)" % (OUT.name, len(notes), last, last / TICKS_PER_BEAT * 60 / BPM, BPM))


if __name__ == "__main__":
    main()
