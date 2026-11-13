#!/usr/bin/env python3
"""audio_reason — ให้โมเดล "ฟัง" หรือ "มอง" ไฟล์เสียง ผ่าน OpenRouter mimo-v2.6-flash

ทางเลือก (mode):
  audio   ส่ง wav เป็น input_audio ตรง ๆ — ใช้ได้กับไฟล์สั้น (ผลทดสอบ: <=3.5 วิ ผ่าน / 14.5 วิ ถูก upstream ถอดเป็น transcript)
  spectro ตัด ffmpeg showspectrumpic → PNG แล้วส่งเป็น image (fallback ของไฟล์ยาว — image ใช้ได้แน่นอน)
  chunks  ตัดเป็นชิ้น <= CHUNK_SEC วิ แล้วส่งทีละชิ้นเป็น audio (ฟังได้ทั้งเพลงแต่จ่าย token ตามจำนวนชิ้น)
  auto    (default) ทุกไฟล์ <= AUDIO_MAX_SEC วิ → audio · ไม่เช่นนั้น → chunks

ใช้:
  python3 test/golden/audio_reason.py a.wav [b.wav ...] [--question "..."] [--mode auto|audio|spectro|chunks] [--max-chunks N]

ตัวอย่าง (เรียกผ่าน subagent — คัดลอก prompt นี้ไปได้เลย):
  "รันคำสั่งนี้แล้วสรุปผลภาษาไทยให้ผมฟัง:
   python3 test/golden/audio_reason.py out/ab/X.wav out/ab/Y.wav --question 'ไฟล์ไหนถูกต้องกว่า และมีอาการผิดปกติตรงไหน'"
"""
import argparse, base64, json, math, os, re, subprocess, sys, tempfile, urllib.error, urllib.request, yaml

CRED = "/home/seal/.dsh/.credentials.yaml"
MODEL = "xiaomi/mimo-v2.6-flash"
AUDIO_MAX_SEC = 4.0     # เหนือกว่านี้ upstream มักถอดเป็น transcript (ผลทดสอบ 3.5 ผ่าน / 14.5 / 30 fail)
CHUNK_SEC = 3.0
DEFAULT_MAX_CHUNKS = 12


def api_key():
    try:
        k = yaml.safe_load(open(CRED))["refs"]["OPENROUTERE_API_KEY"]
    except Exception as e:
        sys.exit("อ่าน key ไม่ได้ (%s) — ตรวจ %s" % (e, CRED))
    if not k:
        sys.exit("OPENROUTERE_API_KEY ว่างใน " + CRED)
    return k


def duration(path):
    try:
        out = subprocess.check_output(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration",
             "-of", "default=nw=1:nk=1", path]).decode().strip()
        return float(out)
    except Exception:
        return -1.0


def to_mono16k(src, tag):
    dst = tempfile.gettempdir() + "/ar16k-%s.wav" % tag
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", src, "-ac", "1", "-ar", "16000", dst], check=True)
    return dst


def spectro_png(src, tag):
    dst = tempfile.gettempdir() + "/ar-spec-%s.png" % tag
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", src,
                    "-lavfi", "showspectrumpic=s=1600x760:legend=1:fscale=log",
                    "-frames:v", "1", dst], check=True)
    return dst


def chunks_of(src, tag, sec=CHUNK_SEC):
    pattern = tempfile.gettempdir() + "/archunk-%s-%%03d.wav" % tag
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", src, "-f", "segment",
                    "-segment_time", str(sec), "-ac", "1", "-ar", "16000", pattern], check=True)
    d = tempfile.gettempdir()
    return sorted(p for p in os.listdir(d)
                  if p.startswith("archunk-%s-" % tag) and p.endswith(".wav"))[:100] and \
        [os.path.join(d, p) for p in sorted(os.listdir(d))
         if p.startswith("archunk-%s-" % tag) and p.endswith(".wav")]


def audio_part(path):
    b64 = base64.b64encode(open(path, "rb").read()).decode()
    return {"type": "input_audio", "input_audio": {"data": b64, "format": "wav"}}


def image_part(path):
    b64 = base64.b64encode(open(path, "rb").read()).decode()
    return {"type": "image_url", "image_url": {"url": "data:image/png;base64," + b64}}


# ลักษณะการล้มเหลวที่พบจริงจากการทดสอบ:
#   - "ไม่ได้ยิน / ฟังไม่ได้"  (upstream บาง request ไม่ส่งเสียงถึงโมเดล)
#   - "ได้เฉพาะ transcript / ข้อความถอดเสียง" (upstream ถอด ASR ไฟล์ยาว)
#   - content ว่าง (finish=length โดน reasoning กิน cap จนไม่ถึงเนื้อหา)
FAIL_SIG = re.compile(
    r"ไม่ได้ยิน|ไม่สามารถฟัง|ฟังไม่ได้|ไม่ได้รับเสียง|ไม่ถึงผมเป็นเสียง|"
    r"ข้อความถอดเสียง|ถอดเป็นข้อความ|เฉพาะ.{0,20}transcript", re.I)
RETRIES = 3


def _one(parts, prompt, max_tokens):
    key = api_key()
    body = {"model": MODEL,
            "messages": [{"role": "user", "content": parts + [{"type": "text", "text": prompt}]}],
            "max_tokens": max_tokens, "usage": {"include": True},
            "reasoning": {"enabled": False}}
    req = urllib.request.Request("https://openrouter.ai/api/v1/chat/completions",
                                 data=json.dumps(body).encode(),
                                 headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=300) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        sys.exit("HTTP %s: %s" % (e.code, e.read().decode()[:400]))
    except Exception as e:
        sys.exit("ERR %s: %s" % (type(e).__name__, e))


def ask(parts, prompt, max_tokens=2500):
    out, content, hits = None, None, 0
    for attempt in range(1, RETRIES + 1):
        out = _one(parts, prompt, max_tokens)
        content = (out["choices"][0]["message"].get("content") or "").strip()
        hit = FAIL_SIG.search(content)
        if content and not hit:
            break
        hits = attempt
        if attempt < RETRIES:
            why = "content ว่าง" if not content else "fail-sig: " + hit.group(0)
            print("[retry %d/%d — %s]" % (attempt, RETRIES, why), file=sys.stderr)
    u = out.get("usage", {})
    print(content or "(ว่าง)")
    print("\n[mode metadata · finish=%s · attempts=%d · in=%s out=%s · cost=$%s · model=%s]" % (
        out["choices"][0].get("finish_reason"), hits + 1, u.get("prompt_tokens"),
        u.get("completion_tokens"), u.get("cost"), out.get("model")))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+")
    ap.add_argument("--question", default=None)
    ap.add_argument("--mode", choices=["auto", "audio", "spectro", "chunks"], default="auto")
    ap.add_argument("--max-chunks", type=int, default=DEFAULT_MAX_CHUNKS)
    ap.add_argument("--blind", action="store_true", help="ไม่บอกว่าไฟล์ไหนคืออะไร (เทียบแบบไม่ลำเอียง)")
    a = ap.parse_args()

    for f in a.files:
        if not os.path.exists(f):
            sys.exit("ไม่พบไฟล์: " + f)

    durs = [(f, duration(f)) for f in a.files]
    mode = a.mode
    if mode == "auto":
        mode = "audio" if all(0 < d <= AUDIO_MAX_SEC for _, d in durs) else "chunks"

    parts, notes = [], []
    if mode == "audio":
        for i, (f, _) in enumerate(durs, 1):
            parts.append(audio_part(to_mono16k(f, "%d" % i)))
            notes.append("ไฟล์ %d = %s (%.1f วิ, 16k mono)" % (i, os.path.basename(f), duration(f)))
    elif mode == "spectro":
        for i, (f, d) in enumerate(durs, 1):
            parts.append(image_part(spectro_png(f, "%d" % i)))
            notes.append("ไฟล์ %d = %s — นี่คือ spectrogram (แกนนอน=เวลา แกนตั้ง=ความถี่ log) ไม่ใช่เสียง" % (i, os.path.basename(f)))
    else:  # chunks
        total = 0
        for fi, (f, d) in enumerate(durs, 1):
            files = chunks_of(f, "%d" % fi)
            if len(files) > a.max_chunks:  # สุ่มกระจายทั่วเพลง
                step = max(1, len(files) // a.max_chunks)
                files = files[::step][:a.max_chunks]
            for ci, cf in enumerate(files, 1):
                parts.append(audio_part(cf))
                total += 1
            notes.append("ไฟล์ %d = %s (%.1f วิ) ถัด %d ชิ้น ๆ ละ %.0f วิ" % (
                fi, os.path.basename(f), d, len(files), CHUNK_SEC))
        notes.append("รวม %d ชิ้น audio" % total)

    head = ["สิ่งที่ได้รับ:"] + ["  " + n for n in notes]
    if a.blind:
        # ห้ามรั่วชื่อไฟล์ (ชื่อบอกอยู่แล้วว่าไฟล์ไหนของใคร = ทำให้ "blind" เป็นโมฆะ)
        head = [re.sub(r"=\s*\S+\.wav", "= ไฟล์ %d.wav" % 0, n) for n in notes]
        head = ["สิ่งที่ได้รับ:"] + [
            re.sub(r"(ไฟล์ \d+) = \S+\.wav", r"\1 = ไฟล์ \1 (ชื่อถูกปิดบัง)", n) for n in notes]
    if mode == "spectro":
        head += ["หมายเหตุ: ได้ภาพ spectrogram ไม่ใช่เสียง — ประเมินจากภาพ (ความหนาแน่น/แนวตั้ง/ช่วงว่าง/การเริ่ม-จบ) เท่านั้น ถ้าไม่มั่นใจให้บอกว่ามองไม่ชัด"]
    if a.blind:
        head.append("ทุกไฟล์เป็นชิ้นงานชุดเดียวกันแต่คนละวิธีเรนเดอร์ — ตอบแบบ blind ไม่รู้ว่าไฟล์ไหนของใคร")
    q = a.question or ("ตอบภาษาไทยสั้น ๆ: (1) ไฟล์ไหนถูกต้อง/สม่ำเสมอที่สุด → แย่สุด พร้อมเหตุผล (2) อาการผิดปกติ + ตำแหน่งวินาที (3) ไฟล์ไหนใกล้ 'ต้นฉบับ/อ้างอิง' ที่สุด — ถ้าไม่ได้ยิน/ไม่ชัดให้บอกตรง ๆ")
    print("mode =", mode)
    ask(parts, "\n".join(head) + "\n\n" + q)


if __name__ == "__main__":
    main()
