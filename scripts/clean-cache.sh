#!/usr/bin/env bash
# clean-cache.sh — ล้าง build cache ของ android-voice-synth
#
# ลบเฉพาะสิ่งที่ "สร้างใหม่ได้" (build cache) — ไม่แตะ source, dataset,
# voicebank, model หรือ asset ของแอป
#
# ค่า default คือ dry-run: ต้องเติม --apply จึงจะลบจริง
#
# ตัวอย่าง:
#   scripts/clean-cache.sh                                # ดูขนาด (ไม่ลบ)
#   scripts/clean-cache.sh --all --apply                  # ล้าง cache ทุกชนิด
#   scripts/clean-cache.sh --rust --apply                 # เฉพาะ Rust target
#   scripts/clean-cache.sh --auto --threshold 20G --apply # สำหรับ timer/cron
#   scripts/clean-cache.sh --poc --apply                  # output กลางทางใน test/poc
#
# ดู docs/DISK-HYGIENE.md สำหรับภาพรวม
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DO_RUST=0; DO_FLUTTER=0; DO_DOTNET=0; DO_POC=0
APPLY=0; AUTO=0; QUIET=0; THRESHOLD="20G"
ORIG_ARGS=("$@")

usage() { sed -n '2,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --rust)    DO_RUST=1 ;;
    --flutter) DO_FLUTTER=1 ;;
    --dotnet)  DO_DOTNET=1 ;;
    --all)     DO_RUST=1; DO_FLUTTER=1; DO_DOTNET=1 ;;
    --poc)     DO_POC=1 ;;
    --auto)    AUTO=1; DO_RUST=1; DO_FLUTTER=1; DO_DOTNET=1 ;;
    --threshold) THRESHOLD="${2:?--threshold ต้องมีค่า เช่น 20G}"; shift ;;
    --apply)   APPLY=1 ;;
    --quiet)   QUIET=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "ไม่รู้จัก option: $1  (ลอง --help)" >&2; exit 2 ;;
  esac
  shift
done

log() { [[ $QUIET == 1 ]] || echo "$@"; }

# ใช้ขนาดที่จองบนดิสก์จริง ไม่ใช่ apparent size — บน exfat (cluster 128K)
# สองค่านี้ต่างกันได้มาก โปรเจกต์นี้อยู่บน exfat จึงต้องใช้ค่าที่จองจริง
# du อาจคืน exit != 0 (มี entry ค้างใน target/debug/deps) แต่ยังให้ยอดรวมที่ถูกต้อง
# จึงห้ามต่อท้ายด้วย `|| echo 0` ไม่งั้นจะได้ค่าซ้อนกันสองบรรทัด
size_of() {
  local out
  out="$(du -s --block-size=1 "$1" 2>/dev/null | cut -f1)" || true
  printf '%s' "${out:-0}"
}
human()   { numfmt --to=iec --suffix=B "${1:-0}" 2>/dev/null || echo "${1:-0}B"; }

if ! THRESHOLD_B="$(numfmt --from=iec "$THRESHOLD" 2>/dev/null)"; then
  echo "ค่า --threshold ไม่ถูกต้อง: $THRESHOLD (ตัวอย่างที่ถูก: 20G)" >&2
  exit 2
fi

# ── รายการเป้าหมาย ───────────────────────────────────────────────────────
# หมายเหตุ: native/build และ native/third_party/world/build "ไม่" ถูกลบ
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

safe_rm() {
  local p="$1"
  if [[ "$p" != "$ROOT"/* || "$p" == "$ROOT" ]]; then
    echo "ปฏิเสธการลบนอกโปรเจกต์: $p" >&2
    return 1
  fi
  rm -rf -- "$p" 2>/dev/null || true
  # บน exfat rm มักล้มเหลวกลางทางเมื่อมี entry ค้างใน deps/ (stat ไม่ได้)
  # ทำให้โฟลเดอร์แม่ลบไม่ได้ทั้งที่ข้อมูลถูกปล่อยไปแล้ว — ไล่ลบทีละ entry แล้วลองใหม่
  if [[ -e "$p" ]]; then
    find "$p" -depth -delete 2>/dev/null || true
    rm -rf -- "$p" 2>/dev/null || true
  fi
  if [[ -e "$p" ]]; then
    echo "  ⚠ ลบไม่หมด: ${p#$ROOT/} (เหลือ $(find "$p" 2>/dev/null | wc -l) entries) — รันสคริปต์ซ้ำอีกครั้ง" >&2
  fi
  return 0
}

# ── เก็บรายการที่เลือก ───────────────────────────────────────────────────
declare -a PATHS=() LABELS=()
add_cat() {
  local label="$1" tmp=() p
  shift
  declare -a tmp=()
  "$@" tmp
  for p in "${tmp[@]:-}"; do
    [[ -n "$p" ]] || continue
    PATHS+=("$p"); LABELS+=("$label")
  done
  return 0
}

# ใช้ if/then แทน `[[ ]] && add_cat` — แบบหลังจะคืน 1 เมื่อ flag ปิด
# แล้ว set -e จะฆ่าสคริปต์ทันที
if [[ $DO_RUST    == 1 ]]; then add_cat rust    make_list_rust;    fi
if [[ $DO_FLUTTER == 1 ]]; then add_cat flutter make_list_flutter; fi
if [[ $DO_DOTNET  == 1 ]]; then add_cat dotnet  make_list_dotnet;  fi
if [[ $DO_POC     == 1 ]]; then add_cat poc     make_list_poc;     fi

if [[ ${#PATHS[@]} -eq 0 ]]; then
  log "ไม่มีอะไรให้ลบ (ไม่พบ cache ที่เลือก — อาจล้างไปแล้ว)"
  exit 0
fi

# ── วัดขนาด ─────────────────────────────────────────────────────────────
TOTAL=0
log ""
log "Build cache ที่พบ (root: $ROOT)"
log "───────────────────────────────────────────────────────────────"
for i in "${!PATHS[@]}"; do
  b="$(size_of "${PATHS[$i]}")"
  TOTAL=$((TOTAL + b))
  printf "  %-8s %10s  %s\n" "${LABELS[$i]}" "$(human "$b")" "${PATHS[$i]#$ROOT/}"
done
log "───────────────────────────────────────────────────────────────"
printf "  %-8s %10s  %s\n" "TOTAL" "$(human "$TOTAL")" "${#PATHS[@]} รายการ"
log ""

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
for i in "${!PATHS[@]}"; do
  b="$(size_of "${PATHS[$i]}")"
  if safe_rm "${PATHS[$i]}"; then
    FREED=$((FREED + b))
    log "  ลบแล้ว  $(human "$b")  ${PATHS[$i]#$ROOT/}"
  fi
done

log ""
log "คืนพื้นที่: $(human "$FREED")"
log "พื้นที่ว่างตอนนี้: $(df -h "$ROOT" | awk 'NR==2 {print $4" (ใช้ไป "$5")"}')"
