#!/usr/bin/env bash
# clean-cache.sh — ล้าง build cache ของ android-voice-synth
#
# ลบเฉพาะสิ่งที่ "สร้างใหม่ได้" (build cache) — ไม่แตะ source, dataset,
# voicebank, model หรือ asset ของแอป
#
# ค่า default คือ dry-run: ต้องเติม --apply จึงจะลบจริง
#
# ตัวอย่าง:
#   scripts/clean-cache.sh                                 # ดูขนาด (ไม่ลบ)
#   scripts/clean-cache.sh --all --apply                   # ล้าง cache ทุกชนิด
#   scripts/clean-cache.sh --rust --apply                  # เฉพาะ Rust target
#   scripts/clean-cache.sh --stale --apply                 # artifact เก่ากว่า 7 วัน
#   scripts/clean-cache.sh --stale --stale-days 14 --apply # เก่ากว่า 14 วัน
#   scripts/clean-cache.sh --orphan --apply                # target/cargo ของ agent ที่ค้าง
#   scripts/clean-cache.sh --auto --threshold 20G --apply  # สำหรับ timer/cron
#   scripts/clean-cache.sh --poc --apply                   # output กลางทางใน test/poc
#
# โหมดที่เพิ่ม 2026-09-23 (ปิดรูรั่วที่ทำให้ cache บาน):
#   --stale [--stale-days N]    ลบ artifact เก่ากว่า N วันใน target/debug (+ wasm32)
#                               ของที่ build ล่าสุดยังอยู่ครบ
#   --orphan [--orphan-days N]  ลบ .<name>-target / .<name>-cargo* ที่ session ของ
#                               agent ละทิ้งไว้ใน workspace root (เคสนี้กินไป 21 GB)
#   --include-release           ให้ --stale ครอบ target/release ด้วย (ปิดไว้โดย
#                               default เพราะ [profile.release] lto = true ทำให้
#                               บิลด์ release ใหม่แพงมาก)
#
# ความปลอดภัย:
#   * --orphan แตะเฉพาะ dot-dir ที่ชื่อลงท้าย -target / -cargo / -cargo-home
#     ใต้ workspace root และข้ามโฟลเดอร์ที่มีไฟล์ใหม่กว่า cutoff
#     (ถือว่าอาจมี session ใช้งานอยู่)
#   * ทุกโหมดยกเว้น path ที่ห้ามลบ: native/build, native/third_party/world/build,
#     test/golden, app/android/app/src/main/assets
#   * ไม่แตะ symlink
#
# ดู docs/DISK-HYGIENE.md สำหรับภาพรวม
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# รากของ workspace ที่ session ของ agent วาง .<name>-target / .<name>-cargo ไว้
WORKSPACE="${AVS_WORKSPACE:-$(cd "$ROOT/.." && pwd)}"

DO_RUST=0; DO_FLUTTER=0; DO_DOTNET=0; DO_POC=0; DO_STALE=0; DO_ORPHAN=0
INCLUDE_RELEASE=0
STALE_DAYS=7
ORPHAN_DAYS=1
APPLY=0; AUTO=0; QUIET=0; THRESHOLD="20G"
ORIG_ARGS=("$@")

usage() { awk 'NR>1 && /^#/ {sub(/^# ?/,""); print; next} NR>1 {exit}' "${BASH_SOURCE[0]}"; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --rust)    DO_RUST=1 ;;
    --flutter) DO_FLUTTER=1 ;;
    --dotnet)  DO_DOTNET=1 ;;
    --poc)     DO_POC=1 ;;
    --stale)   DO_STALE=1 ;;
    --orphan)  DO_ORPHAN=1 ;;
    --include-release) INCLUDE_RELEASE=1 ;;
    --all)     DO_RUST=1; DO_FLUTTER=1; DO_DOTNET=1; DO_STALE=1; DO_ORPHAN=1 ;;
    --auto)    AUTO=1; DO_RUST=1; DO_FLUTTER=1; DO_DOTNET=1; DO_STALE=1; DO_ORPHAN=1 ;;
    --stale-days)  STALE_DAYS="${2:?--stale-days ต้องมีค่า เช่น 7}"; shift ;;
    --orphan-days) ORPHAN_DAYS="${2:?--orphan-days ต้องมีค่า เช่น 1}"; shift ;;
    --threshold)   THRESHOLD="${2:?--threshold ต้องมีค่า เช่น 20G}"; shift ;;
    --apply)   APPLY=1 ;;
    --quiet)   QUIET=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "ไม่รู้จัก option: $1  (ลอง --help)" >&2; exit 2 ;;
  esac
  shift
done

log() { [[ $QUIET == 1 ]] || echo "$@"; }

# ── เวลา: ต้องเป็น epoch เท่านั้น ───────────────────────────────────────
# ห้ามใช้ date string กับ find -newermt บนเครื่องนี้: locale เป็น th_TH.UTF-8
# ทำให้ date พ่นปี พ.ศ. (เช่น 2569) แต่ find ตีความตัวเลขนั้นเป็น ค.ศ.
# => cutoff ไปอยู่ปี 2569 จริง ๆ => ทุกไฟล์กลายเป็น "เก่ากว่า" และถูกลบทั้งก้อน
# เกิดจริง 2026-09-23: target/debug หายไป 40 GB แทนที่จะเป็น 30 GB
cutoff_epoch() { echo $(( $(date +%s) - $1 * 86400 )); }

# ใช้ขนาดที่จองบนดิสก์จริง ไม่ใช่ apparent size
# du อาจคืน exit != 0 (มี entry ค้างใน target/debug/deps) แต่ยังให้ยอดรวมที่ถูกต้อง
# จึงห้ามต่อท้ายด้วย '|| echo 0' ไม่งั้นจะได้ค่าซ้อนกันสองบรรทัด
size_of() {
  local out
  out="$(du -s --block-size=1 "$1" 2>/dev/null | cut -f1)" || true
  out="${out:-0}"
  # ntfs3 ไม่รายงาน block ให้ไฟล์เล็ก ๆ (ได้ 0 ทั้งที่มีของอยู่) — ถอยไปใช้ apparent size
  if [[ "$out" == "0" ]]; then
    out="$(du -s --apparent-size --block-size=1 "$1" 2>/dev/null | cut -f1)" || true
    out="${out:-0}"
  fi
  printf '%s' "$out"
}
human()   { numfmt --to=iec --suffix=B "${1:-0}" 2>/dev/null || echo "${1:-0}B"; }

# นับเฉพาะไฟล์ที่เก่ากว่า cutoff (epoch) — ปลอดภัยจากปัญหา locale
bytes_older_than() { # $1=dir $2=ref_epoch
  { find "$1" -type f ! -newermt "@$2" -printf '%s\n' 2>/dev/null || true; } \
    | awk '{s+=$1} END {print s+0}'
}
count_older_than() { # $1=dir $2=ref_epoch
  { find "$1" -type f ! -newermt "@$2" -printf 'x\n' 2>/dev/null || true; } \
    | awk 'END {print NR+0}'
}

if ! THRESHOLD_B="$(numfmt --from=iec "$THRESHOLD" 2>/dev/null)"; then
  echo "ค่า --threshold ไม่ถูกต้อง: $THRESHOLD (ตัวอย่างที่ถูก: 20G)" >&2
  exit 2
fi

# ── รายการเป้าหมาย ───────────────────────────────────────────────────────
# หมายเหตุ: native/build และ native/third_party/world/build ไม่ถูกลบ
# เพราะ libworldline.so / libmixerfx.so ที่ app ต้องใช้อยู่ที่นั่น
make_list_rust() {
  local -n out=$1
  [[ -d "$ROOT/native/target" ]] && out+=("$ROOT/native/target")
  [[ -d "$ROOT/native/build-debug" ]] && out+=("$ROOT/native/build-debug")
  local d
  for d in "$ROOT"/native/crates/*/target; do [[ -d "$d" ]] && out+=("$d"); done
  return 0   # จำเป็น: ถ้า glob ไม่ match ลูปจะคืน 1 แล้ว set -e จะฆ่าสคริปต์
}
make_list_flutter() {
  local -n out=$1
  local d
  for d in "$ROOT/app/build" "$ROOT/app/android/.gradle" "$ROOT/app/android/.kotlin"; do
    [[ -d "$d" ]] && out+=("$d")
  done
  return 0
}
make_list_dotnet() {
  local -n out=$1
  local d
  for d in "$ROOT/test/golden/golden-renderer/bin" "$ROOT/test/golden/golden-renderer/obj"; do
    [[ -d "$d" ]] && out+=("$d")
  done
  for d in "$ROOT"/"ref(openutau+openutau mobile)"/core/*/bin \
           "$ROOT"/"ref(openutau+openutau mobile)"/core/*/obj \
           "$ROOT"/ref-openutau-clean/core/*/bin \
           "$ROOT"/ref-openutau-clean/core/*/obj; do
    [[ -d "$d" ]] && out+=("$d")
  done
  return 0
}
# output กลางทางของงานเทรน/เรนเดอร์ — เก็บ dataset (*.jsonl, *.tsbin) และโมเดล (*.mpk) ไว้
make_list_poc() {
  local -n out=$1
  local d
  for d in "$ROOT/test/poc/renders" \
           "$ROOT/test/poc/eval/student-all" \
           "$ROOT/test/poc/eval/teacher-wavs" \
           "$ROOT"/test/poc/bigset/student-* \
           "$ROOT"/test/poc/bigset/*teacher-wavs \
           "$ROOT"/test/poc/bigset/worst-boost-wavs; do
    [[ -d "$d" ]] && out+=("$d")
  done
  return 0
}

# ── path ที่ห้ามลบเด็ดขาด ───────────────────────────────────────────────
DENY=(
  "$ROOT/native/build"
  "$ROOT/native/third_party/world/build"
  "$ROOT/test/golden"
  "$ROOT/app/android/app/src/main/assets"
)

safe_rm() {
  local p="$1" x
  if [[ "$p" == "$ROOT" || "$p" == "$WORKSPACE" ]]; then
    echo "ปฏิเสธการลบ root: $p" >&2; return 1
  fi
  case "$p" in
    "$ROOT"/*|"$WORKSPACE"/*) ;;
    *) echo "ปฏิเสธการลบนอกขอบเขต: $p" >&2; return 1 ;;
  esac
  if [[ -L "$p" ]]; then echo "ข้าม symlink: $p" >&2; return 1; fi
  for x in "${DENY[@]}"; do
    if [[ "$p" == "$x" || "$p" == "$x"/* ]]; then
      echo "ปฏิเสธ: $p อยู่ในรายการห้ามลบ ($x)" >&2; return 1
    fi
  done
  rm -rf -- "$p" 2>/dev/null || true
  # บน filesystem ที่มี entry ค้างใน deps/ (stat ไม่ได้) rm มักล้มกลางทาง
  # ไล่ลบทีละ entry แล้วลองใหม่
  if [[ -e "$p" ]]; then
    find "$p" -depth -delete 2>/dev/null || true
    rm -rf -- "$p" 2>/dev/null || true
  fi
  if [[ -e "$p" ]]; then
    echo "  ⚠ ลบไม่หมด: $p (เหลือ $(find "$p" 2>/dev/null | wc -l) entries) — รันสคริปต์ซ้ำอีกครั้ง" >&2
  fi
  return 0
}

# ── เก็บรายการที่เลือก ───────────────────────────────────────────────────
declare -a RM_PATHS=() RM_LABELS=()
declare -a STALE_DIRS=() STALE_REFS=()
declare -a ORPHAN_DIRS=()
declare -a SKIPPED=()

add_cat() {
  local label="$1" tmp=() p
  shift
  declare -a tmp=()
  "$@" tmp
  for p in "${tmp[@]:-}"; do
    [[ -n "$p" ]] || continue
    RM_PATHS+=("$p"); RM_LABELS+=("$label")
  done
  return 0
}

# ใช้ if/then แทน [[ ]] && add_cat — แบบหลังจะคืน 1 เมื่อ flag ปิด
# แล้ว set -e จะฆ่าสคริปต์ทันที
if [[ $DO_RUST    == 1 ]]; then add_cat rust    make_list_rust;    fi
if [[ $DO_FLUTTER == 1 ]]; then add_cat flutter make_list_flutter; fi
if [[ $DO_DOTNET  == 1 ]]; then add_cat dotnet  make_list_dotnet;  fi
if [[ $DO_POC     == 1 ]]; then add_cat poc     make_list_poc;     fi

if [[ $DO_STALE == 1 ]]; then
  SREF="$(cutoff_epoch "$STALE_DAYS")"
  for d in "$ROOT/native/target/debug" "$ROOT/native/target/wasm32-unknown-unknown"; do
    if [[ -d "$d" ]]; then STALE_DIRS+=("$d"); STALE_REFS+=("$SREF"); fi
  done
  if [[ $INCLUDE_RELEASE == 1 && -d "$ROOT/native/target/release" ]]; then
    STALE_DIRS+=("$ROOT/native/target/release"); STALE_REFS+=("$SREF")
  fi
fi

if [[ $DO_ORPHAN == 1 ]]; then
  OREF="$(cutoff_epoch "$ORPHAN_DAYS")"
  shopt -s nullglob
  for d in "$WORKSPACE"/.*-target "$WORKSPACE"/.*-cargo "$WORKSPACE"/.*-cargo-home; do
    [[ -d "$d" ]] || continue
    if [[ -L "$d" ]]; then SKIPPED+=("symlink: $d"); continue; fi
    if [[ -n "$(find "$d" -newermt "@$OREF" -print -quit 2>/dev/null || true)" ]]; then
      SKIPPED+=("$d (มีไฟล์ใหม่กว่า $ORPHAN_DAYS วัน — อาจมี session ใช้อยู่)")
      continue
    fi
    ORPHAN_DIRS+=("$d")
  done
  shopt -u nullglob
fi

if [[ "${#RM_PATHS[@]}" -eq 0 && "${#STALE_DIRS[@]}" -eq 0 && "${#ORPHAN_DIRS[@]}" -eq 0 ]]; then
  log "ไม่มีอะไรให้ลบ (ไม่พบ cache ที่เลือก — อาจล้างไปแล้ว)"
  exit 0
fi

# ── วัดขนาด ─────────────────────────────────────────────────────────────
TOTAL=0
log ""
log "Build cache ที่พบ (root: $ROOT)"
log "───────────────────────────────────────────────────────────────"
for i in "${!RM_PATHS[@]}"; do
  b="$(size_of "${RM_PATHS[$i]}")"
  TOTAL=$((TOTAL + b))
  printf "  %-8s %10s  %s\n" "${RM_LABELS[$i]}" "$(human "$b")" "${RM_PATHS[$i]#$ROOT/}"
done
for i in "${!STALE_DIRS[@]}"; do
  b="$(bytes_older_than "${STALE_DIRS[$i]}" "${STALE_REFS[$i]}")"
  n="$(count_older_than "${STALE_DIRS[$i]}" "${STALE_REFS[$i]}")"
  TOTAL=$((TOTAL + b))
  printf "  %-8s %10s  %s (เก่ากว่า %s วัน: %s ไฟล์)\n" \
    "stale" "$(human "$b")" "${STALE_DIRS[$i]#$ROOT/}" "$STALE_DAYS" "$n"
done
for i in "${!ORPHAN_DIRS[@]}"; do
  b="$(size_of "${ORPHAN_DIRS[$i]}")"
  TOTAL=$((TOTAL + b))
  printf "  %-8s %10s  %s\n" "orphan" "$(human "$b")" "${ORPHAN_DIRS[$i]#$WORKSPACE/}"
done
log "───────────────────────────────────────────────────────────────"
printf "  %-8s %10s  %s\n" "TOTAL" "$(human "$TOTAL")" \
  "$(( ${#RM_PATHS[@]} + ${#STALE_DIRS[@]} + ${#ORPHAN_DIRS[@]} )) รายการ"
log ""

if [[ "${#SKIPPED[@]}" -gt 0 ]]; then
  log "ข้าม (ไม่แตะ):"
  for s in "${SKIPPED[@]}"; do log "  • $s"; done
  log ""
fi


if [[ $AUTO == 1 && $TOTAL -le $THRESHOLD_B ]]; then
  log "--auto: cache $(human "$TOTAL") ยังไม่เกิน threshold $(human "$THRESHOLD_B") — ไม่ลบ"
  exit 0
fi
if [[ $AUTO == 1 ]]; then
  log "--auto: cache $(human "$TOTAL") เกิน threshold $(human "$THRESHOLD_B") → ล้าง"
fi

if [[ $APPLY != 1 ]]; then
  log "โหมด dry-run — ยังไม่ได้ลบอะไร"
  log "ลบจริง:  $(basename "$0") ${ORIG_ARGS[*]:-} --apply"
  exit 0
fi

# ── ลบจริง ──────────────────────────────────────────────────────────────
FREED=0
for i in "${!RM_PATHS[@]}"; do
  p="${RM_PATHS[$i]}"
  b="$(size_of "$p")"
  if safe_rm "$p"; then
    FREED=$((FREED + b))
    log "  ลบแล้ว  $(human "$b")  ${p#$ROOT/}"
  fi
done
for i in "${!STALE_DIRS[@]}"; do
  d="${STALE_DIRS[$i]}"; ref="${STALE_REFS[$i]}"
  b="$(bytes_older_than "$d" "$ref")"
  find "$d" -type f ! -newermt "@$ref" -delete 2>/dev/null || true
  find "$d" -type d -empty -delete 2>/dev/null || true
  FREED=$((FREED + b))
  log "  ลบแล้ว  $(human "$b")  ${d#$ROOT/} (artifact เก่ากว่า $STALE_DAYS วัน)"
done
for i in "${!ORPHAN_DIRS[@]}"; do
  p="${ORPHAN_DIRS[$i]}"
  b="$(size_of "$p")"
  if safe_rm "$p"; then
    FREED=$((FREED + b))
    log "  ลบแล้ว  $(human "$b")  ${p#$WORKSPACE/}"
  fi
done

log ""
log "คืนพื้นที่: $(human "$FREED")"
log "พื้นที่ว่างตอนนี้: $(df -h "$ROOT" | awk 'NR==2 {print $4" (ใช้ไป "$5")"}')"
