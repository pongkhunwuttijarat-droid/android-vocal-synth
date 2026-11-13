#!/usr/bin/env bash
# T1 / POC: run the REAL nsf_hifigan vocoder ONNX model in the browser through
# onnxruntime-web's wasm backend, headless, and report real numbers.
#
#   ./web/run-vocoder.sh
#
# What it proves (all re-checked in this script, not just by the page):
#   * the 56.6 MB model is fetched from disk and its sha256 matches the file in the repo
#   * the onnxruntime-web wasm session really loads it (input/output names from the graph)
#   * real inference: output length == n_frames * hop (hop = 512, from the ConvTranspose
#     strides in the model: 8*8*2*2*2), output is finite, non-silent and not constant
#   * a WAV is written to /tmp/vocoder-poc.wav and re-parsed here as an independent check
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
model_rel="${MODEL:-test/golden/diffsinger-kiritan/nsf_hifigan/model.onnx}"
n1="${N1:-64}"; n2="${N2:-97}"

out=/tmp/vocoder-poc-run.txt
json=/tmp/vocoder-poc-result.json
rssfile=/tmp/vocoder-poc-rss.txt
wav=/tmp/vocoder-poc.wav
rm -f "$out" "$json" "$rssfile" "$wav"

echo "==> vendored ort-web"
bash "$here/vendor/fetch-ort.sh"
ort_mb="$(du -sm "$here/vendor/ort" | cut -f1)"
model_mb="$(du -m "$root/$model_rel" | cut -f1)"
model_sha_want="$(sha256sum "$root/$model_rel" | cut -d' ' -f1)"
echo "    ort-web dir ${ort_mb} MB · model ${model_mb} MB · sha256 ${model_sha_want:0:16}…"

if command -v pgrep >/dev/null && [ "$(pgrep -c chromium || true)" != "0" ]; then
  echo "    note: $(pgrep -c chromium) chromium process(es) already running — timings may be noisy" >&2
fi

python3 "$here/bench-server.py" "$root" > "$out" 2>&1 &
server=$!
chromium_pid=""; sampler_pid=""
cleanup() {
  [ -n "$sampler_pid" ] && kill "$sampler_pid" 2>/dev/null || true
  [ -n "$chromium_pid" ] && kill "$chromium_pid" 2>/dev/null || true
  kill "$server" 2>/dev/null || true
}
trap cleanup EXIT

port=""
for _ in $(seq 1 40); do
  if grep -q '^PORT' "$out" 2>/dev/null; then port="$(awk '/^PORT/ {print $2; exit}' "$out")"; break; fi
  sleep 0.25
done
if [ -z "$port" ]; then echo "server did not start" >&2; cat "$out" >&2; exit 1; fi

url="http://127.0.0.1:$port/web/vocoder-test.html?model=/$model_rel&n1=$n1&n2=$n2"
echo "==> headless chromium (port $port)"
echo "    $url"

prof="$(mktemp -d /tmp/vocpoc-XXXXXX)"   # short + unique: used as the process-tree identity token
chromium-browser --headless=new --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --no-zygote --no-first-run \
  --no-default-browser-check --user-data-dir="$prof" "$url" >/dev/null 2>&1 &
chromium_pid=$!

# Peak RSS of the whole Chromium process tree. It is identified by the throw-away
# --user-data-dir (our identity token), NOT by the launcher's pid: on Debian
# /usr/bin/chromium-browser is a wrapper script that exits and leaves the real
# browser (plus zygote/renderer/GPU) as unrelated-looking processes.
python3 - "$prof" "$rssfile" 300 <<'PY' &
import os, subprocess, sys, time
prof, path, secs = sys.argv[1], sys.argv[2], int(sys.argv[3])
peak_sum = peak_any = max_roots = 0; end = time.time() + secs; seen_any = False; gone = 0
def dump():
    tmp = path + ".tmp"                       # atomic: the shell may read mid-run
    with open(tmp, "w") as fh:
        fh.write("%d %d %d\n" % (peak_sum, peak_any, max_roots))
    os.replace(tmp, path)
while time.time() < end:
    try:
        # -ww: without it ps truncates the arg list before --user-data-dir, so the
        # identity token would never match and the sampler would report 0.
        rows = subprocess.run(["ps", "-ww", "-eo", "pid=,ppid=,rss=,args="], capture_output=True, text=True).stdout.splitlines()
    except Exception:
        break
    procs = {}
    for line in rows:
        parts = line.split(None, 3)
        if len(parts) < 3: continue
        try: procs[int(parts[0])] = (int(parts[1]), int(parts[2]), parts[3] if len(parts) > 3 else "")
        except ValueError: continue
    roots = [p for p, (_, _, args) in procs.items() if prof and prof in args]
    if not roots:
        if seen_any:
            gone += 1
            if gone > 25: break          # browser really exited (5 s)
        dump(); time.sleep(0.2); continue
    seen_any = True; gone = 0
    max_roots = max(max_roots, len(roots))
    kids = {}
    for pid, (ppid, _, _) in procs.items(): kids.setdefault(ppid, []).append(pid)
    seen, stack, total = set(), list(roots), 0
    while stack:
        p = stack.pop()
        if p in seen or p not in procs: continue
        seen.add(p); total += procs[p][1]; stack.extend(kids.get(p, []))
    peak_sum = max(peak_sum, total)
    peak_any = max(peak_any, max((procs[p][1] for p in seen), default=0))
    dump()                       # incremental: we get SIGTERM'd before the loop ends
    time.sleep(0.2)
dump()
PY
sampler_pid=$!

# Wait for the JSON result POST itself (not just the progress line), so the numbers are
# always in the log before we parse them.
for _ in $(seq 1 640); do
  if grep -qE '^RESULT \{' "$out" 2>/dev/null; then break; fi
  if grep -qE '^(RESULT: FAIL|TIMEOUT)' "$out" 2>/dev/null; then sleep 1; break; fi
  sleep 0.5
done
sleep 1
kill "$sampler_pid" 2>/dev/null || true; sampler_pid=""
kill "$chromium_pid" 2>/dev/null || true; chromium_pid=""
pkill -f -- "--user-data-dir=$prof" 2>/dev/null || true
sleep 0.3

# The page posts the full numeric result to /__result; bench-server prints "RESULT {json}".
grep -E '^RESULT \{' "$out" | tail -1 | sed 's/^RESULT //' > "$json" || true
if [ ! -s "$json" ]; then
  echo "==> FAILED: no result POSTed by the page" >&2
  tail -40 "$out" >&2
  exit 1
fi
rss_peak_sum=0; rss_peak_any=0
if [ -s "$rssfile" ]; then
  read -r rss_peak_sum rss_peak_any _rest < "$rssfile" || true
  case "$rss_peak_sum" in ''|*[!0-9]*) rss_peak_sum=0 ;; esac
  case "$rss_peak_any" in ''|*[!0-9]*) rss_peak_any=0 ;; esac
fi

python3 - "$json" "$wav" "$model_rel" "$model_sha_want" "$ort_mb" "$model_mb" "$rss_peak_sum" "$rss_peak_any" <<'PY'
import base64, json, struct, sys, wave
jpath, wavpath, model_rel, sha_want, ort_mb, model_mb, rss_sum, rss_any = sys.argv[1:9]
R = json.load(open(jpath))
fails = []
def need(cond, msg):
    if not cond: fails.append(msg)

need(R.get("verdict") == "PASS", "page verdict = %r (%s)" % (R.get("verdict"), R.get("error", "")))
need(R.get("modelBytes") == 56616643, "model bytes = %r" % R.get("modelBytes"))
need(R.get("modelSha256") == sha_want, "model sha256 mismatch: page %s vs disk %s" % (R.get("modelSha256"), sha_want))
need(sorted(R.get("inputNames", [])) == ["f0", "mel"], "input names %r" % R.get("inputNames"))
need(R.get("outputNames") == ["waveform"], "output names %r" % R.get("outputNames"))
runs = R.get("runs", [])
need(len(runs) == 3, "expected 3 runs, got %d" % len(runs))
for r in runs:
    need(r["samples"] == r["nFrames"] * 512, "%s: samples %d != n*512 %d" % (r["label"], r["samples"], r["nFrames"] * 512))
    need(r["hop"] == 512, "%s: hop %r" % (r["label"], r["hop"]))
    need(r["nonFinite"] == 0, "%s: non-finite samples %r" % (r["label"], r["nonFinite"]))
    need(r["peak"] > 1e-3, "%s: silent (peak %r)" % (r["label"], r["peak"]))
    need(r["distinctLe8"] > 1, "%s: constant output" % r["label"])
    need(r["ms"] > 0, "%s: inference time %r" % (r["label"], r["ms"]))

# Independent re-check from the WAV bytes the page produced.
wav = base64.b64decode(R["wavB64"])
open(wavpath, "wb").write(wav)
wf = wave.open(wavpath, "rb")
samps = wf.getnframes(); sr = wf.getframerate(); ch = wf.getnchannels(); sw = wf.getsampwidth()
frames = wf.readframes(samps); wf.close()
pcm = list(struct.unpack("<%dh" % (len(frames) // 2), frames))
pk = max(abs(v) for v in pcm) / 32768.0
uniq = len(set(pcm[:4000]))
need(sr == 44100 and ch == 1 and sw == 2, "wav fmt %d Hz %d ch %d bytes" % (sr, ch, sw))
need(samps == runs[0]["samples"] if runs else False, "wav frames %d != reported %r" % (samps, runs[0]["samples"] if runs else None))
need(pk > 1e-3, "wav peak %r" % pk)
need(uniq > 1, "wav is constant")

print("")
print("================ vocoder-wasm POC — measured numbers ================")
print("model                : %s (%.2f MB) sha256 %s…" % (model_rel, R["modelBytes"] / 1048576, R["modelSha256"][:16]))
print("ort-web runtime      : %s MB on disk (dist files in web/vendor/ort)" % ort_mb)
print("ort-web version      : %s" % (R.get("ortVersion") or "n/a"))
print("import ort-web       : %d ms" % R["ortImportMs"])
print("model fetch (HTTP)   : %d ms (%.2f MB/s)" % (R["downloadMs"], R["modelBytes"] / 1048576 / max(R["downloadMs"], 1e-3) * 1000))
print("session create+init  : %d ms   <- includes 14 MB wasm instantiate + weights upload" % R["sessionMs"])
print("total load           : %d ms" % (R["downloadMs"] + R["sessionMs"]))
print("graph I/O            : in %s / out %s" % (R["inputNames"], R["outputNames"]))
print("-" * 70)
for r in runs:
    print("%-12s n=%-4d -> %6d samples (%.2f s audio) · %7.1f ms · %5.2fx realtime · peak %.4f · rms %.4f · zcr %.0f Hz"
          % (r["label"], r["nFrames"], r["samples"], r["seconds"], r["ms"], r["realtimeX"], r["peak"], r["rms"], r["zcrHz"]))
print("hop = %d samples/frame (from ConvTranspose strides 8x8x2x2x2) · all lengths == n*hop" % runs[0]["hop"])
print("determinism run1 vs run2 (same input): max|diff| = %.3g" % R.get("runDeterminismMaxDiff", -1))
print("-" * 70)
probes = R.get("probes", [])
if probes:
    print("mel-offset probe — same f0 (186 Hz glide), swept mel DC; mel scale of pc-nsf-hifigan is")
    print("undocumented, so this shows the output really depends on the mel (input -> amplitude response):")
    for p in probes:
        print("  melDc=%-4d peak %.4f  rms %.5f  zcr %5.0f Hz  dominant autocorr %4d Hz (corr %.2f)"
              % (p["melDc"], p["peak"], p["rms"], p["zcrHz"], p["domAutocorrHz"], p["domCorr"]))
    probe_peaks = [p["peak"] for p in probes]
    print("  peak(max/min) over the sweep = %.1fx  => the vocoder output is driven by its mel input"
          % (max(probe_peaks) / max(min(probe_peaks), 1e-12)))
    print("  note: dominant autocorr sits at 44100/512 = 86.1 Hz = the frame rate (upsampling artifact),")
    print("        so autocorrelation cannot read the pitch of this vocoder's output; not claimed here.")
print("-" * 70)
print("peak RSS proc tree   : %.1f MB (peak of the summed RSS of the Chromium tree) · biggest single process %.1f MB"
      % (float(rss_sum) / 1024, float(rss_any) / 1024))
print("js heap (used/limit) : %s / %s MB" % (R.get("jsHeapMB"), R.get("jsHeapLimitMB")))
print("wav                  : %s (%d bytes, fnv %s, re-parsed independently: %d frames @%d Hz, peak %.4f)"
      % (wavpath, R["wavBytes"], R.get("wavFnv"), samps, sr, pk))
print("page total           : %d ms" % R["totalMs"])
if R.get("warnings"):
    print("ort/browser warnings : %s" % "; ".join(R["warnings"][:4]))
print("=====================================================================")
if fails:
    print("FAIL: " + "; ".join(fails))
    sys.exit(1)
print("PASS: model loaded from disk (sha256 ok) and really executed; %d samples, hop %d, non-constant, finite."
      % (runs[0]["samples"], runs[0]["hop"]))
PY
echo "==> PASS"
