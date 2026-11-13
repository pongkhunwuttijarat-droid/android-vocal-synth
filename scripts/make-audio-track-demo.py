#!/usr/bin/env python3
"""Build a demo project with a REAL audio track, for the wave-part paths.

The Hatsune golden project has one `wave_parts` entry, but it points at an MP3
outside the repository that nobody else has, so every render of that track skips
the audio part. This rewrites that entry to a wav generated here (4 s, 220 Hz,
16-bit mono at 44.1 kHz) and writes the pair into a directory you can point
`--media` and `--projects` at.

`file_duration_ms` is rewritten too, and that is the whole point: the golden file
still carries the ORIGINAL MP3's 288391.8367 ms while the wav is 4 s. The engine
turns that number into the part's `duration_ticks`, so leaving it stale makes the
part 72x longer than its audio and the piano roll draws 4 seconds of waveform
followed by a minute and a half of nothing. A real `.ustx` keeps the two in sync,
so this one does too.

    scripts/make-audio-track-demo.py <out-dir> [--source <golden.ustx>]

Everything stays byte-identical to the source except the two wave-part fields, so
the demo still carries the golden's 1709 notes for the voice-track paths.
"""
import argparse
import math
import struct
import wave
from pathlib import Path

SAMPLE_RATE = 44100
SECONDS = 4.0
FREQUENCY = 220.0
# 12000/32768 of full scale: a recognisable tone that is NOT full scale, so a
# round-trip that clips or scales by 2 is visible in the peak value.
AMPLITUDE = 12000

REPO = Path(__file__).resolve().parent.parent
DEFAULT_SOURCE = REPO / "test-ustx-example" / "golden-the-end-of-hutsune-miku.ustx"
DEFAULT_OUT = REPO / "native" / "target" / "media-demo"


def write_tone(path: Path, seconds: float = SECONDS) -> float:
    """A mono sine, and the wav's real length in ms."""
    frames = int(SAMPLE_RATE * seconds)
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(SAMPLE_RATE)
        out.writeframes(b"".join(
            struct.pack("<h", int(round(AMPLITUDE * math.sin(2 * math.pi * FREQUENCY * i / SAMPLE_RATE))))
            for i in range(frames)
        ))
    return frames * 1000.0 / SAMPLE_RATE


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("out_dir", nargs="?", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()

    text = args.source.read_bytes().decode("utf-8-sig")
    if "wave_parts:" not in text:
        raise SystemExit(f"{args.source} has no wave_parts entry; nothing to rewrite")

    out_dir: Path = args.out_dir
    out_dir.mkdir(parents=True, exist_ok=True)
    duration_ms = write_tone(out_dir / "bg.wav")

    marker = "wave_parts:"
    head, tail = text.split(marker, 1)
    lines = []
    for line in tail.splitlines(keepends=True):
        if line.startswith("  relative_path:"):
            line = "  relative_path: bg.wav\n"
        elif line.startswith("  file_duration_ms:"):
            line = f"  file_duration_ms: {duration_ms:.4f}\n"
        lines.append(line)
    # Every OpenUtau .ustx starts with a UTF-8 BOM; keep it so the round trip is
    # byte-faithful rather than only parseable.
    (out_dir / "demo.ustx").write_bytes(b"\xef\xbb\xbf" + (head + marker + "".join(lines)).encode("utf-8"))

    print(f"{out_dir / 'bg.wav'}: {duration_ms:.0f} ms, {FREQUENCY:.0f} Hz, peak {AMPLITUDE}/32768")
    print(f"{out_dir / 'demo.ustx'}: relative_path -> bg.wav, file_duration_ms -> {duration_ms:.4f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
