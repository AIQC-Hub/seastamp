#!/usr/bin/env bash
#
# stamp_data.sh: run stamp.sh over every ctddump data product of the Arctic,
# Baltic, and Mediterranean seas, one worker per product, all in parallel, and
# summarise each stamped file as a report TSV. Two steps run in order:
#
#   stamp     Stamp <in>/<stem>.parquet to <out>/<stem>.<format> with stamp.sh.
#   report    Summarise <out>/<stem>.<format> with `seastamp report stats` to
#             <report>/stamp/<stem>.tsv, which summary_data.sh turns into a page.
#
# The inputs are the de-duplicated Parquet files that ctddump's dedup_data.sh
# writes, one per (region, product) stem:
#   arctic         nrt_ar_ar  nrt_ar_gl  cora_ar
#   baltic         nrt_bo_bo  nrt_bo_gl  cora_bo
#   mediterranean  nrt_mo_mo  nrt_mo_gl  cora_mo
# Each <in>/<stem>.parquet is stamped to <out>/<stem>.<format>. A stem whose
# input is missing is skipped with a note, not an error, so an unpublished
# product (such as the Baltic GL) does not fail the run.
#
# Usage:
#   scripts/stamp_data.sh [options] [stamp.sh options] [command] [region ...]
#
# Commands:
#   stamp       Stamp the inputs.
#   report      Summarise stamped outputs that already exist (no stamping, so
#               no module flags are needed).
#   all         stamp, then report.  (default)
#   help        Show this help.
#
# Regions:  arctic  baltic  mediterranean   (default: all three; "all" also works)
#
# Options (may appear anywhere on the command line):
#   -i, --in DIR      directory holding the <stem>.parquet inputs
#                     (default: output/dedup/dedup, ctddump's final stage)
#   -o, --out DIR     directory for the stamped outputs  (default: output/stamp)
#   -r, --report DIR  root for the report TSVs, written to <report>/stamp/
#                     (default: report)
#   -f, --format FMT  output format, as a file extension: parquet, csv, tsv,
#                     csv.gz, or tsv.gz  (default: parquet)
#   -t, --threads N   worker threads per seastamp call   (default: 2). Every
#                     product runs at once, so the total is N times the number
#                     of products
#   --dist-bands LIST, --depth-bands LIST
#                     band edges for the report, as `seastamp report stats`
#                     takes them (default: 1,5,20,100 km and 10,50,200,1000 m)
#   --by-region       parallelise per region instead of per product (coarser:
#                     one worker per region, its products stamped in order)
#   --sequential      stamp one product at a time (no parallelism)
#   -n, --dry-run     print the commands without running them (no prompt)
#   -y, --yes         skip the confirmation prompt and start immediately
#   -h, --help        show this help
#
# The stamp.sh options are passed to it unchanged, so the module flags (--coast,
# --depth, --sea, --countries, --nearest), their per-module options, and
# --region / --partition / --lon-col / --lat-col / --decimals / --bin / --keep
# all work as they do there. Run scripts/stamp.sh --help for that list. They are
# checked once, against the first input, before any worker starts, so a bad
# combination fails the run up front rather than once per product. The report
# step reads the options that describe the stamped columns from the same flags
# (--coast-unit, --place-unit, --nearest-unit, --depth-positive, --lon-col,
# --lat-col, --decimals), so give them to `report` exactly as they were given to
# `stamp`.
#
# Each worker is its own seastamp process, so stamping depth for several
# products at once is safe: the one-thread rule for HDF5 is per process. Each
# process loads its own copy of the reference data, though, so memory use grows
# with the number of workers; use --by-region or --sequential if that is too
# much.
#
# Example:
#   scripts/stamp_data.sh -i ../ctddump/output/dedup/dedup \
#     --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f --partition \
#     --depth ./data/gebco/GEBCO_2024_sub_ice_topo.nc
#   scripts/stamp_data.sh report      # re-run only the reports

set -euo pipefail

usage() { awk 'NR<3 {next} /^#/ {sub(/^# ?/, ""); print; next} {exit}' "$0"; }

STAMP="$(cd "$(dirname "$0")" && pwd)/stamp.sh"

# ---- Configuration (defaults; override with the options below) -----------
IN=output/dedup/dedup
OUT=output/stamp
REPORT=report
FORMAT=parquet
THREADS=2
PARALLEL=file   # file | region | none
DRYRUN=0
ASSUME_YES=0
BIN="${SEASTAMP_BIN:-}"

# The stamp.sh options forwarded unchanged: those that take a value (forwarded
# with it, so it is not mistaken for a region) and the bare flags. Anything else
# is rejected here. Keep both lists in step with the parser in stamp.sh.
STAMP_VALUE_OPTS=" --coast --depth --sea --countries --nearest --municipalities \
--coast-unit --max-municipality-dist --place-unit --sea-name-field \
--nearest-name-field --nearest-unit --region --lon-col --lat-col --decimals \
--in-format --bin "
STAMP_FLAG_OPTS=" --depth-positive --depth-on-land --partition -k --keep "

# The forwarded options that also describe the stamped columns, and so go to
# `seastamp report stats` too, under the same names. RPASS collects them.
REPORT_VALUE_OPTS=" --coast-unit --place-unit --nearest-unit --lon-col --lat-col --decimals "
REPORT_FLAG_OPTS=" --depth-positive "

# ---- Parse options -------------------------------------------------------
# Options may appear anywhere; the remaining words are regions. Options this
# script does not know are collected in PASS for stamp.sh.
ARGS=()
PASS=()
RPASS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -i|--in)        IN="${2:?--in requires a directory}"; shift 2 ;;
    --in=*)         IN="${1#*=}"; shift ;;
    -o|--out)       OUT="${2:?--out requires a directory}"; shift 2 ;;
    --out=*)        OUT="${1#*=}"; shift ;;
    -r|--report)    REPORT="${2:?--report requires a directory}"; shift 2 ;;
    --report=*)     REPORT="${1#*=}"; shift ;;
    --dist-bands|--depth-bands)
                    RPASS+=("$1" "${2:?$1 requires a list}"); shift 2 ;;
    --dist-bands=*|--depth-bands=*)
                    RPASS+=("$1"); shift ;;
    -f|--format)    FORMAT="${2:?--format requires a value}"; shift 2 ;;
    --format=*)     FORMAT="${1#*=}"; shift ;;
    -t|--threads)   THREADS="${2:?--threads requires a number}"; shift 2 ;;
    --threads=*)    THREADS="${1#*=}"; shift ;;
    --by-region)    PARALLEL=region; shift ;;
    --sequential)   PARALLEL=none; shift ;;
    -n|--dry-run)   DRYRUN=1; shift ;;
    -y|--yes)       ASSUME_YES=1; shift ;;
    -h|--help)      usage; exit 0 ;;
    --)             shift; ARGS+=("$@"); break ;;
    -*)
      if [[ "$1" == --*=* && "$STAMP_VALUE_OPTS" == *" ${1%%=*} "* ]]; then
        PASS+=("$1")
        [[ "$REPORT_VALUE_OPTS" == *" ${1%%=*} "* ]] && RPASS+=("$1")
        [[ "${1%%=*}" == --bin ]] && BIN="${1#*=}"
        shift
      elif [[ "$STAMP_VALUE_OPTS" == *" $1 "* ]]; then
        PASS+=("$1" "${2:?$1 requires a value}")
        [[ "$REPORT_VALUE_OPTS" == *" $1 "* ]] && RPASS+=("$1" "$2")
        [[ "$1" == --bin ]] && BIN="$2"
        shift 2
      elif [[ "$STAMP_FLAG_OPTS" == *" $1 "* ]]; then
        PASS+=("$1")
        [[ "$REPORT_FLAG_OPTS" == *" $1 "* ]] && RPASS+=("$1")
        shift
      else
        echo "Unknown option: $1 (see --help, and scripts/stamp.sh --help)" >&2; exit 1
      fi ;;
    *)              ARGS+=("$1"); shift ;;
  esac
done

case "$FORMAT" in
  parquet|csv|tsv|csv.gz|tsv.gz) ;;
  *) echo "Unknown --format: $FORMAT (parquet, csv, tsv, csv.gz, tsv.gz)" >&2; exit 1 ;;
esac

PASS+=(--threads "$THREADS")
RPASS+=(--threads "$THREADS")
[[ "$DRYRUN" == 1 ]] && PASS+=(--dry-run)

REGIONS=(arctic baltic mediterranean)

# Merged-file stems produced by ctddump's convert_data.sh and carried through
# clean_data.sh and dedup_data.sh.
stems_for() {  # <region>
  case "$1" in
    arctic)        echo nrt_ar_ar nrt_ar_gl cora_ar ;;
    baltic)        echo nrt_bo_bo nrt_bo_gl cora_bo ;;
    mediterranean) echo nrt_mo_mo nrt_mo_gl cora_mo ;;
  esac
}

# ---- Logging -------------------------------------------------------------
# Announce each step (timestamped, to stderr). Each parallel worker sets TAG so
# its lines, and those of the stamp.sh it runs, are tagged "[stem]".
log() {
  local p=""
  [[ -n "${TAG:-}" ]] && p="[$TAG] "
  printf '[%s] %s%s\n' "$(date '+%H:%M:%S')" "$p" "$*" >&2
}

# Copy stdin to stderr with "[TAG] " inserted after the leading timestamp of
# stamp.sh's log lines (or at the start of any other line), so the output of
# workers running side by side stays attributable.
tag_lines() {
  local line re='^(\[[0-9:]+\] )(.*)$'
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" =~ $re ]]; then
      printf '%s[%s] %s\n' "${BASH_REMATCH[1]}" "$TAG" "${BASH_REMATCH[2]}"
    else
      printf '[%s] %s\n' "$TAG" "$line"
    fi
  done >&2
}

# Print the resolved configuration, then ask for confirmation unless -y/--yes
# (or --dry-run) was given. In a non-interactive shell without -y there is
# nothing to read, so abort with a hint rather than hang.
show_config() {  # <nfound> <nmissing> <region...>
  local nfound="$1" nmissing="$2"; shift 2
  local mode
  case "$PARALLEL" in
    none)   mode="sequential" ;;
    region) [[ $# -gt 1 ]] && mode="parallel (per region)" || mode="sequential" ;;
    file)   [[ $nfound -gt 1 ]] && mode="parallel (per product)" || mode="sequential" ;;
  esac
  {
    echo "Configuration:"
    echo "  regions  : $*"
    echo "  products : $nfound found, $nmissing missing (skipped)"
    if [[ "$CMD" != report ]]; then echo "  in       : $IN"; fi
    echo "  command  : $CMD"
    echo "  out      : $OUT/<stem>.$FORMAT"
    if [[ "$CMD" != stamp ]]; then echo "  report   : $REPORT/stamp/<stem>.tsv"; fi
    echo "  threads  : $THREADS per product"
    echo "  mode     : $mode"
    if [[ "$CMD" != report ]]; then echo "  stamp.sh : ${PASS[*]}"; fi
    if [[ "$CMD" != stamp ]]; then echo "  stats    : seastamp report stats ${RPASS[*]}"; fi
    echo "Run with -h/--help to see all options."
  } >&2
}

confirm() {
  [[ "$ASSUME_YES" == 1 || "$DRYRUN" == 1 ]] && return 0
  if [[ ! -t 0 ]]; then
    log "non-interactive shell: pass -y/--yes to proceed without confirmation."
    return 1
  fi
  local reply
  read -r -p "Proceed? [y/N] " reply
  [[ "$reply" == [yY] || "$reply" == [yY][eE][sS] ]]
}

# ---- Per-product step ----------------------------------------------------

# The file a stem's step reads: the ctddump input for stamp (and all), the
# stamped output for report alone. A stem without it is skipped.
source_of() {  # <stem>
  if [[ "$CMD" == report ]]; then echo "$OUT/$1.$FORMAT"; else echo "$IN/$1.parquet"; fi
}

stamp_file() {  # <stem>
  local TAG="$1"
  local in="$IN/$1.parquet" out="$OUT/$1.$FORMAT"
  [[ "$DRYRUN" == 1 ]] || mkdir -p "$OUT"
  log "stamp $1: $in -> $out"
  # pipefail carries a stamp.sh failure through the tagging pipe.
  "$STAMP" "${PASS[@]}" "$in" "$out" 2>&1 </dev/null | tag_lines
}

report_file() {  # <stem>
  local TAG="$1"
  local in="$OUT/$1.$FORMAT" out="$REPORT/stamp/$1.tsv"
  log "report $1: $in -> $out"
  if [[ "$DRYRUN" == 1 ]]; then
    log "RUN: $BIN report stats ${RPASS[*]} $in $out"
    return 0
  fi
  mkdir -p "$REPORT/stamp"
  "$BIN" report stats "${RPASS[@]}" "$in" "$out" 2>&1 </dev/null | tag_lines
}

# Every step of $CMD for one stem, in order.
process() {  # <stem>
  local src; src="$(source_of "$1")"
  [[ -f "$src" ]] || { TAG="$1" log "skip $1 (missing $src)"; return 0; }
  if [[ "$CMD" != report ]]; then stamp_file "$1"; fi
  if [[ "$CMD" != stamp ]]; then report_file "$1"; fi
}

run_region() {  # <region>
  local s
  for s in $(stems_for "$1"); do process "$s"; done
}

# ---- Dispatch ------------------------------------------------------------

is_region() {
  local r
  for r in "${REGIONS[@]}"; do [[ "$r" == "$1" ]] && return 0; done
  return 1
}

# Check the forwarded options once, with a stamp.sh dry run on <input>, so a bad
# flag or a missing module selection is reported a single time before any work.
preflight() {  # <input>
  local err
  if ! err="$("$STAMP" "${PASS[@]}" --dry-run "$1" "$OUT/preflight.$FORMAT" 2>&1 </dev/null)"; then
    printf '%s\n' "$err" >&2
    log "stamp.sh rejected the options above; nothing was run."
    return 1
  fi
}

# Stamp every product of the selected regions, honoring $PARALLEL:
#   file   (default) one background worker per product
#   region           one background worker per region (its products in order)
#   none             everything sequentially, on the main shell
# Worker failures are collected; exit is non-zero if any product failed.
run_all() {  # <region...>
  local -a regions=("$@") stems=()
  local r s
  for r in "${regions[@]}"; do
    for s in $(stems_for "$r"); do
      [[ -f "$(source_of "$s")" ]] && stems+=("$s")
    done
  done

  if [[ "$PARALLEL" == none || ${#stems[@]} -le 1 ]]; then
    for r in "${regions[@]}"; do
      log "===== $r ====="
      run_region "$r"
    done
    return 0
  fi

  local -a pids=() labels=()
  local i
  if [[ "$PARALLEL" == region ]]; then
    log "starting ${#regions[@]} regions in parallel (--sequential to disable)"
    for r in "${regions[@]}"; do
      ( TAG="$r"; run_region "$r" ) </dev/null &
      pids+=("$!"); labels+=("$r")
    done
  else  # file (default)
    log "starting ${#stems[@]} products in parallel (--by-region or --sequential to change)"
    for s in "${stems[@]}"; do
      ( process "$s" ) </dev/null &
      pids+=("$!"); labels+=("$s")
    done
  fi

  local fail=0
  for i in "${!pids[@]}"; do
    if ! wait "${pids[$i]}"; then
      log "'${labels[$i]}' FAILED"; fail=1
    fi
  done
  return "$fail"
}

# Resolve the seastamp binary for the report step, the same way stamp.sh does:
# --bin / $SEASTAMP_BIN if given; else seastamp on PATH; else a build in this
# repo (release preferred over debug).
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

main() {
  # An optional leading command, then regions.
  CMD=all
  case "${1:-}" in
    help)             usage; return 0 ;;
    stamp|report|all) CMD="$1"; shift ;;
  esac
  # The remaining words are regions; default to all, and "all" is an alias.
  local -a regions=("$@")
  if [[ ${#regions[@]} -eq 0 || "${regions[0]}" == "all" ]]; then
    regions=("${REGIONS[@]}")
  fi
  local r s
  for r in "${regions[@]}"; do
    is_region "$r" || { echo "Unknown region: $r" >&2; usage; return 1; }
  done

  local dir="$IN" opt=--in
  [[ "$CMD" == report ]] && { dir="$OUT"; opt=--out; }
  [[ -d "$dir" ]] || { echo "directory not found: $dir (see $opt)" >&2; return 1; }
  if [[ "$CMD" != stamp ]]; then resolve_bin || return 1; fi

  local -a found=()
  local nmissing=0
  for r in "${regions[@]}"; do
    for s in $(stems_for "$r"); do
      if [[ -f "$(source_of "$s")" ]]; then found+=("$s"); else nmissing=$((nmissing + 1)); fi
    done
  done
  if [[ ${#found[@]} -eq 0 ]]; then
    echo "no inputs found for: ${regions[*]} (looked for $(source_of '<stem>'))" >&2; return 1
  fi

  show_config "${#found[@]}" "$nmissing" "${regions[@]}"
  if [[ "$CMD" != report ]]; then preflight "$IN/${found[0]}.parquet" || return 1; fi
  confirm || { log "aborted."; return 1; }

  run_all "${regions[@]}" || { log "one or more products failed."; return 1; }
  log "done."
}

main ${ARGS[@]+"${ARGS[@]}"}
