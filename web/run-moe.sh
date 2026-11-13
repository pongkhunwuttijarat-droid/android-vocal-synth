#!/usr/bin/env bash
# T4/T7 POC: run the REAL burn MoE acoustic forward (native/crates/moe-train engine,
# loaded from a real .mpk checkpoint) inside wasm32, headless, in Chromium — and
# compare the wasm VALUES with the same engine running natively on this machine.
#
#   ./web/run-moe.sh
#
# What this proves (every claim has an assert below; 7 groups, see "asserts" in the output):
#   G1 the page result is intact (verdict, checkpoint sha256, byte count)
#   G2 the wasm instance really holds the checkpoint weights — 3 independent legs:
#      (a) the wasm module imports ONLY wasm-bindgen JS shims: no WASI, no filesystem
#      (b) load_checkpoint(bytes) materialised N parameter elements inside wasm,
#          where N is counted INDEPENDENTLY from the .mpk bytes by tools/mpk_param_count.py
#      (c) the wasm output equals the native output (G7) — same engine, same input
#      (linear-memory growth is reported too, but it is NOT the evidence for this claim)
#   G3 load/instantiate timings were recorded
#   G4 every forward pass returned a real mel (length, finite, non-constant, non-zero)
#   G5 repeated runs are bit-identical
#   G6 the output depends on the input
#   G7 wasm and native agree numerically (values only — see the timing caveat below)
#
# TIMING: the absolute numbers are INDICATIVE, not a benchmark. The wasm side and the
# native side run in different processes (native moe-ref finishes before Chromium opens)
# and the wasm number includes JS<->wasm marshalling, so only VALUES are compared strictly.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
model_rel="${MODEL:-test/poc/bigset/w9-2048-model-g0.mpk}"
rows="${ROWS:-1,128,1024}"
cd "$root"

out=/tmp/moe-wasm-run.txt              # bench-server log (progress lines + RESULT json)
json=/tmp/moe-wasm-result.json         # what the page POSTed to /__result
ref=/tmp/moe-native-ref.json           # native reference (same engine/input, other host)
rssfile=/tmp/moe-wasm-rss.txt          # peak RSS of the Chromium process tree
imp_cargo=/tmp/moe-wasm-imports-cargo.json   # import/export sections of the cargo .wasm
imp_glue=/tmp/moe-wasm-imports-glue.json     # ... and of the wasm-bindgen output
mpkfacts=/tmp/moe-mpk-facts.json             # independent element count of the .mpk
wasm_dir=native/crates/moe-wasm/www/pkg
tools=native/crates/moe-wasm/tools
rm -f "$out" "$json" "$ref" "$rssfile" "$imp_cargo" "$imp_glue" "$mpkfacts"

echo "==> 1/5 build wasm (release) + wasm-bindgen glue + independent section analysis"
cargo build --manifest-path "$root/native/Cargo.toml" -p moe-wasm --lib \
  --target wasm32-unknown-unknown --release
wasm_file="$root/native/target/wasm32-unknown-unknown/release/moe_wasm.wasm"
wasm-bindgen --target web --out-dir "$wasm_dir" "$wasm_file"
wasm_bytes=$(stat -c%s "$wasm_file")
wasm_sha=$(sha256sum "$wasm_file" | cut -d" " -f1)
python3 "$tools/wasm_imports.py" "$wasm_file" > "$imp_cargo"
python3 "$tools/wasm_imports.py" "$wasm_dir/moe_wasm_bg.wasm" > "$imp_glue"
echo "    .wasm ${wasm_bytes} B · sha256 ${wasm_sha:0:16}… · imports $(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['importCount'])" "$imp_cargo") (cargo) / $(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['importCount'])" "$imp_glue") (glue)"

echo "==> 2/5 native reference (same engine + same input, different process/host)"
# --release on purpose: the wasm module is built in release, so the native reference must be
# too or the (indicative) timing comparison would be meaningless. Values do not depend on it.
cargo run -q --release --manifest-path "$root/native/Cargo.toml" -p moe-wasm --bin moe-ref -- \
  "$model_rel" "$rows" > "$ref"
python3 "$tools/mpk_param_count.py" "$model_rel" > "$mpkfacts"
model_sha_want=$(sha256sum "$root/$model_rel" | cut -d" " -f1)
model_mb=$(du -m "$root/$model_rel" | cut -f1)
echo "    model $model_rel (${model_mb} MB) sha256 ${model_sha_want:0:16}… · $(python3 -c "import json,sys;d=json.load(open(sys.argv[1]));print('%d tensors / %d parameter elements (independent msgpack count)' % (d['tensors'], d['params']))" "$mpkfacts")"

if command -v pgrep >/dev/null && [ "$(pgrep -c chromium || true)" != "0" ]; then
  echo "    note: $(pgrep -c chromium) chromium process(es) already running — timings may be noisy" >&2
fi

echo "==> 3/5 serve the repo root and open the page in headless Chromium"
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
  if grep -q "^PORT" "$out" 2>/dev/null; then port="$(awk "/^PORT/ {print \$2; exit}" "$out")"; break; fi
  sleep 0.25
done
if [ -z "$port" ]; then echo "server did not start" >&2; cat "$out" >&2; exit 1; fi

url="http://127.0.0.1:$port/web/moe-test.html?model=/$model_rel&rows=$rows"
echo "    $url"

prof="$(mktemp -d /tmp/moepoc-XXXXXX)"
chromium-browser --headless=new --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --no-zygote --no-first-run \
  --no-default-browser-check --enable-precise-memory-info \
  --user-data-dir="$prof" "$url" >/tmp/moe-wasm-chromium.log 2>&1 &
chromium_pid=$!

# Peak RSS of the whole browser process tree, identified by the throw-away --user-data-dir
# token (the launcher is a wrapper that exits immediately). NOTE: this is a flat SUM of RSS
# over matching processes; T1 (vocoder POC) uses a BFS tree walk, so the two RSS numbers are
# NOT comparable with each other (shared pages are counted differently).
python3 - "$prof" "$rssfile" 300 <<"PY" &
import subprocess, sys, time, os
prof, path, secs = sys.argv[1], sys.argv[2], int(sys.argv[3])
peak = 0; end = time.time() + secs; seen = False; gone = 0
def dump():
    tmp = path + ".tmp"
    with open(tmp, "w") as fh: fh.write(str(peak))
    os.replace(tmp, path)
dump()
while time.time() < end:
    try:
        rows = subprocess.run(["ps", "-ww", "-eo", "pid=,rss=,args="], capture_output=True, text=True).stdout.splitlines()
    except Exception:
        break
    total = 0; found = 0
    for line in rows:
        parts = line.split(None, 2)
        if len(parts) < 3: continue
        if prof in parts[2]:
            try: total += int(parts[1])
            except ValueError: pass
            found += 1
    if found:
        seen = True; gone = 0
        if total > peak: peak = total; dump()
    elif seen:
        gone += 1
        if gone > 25: break
    time.sleep(0.25)
dump()
PY
sampler_pid=$!

echo "==> 4/5 waiting for the page result (real wall-clock timing, no virtual time)"
for _ in $(seq 1 600); do
  if grep -qE "^RESULT " "$out" 2>/dev/null; then break; fi
  if grep -qE "^TIMEOUT" "$out" 2>/dev/null; then break; fi
  sleep 0.5
done
sleep 1
kill "$sampler_pid" 2>/dev/null || true; sampler_pid=""
kill "$chromium_pid" 2>/dev/null || true; chromium_pid=""
pkill -f -- "--user-data-dir=$prof" 2>/dev/null || true
sleep 0.3

grep -E "^RESULT " "$out" | tail -1 | sed "s/^RESULT //" > "$json" || true
if [ ! -s "$json" ]; then
  echo "==> FAILED: no result POSTed by the page" >&2
  tail -40 "$out" >&2
  echo "--- chromium log ---" >&2; tail -20 /tmp/moe-wasm-chromium.log >&2
  exit 1
fi

rss_peak=0
if [ -s "$rssfile" ]; then rss_peak=$(cat "$rssfile"); fi
case "$rss_peak" in ""|*[!0-9]*) rss_peak=0 ;; esac
need_sites=$(grep -cE "^[[:space:]]*need\(" "$0")   # call sites only (the definition is "def need(")

echo "==> 5/5 assertions"
python3 - "$json" "$ref" "$model_sha_want" "$model_rel" "$wasm_bytes" "$wasm_sha" "$rss_peak" \
  "$imp_cargo" "$imp_glue" "$mpkfacts" "$need_sites" <<"PY"
import json, sys
jpath, rpath, sha_want, model_rel, wasm_bytes, wasm_sha, rss_kb, imp_cargo_p, imp_glue_p, mpk_p, need_sites = sys.argv[1:12]
R = json.load(open(jpath)); N = json.load(open(rpath))
IC = json.load(open(imp_cargo_p)); IG = json.load(open(imp_glue_p)); M = json.load(open(mpk_p))
fails = []
def need(cond, msg):
    if not cond: fails.append(msg)

# ---- G1: the page result is intact ----------------------------------------------
need(R.get("verdict") == "PASS", "page verdict = %r (%s)" % (R.get("verdict"), R.get("error")))
need(R.get("modelSha256") == sha_want, "model sha256 page %s vs disk %s" % (R.get("modelSha256"), sha_want))
need(R.get("modelBytes") == N.get("modelBytes"), "model bytes page %r vs native %r" % (R.get("modelBytes"), N.get("modelBytes")))

# ---- G2: the wasm instance really holds the checkpoint weights -------------------
# leg (a): no WASI / filesystem / env import - only wasm-bindgen JS shims
for tag, I in (("cargo-built", IC), ("wasm-bindgen", IG)):
    bad_mod = [m for m in I["modules"] if any(b in m.lower() for b in ("wasi", "fs", "env"))]
    bad_name = [i["name"] for i in I["imports"] if not (i["name"].startswith("__wbg_") or i["name"].startswith("__wbindgen_"))]
    need(I["importCount"] > 0, "%s wasm has no imports at all" % tag)
    need(not bad_mod, "%s wasm imports a non-bindgen module: %r" % (tag, bad_mod))
    need(not bad_name, "%s wasm imports a non-bindgen symbol: %r" % (tag, bad_name))
# leg (b): load_checkpoint materialised exactly the checkpoint parameter elements in wasm
need(R.get("paramCount", 0) > 0, "wasm paramCount %r" % R.get("paramCount"))
need(R.get("paramCount") == M["params"], "wasm paramCount %r != independent .mpk element count %r" % (R.get("paramCount"), M["params"]))
need(N.get("paramCount") == M["params"], "native paramCount %r != independent .mpk element count %r" % (N.get("paramCount"), M["params"]))
need(R.get("memBytesAfterInit", 0) > 0, "wasm linear memory unreadable before load")
need(R.get("memBytesAfterLoad", 0) > R.get("memBytesAfterInit", 0), "wasm linear memory did not grow on load")
need(R.get("memBytesAfterLoad", 0) >= R.get("modelBytes", 0), "wasm memory (%r) < checkpoint (%r) after load" % (R.get("memBytesAfterLoad"), R.get("modelBytes")))

# ---- G3: timings were recorded ---------------------------------------------------
need(R.get("loadMs", -1) > 0, "load time %r" % R.get("loadMs"))
need(R.get("instantiateMs", -1) >= 0, "instantiate time %r" % R.get("instantiateMs"))

# ---- G4: every forward pass produced a real mel ----------------------------------
for r in R.get("runs", []):
    need(r["n"] == r["rows"] * 128, "rows %d: mel len %d != rows*128" % (r["rows"], r["n"]))
    need(r["nonFinite"] == 0, "rows %d %s: nonFinite %d" % (r["rows"], r["label"], r["nonFinite"]))
    need(r["distinct"] > 1, "rows %d %s: constant output" % (r["rows"], r["label"]))
    need(r["rms"] > 0, "rows %d %s: zero rms" % (r["rows"], r["label"]))
    need(r["ms"] > 0, "rows %d %s: forward time %r" % (r["rows"], r["label"], r["ms"]))

# ---- G5: deterministic -----------------------------------------------------------
for d in R.get("determinism", []):
    need(d["maxDiff"] == 0, "rows %d: repeated run differs (maxDiff %r)" % (d["rows"], d["maxDiff"]))

# ---- G6: the output depends on the input (observed ~1.59) ------------------------
for s in R.get("sensitivity", []):
    need(s["maxDiff"] > 1e-2, "rows %d: output ignores the input (maxDiff %r)" % (s["rows"], s["maxDiff"]))

# ---- G7: wasm and native agree on VALUES -----------------------------------------
# Strict thresholds: the observed agreement is ~1e-8 (relative) and ~1.2e-7 (absolute on
# the first 8 mel values), so 1e-6 / 1e-5 leaves a small margin and still catches drift.
ncases = {c["rows"]: c for c in N.get("cases", [])}
compared = []
for r in R.get("runs", []):
    if r["label"] != "A": continue
    c = ncases.get(r["rows"])
    need(c is not None, "no native reference case for rows %d" % r["rows"])
    if c is None: continue
    rel_mean = abs(r["mean"] - c["mean"]) / max(abs(c["mean"]), 1e-12)
    rel_rms = abs(r["rms"] - c["rms"]) / max(abs(c["rms"]), 1e-12)
    d8 = max(abs(a - b) for a, b in zip(r["first8"], c["first8"]))
    need(rel_mean < 1e-6, "rows %d: wasm mean %.8f vs native %.8f (rel %.2e)" % (r["rows"], r["mean"], c["mean"], rel_mean))
    need(rel_rms < 1e-6, "rows %d: wasm rms %.8f vs native %.8f (rel %.2e)" % (r["rows"], r["rms"], c["rms"], rel_rms))
    need(d8 < 1e-5, "rows %d: max|wasm-native| over first 8 mel values = %.3e" % (r["rows"], d8))
    compared.append((r["rows"], r["ms"], c["forwardMs"], rel_mean, rel_rms, d8))

print("")
print("================== MoE-wasm POC — measured numbers ==================")
print("TIMING NOTE      : absolute ms below are INDICATIVE, not a benchmark.")
print("                   wasm and native are different processes/rounds; the wasm side")
print("                   also includes JS<->wasm marshalling. Only VALUES are exact.")
print("checkpoint       : %s (%d bytes, sha256 %s…)" % (model_rel, R["modelBytes"], R["modelSha256"][:16]))
print("independent count: %d tensors / %d parameter elements (msgpack decode of the .mpk)" % (M["tensors"], M["params"]))
print("wasm module      : %s bytes (sha256 %s…) · imports %d cargo / %d glue (modules %s)"
      % (wasm_bytes, wasm_sha[:16], IC["importCount"], IG["importCount"], ",".join(IC["modules"])))
print("import glue      : %s ms" % R.get("importMs"))
print("wasm instantiate : %s ms" % R.get("instantiateMs"))
print("checkpoint fetch : %s ms" % R.get("fetchMs"))
print("load inside wasm : %s ms   <- NamedMpkBytesRecorder + tensor build, in wasm" % R.get("loadMs"))
print("-" * 70)
for r in R.get("runs", []):
    spread = ""
    if r.get("msRepeat") is not None and r["ms"]:
        spread = " · repeat %.1f ms (%.2fx)" % (r["msRepeat"], r["msRepeat"] / r["ms"])
    print("wasm rows=%-5d %s  %8.3f ms  mel[%d]  min %-12.6f max %-12.6f mean %-12.8f rms %-12.8f nonFinite %d distinct %d%s"
          % (r["rows"], r["label"], r["ms"], r["n"], r["min"], r["max"], r["mean"], r["rms"], r["nonFinite"], r["distinct"], spread))
for rows, ms_wasm, ms_native, rm, rr, d8 in compared:
    print("native rows=%-5d A  %8.3f ms  vs wasm %8.3f ms (indicative only) |  rel mean %.2e · rel rms %.2e · max|diff| first8 %.2e"
          % (rows, ms_native, ms_wasm, rm, rr, d8))
print("-" * 70)
print("wasm weights     : paramCount %s (wasm) == %s (native) == %s (independent .mpk count)"
      % (R.get("paramCount"), N.get("paramCount"), M["params"]))
print("                   imports %d/%d are wasm-bindgen shims only (no WASI, no filesystem)" % (IC["importCount"], IG["importCount"]))
print("determinism      : %s" % ", ".join("rows %d -> 0" % d["rows"] for d in R.get("determinism", [])))
print("input sensitivity: %s" % ", ".join("rows %d -> %.6g" % (s["rows"], s["maxDiff"]) for s in R.get("sensitivity", [])))
print("-" * 70)
print("wasm linear mem  : init %s B -> load %s B -> forward %s B (delta +%s B on load, +%s B during forward)"
      % (R.get("memBytesAfterInit"), R.get("memBytesAfterLoad"), R.get("memBytesAfterForward"),
         R.get("memBytesAfterLoad", 0) - R.get("memBytesAfterInit", 0),
         R.get("memBytesAfterForward", 0) - R.get("memBytesAfterLoad", 0)))
print("js heap          : %s MB used / %s MB limit" % (R.get("jsHeapMB"), R.get("jsHeapLimitMB")))
print("peak RSS tree    : %.1f MB (flat sum over the Chromium tree; baseline about:blank ~1078 MB,"
      % (float(rss_kb) / 1024.0))
print("                   and T1 measures RSS with a BFS tree walk) => browser overhead, not wasm RAM")
print("total page time  : %s ms" % R.get("totalMs"))
print("asserts          : %s need() call sites in 7 groups (G1..G7), none decorative" % need_sites)
print("=====================================================================")
if fails:
    for f in fails: print("FAIL: " + f)
    sys.exit(1)
print("PASS: a real checkpoint was loaded and executed by burn/ndarray INSIDE wasm in the browser;")
print("      wasm and native agree to rel < 1e-6 / max|diff| < 1e-5 on the same input.")
PY
echo "==> PASS"