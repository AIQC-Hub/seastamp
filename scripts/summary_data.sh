#!/usr/bin/env bash
#
# summary_data.sh: build one summary page (Markdown or HTML) per ctddump data
# product of the Arctic, Baltic, and Mediterranean seas, from the report TSVs
# that stamp_data.sh writes. One page per (region, product) stem, the same stems
# stamp_data.sh uses (nrt_ar_ar, nrt_ar_gl, cora_ar, and so on).
#
# Each page is `seastamp report summary <stem>`: an Overview of rows and unique
# locations, then a section per module the file was stamped with (distance to
# coast, depth, sea area, place, nearest location). It reads only the small
# TSVs under <report>/stamp/, never the stamped data, so it is instant.
#
# Usage:
#   scripts/summary_data.sh [options] [region ...]
#
# Regions:  arctic  baltic  mediterranean   (default: all three; "all" also works)
#
# Options (may appear anywhere on the command line):
#   -r, --report DIR   root of the report tree; pages are built from
#                      <report>/stamp/<stem>.tsv              (default: report)
#   -d, --dest DIR     directory for the generated pages      (default: summary)
#   -f, --format FMT   page format: md or html                (default: md)
#   --top N            most values listed per name table (sea area, country,
#                      ...); the rest are summed in one row   (default: 20)
#   --bin PATH         seastamp binary (default: $SEASTAMP_BIN, else the one on
#                      PATH, else ./target/release or ./target/debug)
#   -y, --yes          skip the confirmation prompt and start immediately
#   -h, --help         show this help
#
# Each page gets a human-readable title and any product-specific notes; both are
# defined in the "Page text" section below and are the place to edit what a page
# says about a region or product. The section prose itself is generic and comes
# from seastamp.
#
# A stem with no report is skipped (for example the unpublished Baltic GL), not
# an error. Run stamp_data.sh first. summary_site.sh builds a web site from the
# Markdown pages.

set -euo pipefail

usage() { awk 'NR<3 {next} /^#/ {sub(/^# ?/, ""); print; next} {exit}' "$0"; }

# ---- Configuration (defaults; override with the options below) -----------
REPORT=report
DEST=summary
FORMAT=md
TOP=20
BIN="${SEASTAMP_BIN:-}"
ASSUME_YES=0

# ---- Parse options -------------------------------------------------------
# Options may appear anywhere; the remaining words are the regions.
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -r|--report)  REPORT="${2:?--report requires a directory}"; shift 2 ;;
    --report=*)   REPORT="${1#*=}"; shift ;;
    -d|--dest)    DEST="${2:?--dest requires a directory}"; shift 2 ;;
    --dest=*)     DEST="${1#*=}"; shift ;;
    -f|--format)  FORMAT="${2:?--format requires md or html}"; shift 2 ;;
    --format=*)   FORMAT="${1#*=}"; shift ;;
    --top)        TOP="${2:?--top requires a number}"; shift 2 ;;
    --top=*)      TOP="${1#*=}"; shift ;;
    --bin)        BIN="${2:?--bin requires a path}"; shift 2 ;;
    --bin=*)      BIN="${1#*=}"; shift ;;
    -y|--yes)     ASSUME_YES=1; shift ;;
    -h|--help)    usage; exit 0 ;;
    --)           shift; ARGS+=("$@"); break ;;
    -*)           echo "Unknown option: $1" >&2; usage; exit 1 ;;
    *)            ARGS+=("$1"); shift ;;
  esac
done

case "$FORMAT" in
  md|html) ;;
  *) echo "Unknown format: $FORMAT (use md or html)" >&2; usage; exit 1 ;;
esac

REGIONS=(arctic baltic mediterranean)

# The (region, product) stems, as in stamp_data.sh and ctddump's scripts.
stems_for() {  # <region>
  case "$1" in
    arctic)        echo nrt_ar_ar nrt_ar_gl cora_ar ;;
    baltic)        echo nrt_bo_bo nrt_bo_gl cora_bo ;;
    mediterranean) echo nrt_mo_mo nrt_mo_gl cora_mo ;;
  esac
}

# ---- Page text -----------------------------------------------------------
# The page's section prose is generic and lives in seastamp. The title and the
# notes below are the region- and product-specific bits: edit them here. The
# titles match ctddump's summary pages for the same products.

# Human-readable page title, replacing the default "Summary: <stem>".
title_for() {  # <stem>
  case "$1" in
    nrt_ar_ar) echo "Arctic Ocean: Near Real Time, regional product (AR)" ;;
    nrt_ar_gl) echo "Arctic Ocean: Near Real Time, global product (GL)" ;;
    cora_ar)   echo "Arctic Ocean: CORA reanalysis" ;;
    nrt_bo_bo) echo "Baltic Sea: Near Real Time, regional product (BO)" ;;
    nrt_bo_gl) echo "Baltic Sea: Near Real Time, global product (GL)" ;;
    cora_bo)   echo "Baltic Sea: CORA reanalysis" ;;
    nrt_mo_mo) echo "Mediterranean Sea: Near Real Time, regional product (MO)" ;;
    nrt_mo_gl) echo "Mediterranean Sea: Near Real Time, global product (GL)" ;;
    cora_mo)   echo "Mediterranean Sea: CORA reanalysis" ;;
    *)         echo "Summary: $1" ;;
  esac
}

# Notes shown under the page title, one per line (blank lines are ignored). Each
# becomes a `--note`. Put anything product- or run-specific here, such as which
# reference data a product was stamped against.
notes_for() {  # <stem>
  # The *_gl pattern must precede the general nrt_* one.
  case "$1" in
    nrt_*_gl) echo "Global (GL) Near Real Time product restricted to this region's bounding box, after ctddump's cleaning and de-duplication." ;;
    nrt_*)    echo "Regional Near Real Time product, after ctddump's cleaning and de-duplication." ;;
    cora_*)   echo "CORA delayed-mode reanalysis, after ctddump's cleaning and de-duplication." ;;
  esac
}

# ---- Logging -------------------------------------------------------------
# Announce each step (timestamped, to stderr) so progress is visible. The current
# region tags its lines "[region]".
log() {
  local p=""
  [[ -n "${REGION:-}" ]] && p="[$REGION] "
  printf '[%s] %s%s\n' "$(date '+%H:%M:%S')" "$p" "$*" >&2
}

# Print the resolved configuration, then ask for confirmation unless -y/--yes was
# given. In a non-interactive shell without -y there is nothing to read, so abort
# with a hint rather than hang.
show_config() {  # <region...>
  local -a rs=("$@")
  local n=0 r s
  for r in "${rs[@]}"; do
    for s in $(stems_for "$r"); do [[ -f "$REPORT/stamp/$s.tsv" ]] && n=$((n + 1)); done
  done
  {
    echo "Configuration:"
    echo "  regions : ${rs[*]}"
    echo "  reports : $n found in $REPORT/stamp"
    echo "  dest    : $DEST/<stem>.$FORMAT"
    echo "  top     : $TOP"
    echo "  binary  : $BIN"
    echo "Run with -h/--help to see all options."
  } >&2
}

confirm() {
  [[ "$ASSUME_YES" == 1 ]] && return 0
  if [[ ! -t 0 ]]; then
    log "non-interactive shell: pass -y/--yes to proceed without confirmation."
    return 1
  fi
  local reply
  read -r -p "Proceed? [y/N] " reply
  [[ "$reply" == [yY] || "$reply" == [yY][eE][sS] ]]
}

# ---- Resolve the seastamp binary ----------------------------------------
# --bin / $SEASTAMP_BIN if given; else seastamp on PATH; else a build in this
# repo (release preferred over debug). The same lookup as stamp.sh.
resolve_bin() {
  if [[ -n "$BIN" ]]; then
    command -v "$BIN" >/dev/null 2>&1 || [[ -x "$BIN" ]] || {
      echo "seastamp binary not found: $BIN" >&2; return 1; }
    return 0
  fi
  if command -v seastamp >/dev/null 2>&1; then BIN=seastamp; return 0; fi
  local root; root="$(cd "$(dirname "$0")/.." && pwd)"
  if   [[ -x "$root/target/release/seastamp" ]]; then BIN="$root/target/release/seastamp"
  elif [[ -x "$root/target/debug/seastamp"   ]]; then BIN="$root/target/debug/seastamp"
  else
    echo "seastamp not found: build it (cargo build --release) or pass --bin" >&2
    return 1
  fi
}

# ---- Per-stem step -------------------------------------------------------

do_summary() {  # <stem>
  local s="$1"
  if [[ ! -f "$REPORT/stamp/$s.tsv" ]]; then log "skip $s (no report)"; return 0; fi
  mkdir -p "$DEST"; log "summary $s -> $DEST/$s.$FORMAT"

  local -a nargs=()
  local n
  while IFS= read -r n; do
    [[ -n "$n" ]] && nargs+=(--note "$n")
  done < <(notes_for "$s")

  "$BIN" report summary "$s" --report-dir "$REPORT" --format "$FORMAT" \
    --top "$TOP" --title "$(title_for "$s")" \
    ${nargs[@]+"${nargs[@]}"} -o "$DEST/$s.$FORMAT"
}

# ---- Dispatch ------------------------------------------------------------
is_region() {
  local r
  for r in "${REGIONS[@]}"; do [[ "$r" == "$1" ]] && return 0; done
  return 1
}

main() {
  [[ "${1:-}" == "help" ]] && { usage; return 0; }

  # Remaining args are regions; default to all, and "all" is an alias.
  local -a regions=("$@")
  if [[ ${#regions[@]} -eq 0 || "${regions[0]}" == "all" ]]; then
    regions=("${REGIONS[@]}")
  fi
  local r s
  for r in "${regions[@]}"; do
    is_region "$r" || { echo "Unknown region: $r" >&2; usage; return 1; }
  done
  resolve_bin || return 1

  show_config "${regions[@]}"
  confirm || { log "aborted."; return 1; }

  # Summaries only read small TSVs, so a plain sequential pass is plenty fast.
  for r in "${regions[@]}"; do
    REGION="$r"
    log "===== summary: $r ====="
    for s in $(stems_for "$r"); do do_summary "$s"; done
  done
  REGION=""
  log "done."
}

main ${ARGS[@]+"${ARGS[@]}"}
