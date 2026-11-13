#!/usr/bin/env bash
# T5: run the REAL worldline-R2 neural chain in the browser (wasm) and compare it
# with the native CPU EP on identical tensors.
#
#   ./web/run-r2.sh                    # browser chain + parity vs onnxruntime-node
#   PARITY=0 ./web/run-r2.sh           # browser chain only
#   NATIVE_R2=1 ./web/run-r2.sh        # extra: full native R2 render via synth-cli
#
# Chain (mirrors .../Classic/WorldlineRenderer.cs:142-228):
#   f0 + sp_env + ap -> models/r2/mel.onnx -> mel -> pc-nsf-hifigan -> waveform
#   -> trim leftPad*512, take frames*512, ease 512 samples each end
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"

MEL_REL="models/r2/mel.onnx"
VOC_REL="models/r2/pc-nsf-hifigan/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.onnx"
MEL_SHA_WANT="1493ffc868c6fb8b66104e2c6a2902cbf1a8c02bb09215db014f02e9d0e82f94"
VOC_SHA_WANT="b297c0c790dbcfe6b2123d87cc20bf8718969f91ffa3170a198c8fb53686155d"
set +u
F1="$FRAMES"; F2="$FRAMES2"; PARITY="$PARITY"; NATIVE_R2="$NATIVE_R2"; proj="$PROJECT"; bank="$VOICEBANK"
set -u
[ -n "$F1" ] || F1=64
[ -n "$F2" ] || F2=97
[ -n "$PARITY" ] || PARITY=1
[ -n "$NATIVE_R2" ] || NATIVE_R2=0

out=/tmp/r2-run.txt
json=/tmp/r2-result.json
rssfile=/tmp/r2-rss.txt
wav=/tmp/r2-wasm.wav
natjson=/tmp/r2-native.json
natwav=/tmp/r2-native.wav
rm -f "$out" "$json" "$rssfile" "$wav" "$natjson" "$natwav"

if [ ! -f "$root/$MEL_REL" ] || [ ! -f "$root/$VOC_REL" ]; then
  cat >&2 <<'EOM'
missing R2 models. Fetch them (see models/r2/PROVENANCE.md):
  mkdir -p models/r2
  curl -fL https://github.com/openvpi/vocoders/releases/download/pc-nsf-hifigan-44.1k-hop512-128bin-2025.02/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep -o models/r2/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep
  python3 -c "import zipfile; zipfile.ZipFile('models/r2/pc_nsf_hifigan_44.1k_hop512_128bin_2025.02.oudep').extractall('models/r2/pc-nsf-hifigan')"
  cp 'ref(openutau+openutau mobile)/core/OpenUtau.Core/Classic/Data/mel.onnx' models/r2/mel.onnx
EOM
  exit 1
fi

# ---- 0. provenance: the two models must be the real ones -------------------
mel_sha="$(sha256sum "$root/$MEL_REL" | cut -d' ' -f1)"
voc_sha="$(sha256sum "$root/$VOC_REL" | cut -d' ' -f1)"
mel_mb="$(du -m "$root/$MEL_REL" | cut -f1)"
voc_mb="$(du -m "$root/$VOC_REL" | cut -f1)"
echo "==> provenance"
echo "    mel.onnx  $mel_mb MB sha256 $mel_sha"
echo "    vocoder   $voc_mb MB sha256 $voc_sha"
if [ "$mel_sha" != "$MEL_SHA_WANT" ]; then echo "mel.onnx sha256 mismatch (want $MEL_SHA_WANT)" >&2; exit 1; fi
if [ "$voc_sha" != "$VOC_SHA_WANT" ]; then echo "vocoder sha256 mismatch (want $VOC_SHA_WANT)" >&2; exit 1; fi

# ---- 1. vendored runtime --------------------------------------------------
bash "$here/vendor/fetch-ort.sh" >/dev/null
ort_mb="$(du -sm "$here/vendor/ort" | cut -f1)"

if command -v pgrep >/dev/null && [ "$(pgrep -c chromium || true)" != "0" ]; then
  echo "    note: $(pgrep -c chromium) chromium process(es) already running — timings may be noisy" >&2
fi

# ---- 1b. baseline: the same browser with nothing loaded (about:blank) ------
# Process-tree RSS is dominated by Chromium itself (task-4 measured ~1.08 GB for
# about:blank), so the RAM numbers below are only meaningful as a DELTA over this
# baseline. The wasm linear memory / JS heap are measured in the page itself.
base_rss=0; base_any=0
base_prof="$(mktemp -d /tmp/r2base-XXXXXX)"
chromium-browser --headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage   --no-zygote --no-first-run --no-default-browser-check   --user-data-dir="$base_prof" about:blank >/dev/null 2>&1 &
base_pid=$!
python3 "$here/vendor/rss-sampler.py" "$base_prof" /tmp/r2-rss-baseline.txt 10 >/dev/null 2>&1 || true
kill "$base_pid" 2>/dev/null || true
pkill -f -- "--user-data-dir=$base_prof" 2>/dev/null || true
sleep 0.5
if [ -s /tmp/r2-rss-baseline.txt ]; then
  read -r base_rss base_any _r < /tmp/r2-rss-baseline.txt || true
  case "$base_rss" in ''|*[!0-9]*) base_rss=0 ;; esac
  case "$base_any" in ''|*[!0-9]*) base_any=0 ;; esac
fi
echo "    baseline RSS (about:blank, same flags): $base_rss KB summed · biggest process $base_any KB"

# ---- 2. serve the repo and drive headless Chromium ------------------------
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

url="http://127.0.0.1:$port/web/r2-test.html?frames=$F1&frames2=$F2"
echo "==> headless chromium (port $port)"
echo "    $url"

prof="$(mktemp -d /tmp/r2poc-XXXXXX)"
chromium-browser --headless=new --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --no-zygote --no-first-run \
  --no-default-browser-check --user-data-dir="$prof" "$url" >/dev/null 2>&1 &
chromium_pid=$!

# Peak RSS of the Chromium tree while the chain runs (see web/vendor/rss-sampler.py).
python3 "$here/vendor/rss-sampler.py" "$prof" "$rssfile" 300 &
sampler_pid=$!

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

python3 - "$json" "$wav" "$mel_sha" "$voc_sha" "$ort_mb" "$rss_peak_sum" "$rss_peak_any" "$base_rss" "$base_any" <<'PY'
import base64, json, struct, sys, wave
jpath, wavpath, mel_sha, voc_sha, ort_mb, rss_sum, rss_any, base_sum, base_any = sys.argv[1:10]
R = json.load(open(jpath))
runs = R.get("runs", [])
fails = []
def need(cond, msg):
    if not cond: fails.append(msg)
need(R.get("verdict") == "PASS", "page verdict = %r (%s)" % (R.get("verdict"), R.get("error", "")))
need(R["mel"]["bytes"] == 85404, "mel.onnx bytes %r" % R["mel"]["bytes"])
need(R["mel"]["sha256"] == mel_sha, "mel sha256 %s != %s" % (R["mel"]["sha256"], mel_sha))
need(R["voc"]["bytes"] == 56616643, "vocoder bytes %r" % R["voc"]["bytes"])
need(R["voc"]["sha256"] == voc_sha, "vocoder sha256 %s != %s" % (R["voc"]["sha256"], voc_sha))
need(sorted(R["mel"]["inputs"]) == ["ap", "f0", "sp_env"], "mel inputs %r" % R["mel"]["inputs"])
need(R["mel"]["outputs"] == ["mel"], "mel outputs %r" % R["mel"]["outputs"])
need(sorted(R["voc"]["inputs"]) == ["f0", "mel"], "voc inputs %r" % R["voc"]["inputs"])
need(R["voc"]["outputs"] == ["waveform"], "voc outputs %r" % R["voc"]["outputs"])
need(len(runs) == 2, "expected 2 runs, got %d" % len(runs))
for r in runs:
    need(r["melDims"] == [1, r["padded"], 128], "%s mel dims %r" % (r["label"], r["melDims"]))
    need(r["waveDims"] == [1, r["padded"] * 512], "%s wave dims %r" % (r["label"], r["waveDims"]))
    need(r["samples"] == r["frames"] * 512, "%s samples %d != frames*512" % (r["label"], r["samples"]))
    need(r["nonFinite"] == 0, "%s non-finite %r" % (r["label"], r["nonFinite"]))
    need(r["melStats"]["nonFinite"] == 0, "%s mel non-finite" % r["label"])
    need(r["peak"] > 0, "%s peak %r" % (r["label"], r["peak"]))
    need(r["distinct"] > 1, "%s constant output" % r["label"])
    need(r["easeFirst"] == 0, "%s ease-in first sample %r" % (r["label"], r["easeFirst"]))
    need(r["melMs"] > 0 and r["vocMs"] > 0, "%s timing %r" % (r["label"], r))

wav = base64.b64decode(R["wavB64"])
open(wavpath, "wb").write(wav)
wf = wave.open(wavpath, "rb")
samps = wf.getnframes(); sr = wf.getframerate(); ch = wf.getnchannels(); sw = wf.getsampwidth()
frames_b = wf.readframes(samps); wf.close()
pcm = struct.unpack("<%dh" % (samps), frames_b)
pk = max(abs(v) for v in pcm) / 32768.0
need(sr == 44100 and ch == 1 and sw == 2, "wav fmt %d %d %d" % (sr, ch, sw))
need(samps == runs[0]["samples"] if runs else False, "wav frames %d" % samps)
need(pk > 0, "wav peak %r" % pk)

print("")
print("================ T5: R2 chain (mel.onnx -> pc-nsf-hifigan) on wasm ================")
print("mel.onnx        : %d B sha256 %s… (checked against PROVENANCE)" % (R["mel"]["bytes"], R["mel"]["sha256"][:16]))
print("vocoder onnx    : %.2f MB sha256 %s… (byte-identical to the T1 vocoder file)" % (R["voc"]["bytes"] / 1048576, R["voc"]["sha256"][:16]))
print("ort-web runtime : %s MB on disk · version %s · numThreads 1" % (ort_mb, R.get("ortVersion")))
print("import ort-web  : %d ms" % R["ortImportMs"])
print("load mel.onnx   : fetch %d ms + session %d ms" % (R["mel"]["fetchMs"], R["mel"]["sessionMs"]))
print("load vocoder    : fetch %d ms + session %d ms" % (R["voc"]["fetchMs"], R["voc"]["sessionMs"]))
print("graph I/O       : mel.onnx in %s out %s · vocoder in %s out %s" % (R["mel"]["inputs"], R["mel"]["outputs"], R["voc"]["inputs"], R["voc"]["outputs"]))
print("-" * 82)
for r in runs:
    print("frames=%-4d padded=%-4d -> mel[1,%d,128] in %6.1f ms · vocoder -> %6d samples in %7.1f ms · chain %7.1f ms (%4.2fx rt)"
          % (r["frames"], r["padded"], r["padded"], r["melMs"], r["waveDims"][1], r["vocMs"], r["chainMs"], r["realtimeX"]))
    print("             mel min %.4f max %.4f mean %.4f · trimmed %d = frames*512 · peak %.6f rms %.6f inputFnv f0=%d sp=%d ap=%d"
          % (r["melStats"]["min"], r["melStats"]["max"], r["melStats"]["mean"], r["samples"], r["peak"], r["rms"],
             r["inputFnv"]["f0"], r["inputFnv"]["sp"], r["inputFnv"]["ap"]))
print("-" * 82)
print("RAM (see caveat): baseline about:blank tree %.1f MB -> chain peak %.1f MB => delta %.1f MB (summed RSS)"
      % (float(base_sum) / 1024, float(rss_sum) / 1024, (float(rss_sum) - float(base_sum)) / 1024))
print("                  biggest single process: baseline %.1f MB -> chain %.1f MB"
      % (float(base_any) / 1024, float(rss_any) / 1024))
wm = R.get("wasmMemory") or {}
print("                  wasm linear memory: %s MB (final) · js heap %s / %s MB"
      % (round((wm.get("finalBytes") or 0) / 1048576, 1), R.get("jsHeapMB"), R.get("jsHeapLimitMB")))
print("                  CAVEAT: process-tree RSS is mostly Chromium; the delta is an upper bound,")
print("                  not isolated engine RAM (wasm linear memory is the real engine number).")
print("wasm WAV        : %s (%d bytes, re-parsed: %d frames @%d Hz, peak %.6f)" % (wavpath, len(wav), samps, sr, pk))
print("==================================================================================")
if fails:
    print("FAIL: " + "; ".join(fails))
    sys.exit(1)
print("PASS: both real R2 models loaded (mel.onnx sha256 ok, vocoder sha256 ok) and the full chain ran; "
      "%d + %d samples, mel dims exactly [1,T,128], audio finite/non-constant, ease-in zeroed."
      % (runs[0]["samples"], runs[1]["samples"]))
PY
browser_ok=$?
if [ "$browser_ok" != "0" ]; then echo "==> FAIL (browser chain)"; exit 1; fi

# ---- 3. parity: same tensors through the native CPU EP --------------------
parity_status="SKIPPED"
if [ "$PARITY" = "1" ]; then
  echo "==> parity: native CPU EP (onnxruntime-node) vs wasm EP, identical tensors"
  if bash "$here/vendor/fetch-onnxruntime-node.sh"; then
    node "$here/vendor/r2-parity.mjs" "$root/$MEL_REL" "$root/$VOC_REL" "$F1" "$F2" "$natjson" "$natwav"
    python3 - "$json" "$natjson" "$wav" "$natwav" <<'PY'
import json, struct, sys, wave
wj, nj, wasm_wav, nat_wav = sys.argv[1:5]
W = json.load(open(wj)); N = json.load(open(nj))
fails = []
def need(cond, msg):
    if not cond: fails.append(msg)
need(N.get("verdict") == "PASS", "native verdict %r (%s)" % (N.get("verdict"), N.get("error")))
print("  native sessions: mel %d ms · vocoder %d ms (onnxruntime-node)" % (N["melSessionMs"], N["vocSessionMs"]))
for w, n in zip(W.get("runs", []), N.get("runs", [])):
    need(w["inputFnv"] == n["inputFnv"], "input tensors differ (wasm %r vs native %r)" % (w["inputFnv"], n["inputFnv"]))
    print("  frames=%-4d input fnv identical · mel bit-identical: %-5s · audio(f32) bit-identical: %-5s"
          % (w["frames"], w["melFnv"] == n["melFnv"], w["wavFnv"] == n["wavFnv"]))
    print("                mel mean wasm %.6f / native %.6f · audio peak wasm %.6f / native %.6f"
          % (w["melStats"]["mean"], n["melStats"]["mean"], w["peak"], n["peak"]))
def read16(p):
    wf = wave.open(p, "rb"); n = wf.getnframes(); d = wf.readframes(n); wf.close()
    return n, struct.unpack("<%dh" % n, d)
n1, a = read16(wasm_wav); n2, b = read16(nat_wav)
need(n1 == n2, "wav frame counts %d vs %d" % (n1, n2))
diff = [abs(x - y) for x, y in zip(a, b)]
mx = max(diff) if diff else 0
nz = sum(1 for d in diff if d != 0)
print("  WAV parity (16-bit LSB, %d frames): max|diff| = %d LSB · differing samples = %d (%.4f%%)"
      % (n1, mx, nz, 100.0 * nz / max(n1, 1)))
need(mx <= 2, "WAV parity max|diff| = %d LSB (> 2)" % mx)
if fails:
    print("  PARITY FAIL: " + "; ".join(fails)); sys.exit(1)
print("  PARITY PASS: wasm EP and native CPU EP agree within %d LSB on identical tensors" % mx)
PY
    parity_status="PASS"
  else
    parity_status="SKIPPED (onnxruntime-node could not be installed — offline?)"
    echo "  $parity_status" >&2
  fi
else
  parity_status="SKIPPED (PARITY=0)"
fi

# ---- 4. optional: full native R2 render (real project, real WORLD features) ----
nat_status="SKIPPED (NATIVE_R2=0)"
if [ "$NATIVE_R2" = "1" ]; then
  cli="$root/native/target/release/synth-cli"
  so="$root/native/build/build-linux/libworldline.so"
  [ -n "$proj" ] || proj="$root/test/golden/sakura-test.ustx"
  [ -n "$bank" ] || bank="$root/test/golden/teto-english/library"
  if [ -x "$cli" ] && [ -f "$so" ]; then
    echo "==> native R2 render (synth-cli --engine r2)"
    native_rc=0
    WORLD_DUMP_MEL_FILE=/tmp/r2-native-meldump.bin \
      "$cli" render --engine r2 --mel-onnx "$root/$MEL_REL" --vocoder-onnx "$root/$VOC_REL" \
        --so "$so" --project "$proj" --voicebank "$bank" --out /tmp/r2-native-real.wav --verbose \
      2>&1 | tail -25 || native_rc=$?
    if [ "$native_rc" = "0" ] && [ -f /tmp/r2-native-real.wav ]; then
      nat_status="PASS (/tmp/r2-native-real.wav)"
    else
      nat_status="FAIL (exit $native_rc)"
    fi
  else
    nat_status="SKIPPED (synth-cli or libworldline.so missing)"
  fi
fi

echo "==> PASS"
echo "    parity vs native CPU EP : $parity_status"
echo "    native R2 render        : $nat_status"
echo "    wasm wav: $wav · native wav: $natwav · json: $json"
