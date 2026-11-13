#!/usr/bin/env python3
"""A/B audio metrics ที่ Lead ใช้เทียบ OpenUtau golden กับของเรา

นิยาม (ทุกค่า dBFS อ้าง full scale = 32768 = 0 dBFS — "ไม่ใช่" อ้าง peak):
  duration  : จำนวน sample / 44100
  rms_db    : 20*log10(RMS/32768) ทั้งไฟล์
  onset_ms   : หน้าต่าง 50 ms แรกที่ RMS > -30 dBFS ต่อเนื่อง >= 2 หน้าต่าง
               (≈ "เสียงเริ่มดัง" — ถ้าไม่พบให้ค่า nan)
  silence_%  : % ของหน้าต่าง 10 ms ที่ RMS < -45 dBFS
  clip       : จำนวน sample ที่ |x| >= 32767

ใช้:
  python3 test/golden/ab_metrics.py a.wav [b.wav ...]
"""
import math
import struct
import sys
import wave


def load(path):
    with wave.open(path) as w:
        ch = w.getnchannels()
        n = w.getnframes()
        raw = struct.unpack("<%dh" % (n * ch), w.readframes(n))
    if ch == 1:
        return list(raw)
    return [(raw[i * 2] + raw[i * 2 + 1]) / 2.0 for i in range(n)]


def rms(x):
    return math.sqrt(sum(float(v) * v for v in x) / max(1, len(x))) / 32768.0


def db(v):
    return 20 * math.log10(max(v, 1e-9))


def onset_ms(x, thresh_db=-30, hold_ms=50):
    hold = int(44100 * hold_ms / 1000)
    run = 0
    for i in range(0, len(x) - hold, hold):
        if db(rms(x[i:i + hold])) > thresh_db:
            run += 1
            if run >= 2:
                return i * 1000.0 / 44100
        else:
            run = 0
    return float("nan")


def silence_pct(x, floor_db=-45):
    win = 441
    silent = total = 0
    for i in range(0, len(x), win):
        total += 1
        if db(rms(x[i:i + win])) < floor_db:
            silent += 1
    return 100.0 * silent / max(1, total)


def report(path):
    x = load(path)
    peak = max(abs(v) for v in x) / 32768.0
    clip = sum(1 for v in x if abs(v) >= 32767)
    print("%-46s %7.0f ms  rms %5.1f dB  onset %5.0f ms  silence %4.1f%%  peak %.3f  clip %d"
          % (path.split("/")[-1], len(x) * 1000.0 / 44100, db(rms(x)),
             onset_ms(x), silence_pct(x), peak, clip))


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        raise SystemExit(2)
    for p in sys.argv[1:]:
        report(p)
