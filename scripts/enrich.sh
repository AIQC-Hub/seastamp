#!/usr/bin/env bash
#
# enrich.sh: run several seastamp modules over one input in sequence, so the
# result is a single file carrying the new columns of every selected module.
#
# Each module is chained onto the previous one's output (its columns accumulate).
# The in-between files are written to a temporary directory that is removed when
# the script ends, so only the final output file remains.
#
# Usage:
#   scripts/enrich.sh [options] <input> <output>
#
# A module runs only when you give its data source, so the flags you pass pick
# both the modules and their inputs. At least one module must be selected. The
# modules run in a fixed order (coast, depth, sea, place, nearest); the order
# does not matter, since each adds distinct columns.
#
# Module selection (give the data source to enable the module):
#   --coast PATH          GSHHG shapefile dir or a GSHHS_*_L1.shp file (coast)
#   --depth FILE          GEBCO bathymetry NetCDF file (depth)
#   --sea PATH            IHO Sea Areas GeoJSON or shapefile (sea)
#   --countries FILE      Natural Earth countries shapefile (place)
#   --nearest FILE        reference table of named locations (nearest, --to)
#
# Per-module options:
#   --municipalities FILE GISCO LAU municipalities shapefile (place, optional)
#   --coast-unit km|m     distance unit for coast   (default: km)
#   --depth-positive      report depth positive below sea level
#   --depth-on-land       add an on_land boolean column (depth)
#   --max-municipality-dist N  drop a municipality match further than N away
#                         (place; in --place-unit, default km)
#   --place-unit km|m     unit for municipality_dist and its cutoff (default: km)
#   --sea-name-field STR  feature field with the sea name  (default: NAME)
#   --nearest-name-field STR  reference name column  (default: name)
#   --nearest-unit km|m   distance unit for nearest (default: km)
#
# Common options (applied to every module that accepts them):
#   --region NAME         region preset (coast, sea, place). See seastamp --help
#   --lon-col NAME        longitude column   (default: longitude)
#   --lat-col NAME        latitude column    (default: latitude)
#   --decimals N          rounding before de-duplication  (default: 3)
#   -t, --threads N       worker threads. depth ignores it for the grid lookup,
#                         which always runs on one thread (HDF5 is often built
#                         without thread safety)
#   --in-format FMT       format of <input> (default: inferred from extension)
#
# Other options:
#   --bin PATH            seastamp binary (default: $SEASTAMP_BIN, else the
#                         one on PATH, else ./target/release|debug/seastamp)
#   -k, --keep            keep the intermediate files (default: remove them)
#   -n, --dry-run         print the commands without running them
#   -h, --help            show this help
#
# Example:
#   scripts/enrich.sh cores.parquet cores.enriched.parquet \
#     --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f \
#     --depth ./data/gebco/GEBCO_2024_sub_ice.nc \
#     --nearest farms.parquet --nearest-name-field farm_name

set -euo pipefail

usage() { awk 'NR<3 {next} /^#/ {sub(/^# ?/, ""); print; next} {exit}' "$0"; }

# ---- Configuration (defaults; override with the options below) -----------
COAST=; DEPTH=; SEA=; COUNTRIES=; NEAREST=
MUNICIPALITIES=
COAST_UNIT=km
DEPTH_POSITIVE=0
DEPTH_ON_LAND=0
SEA_NAME_FIELD=NAME
PLACE_UNIT=km
MAX_MUNI_DIST=
NEAREST_NAME_FIELD=name
NEAREST_UNIT=km
REGION=
LON_COL=; LAT_COL=; DECIMALS=; THREADS=; IN_FORMAT=
BIN="${SEASTAMP_BIN:-}"
KEEP=0
DRYRUN=0

# Intermediate files live under TMP, removed on exit unless --keep (never
# removed in --dry-run, which does not create it). Global so the EXIT trap,
# which fires after main returns, can still see it.
TMP=""
cleanup() { [[ -n "$TMP" && "$KEEP" != 1 && "$DRYRUN" != 1 ]] && rm -rf "$TMP"; }
trap cleanup EXIT

# ---- Parse options -------------------------------------------------------
# Options may appear anywhere; the two remaining words are <input> and <output>.
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --coast)              COAST="${2:?--coast requires a path}"; shift 2 ;;
    --coast=*)            COAST="${1#*=}"; shift ;;
    --depth)              DEPTH="${2:?--depth requires a file}"; shift 2 ;;
    --depth=*)            DEPTH="${1#*=}"; shift ;;
    --sea)                SEA="${2:?--sea requires a path}"; shift 2 ;;
    --sea=*)              SEA="${1#*=}"; shift ;;
    --countries)          COUNTRIES="${2:?--countries requires a file}"; shift 2 ;;
    --countries=*)        COUNTRIES="${1#*=}"; shift ;;
    --nearest)            NEAREST="${2:?--nearest requires a file}"; shift 2 ;;
    --nearest=*)          NEAREST="${1#*=}"; shift ;;
    --municipalities)     MUNICIPALITIES="${2:?--municipalities requires a file}"; shift 2 ;;
    --municipalities=*)   MUNICIPALITIES="${1#*=}"; shift ;;
    --coast-unit)         COAST_UNIT="${2:?--coast-unit requires km or m}"; shift 2 ;;
    --coast-unit=*)       COAST_UNIT="${1#*=}"; shift ;;
    --depth-positive)     DEPTH_POSITIVE=1; shift ;;
    --depth-on-land)      DEPTH_ON_LAND=1; shift ;;
    --max-municipality-dist) MAX_MUNI_DIST="${2:?--max-municipality-dist requires a number}"; shift 2 ;;
    --max-municipality-dist=*) MAX_MUNI_DIST="${1#*=}"; shift ;;
    --place-unit)         PLACE_UNIT="${2:?--place-unit requires km or m}"; shift 2 ;;
    --place-unit=*)       PLACE_UNIT="${1#*=}"; shift ;;
    --sea-name-field)     SEA_NAME_FIELD="${2:?--sea-name-field requires a value}"; shift 2 ;;
    --sea-name-field=*)   SEA_NAME_FIELD="${1#*=}"; shift ;;
    --nearest-name-field) NEAREST_NAME_FIELD="${2:?--nearest-name-field requires a value}"; shift 2 ;;
    --nearest-name-field=*) NEAREST_NAME_FIELD="${1#*=}"; shift ;;
    --nearest-unit)       NEAREST_UNIT="${2:?--nearest-unit requires km or m}"; shift 2 ;;
    --nearest-unit=*)     NEAREST_UNIT="${1#*=}"; shift ;;
    --region)             REGION="${2:?--region requires a name}"; shift 2 ;;
    --region=*)           REGION="${1#*=}"; shift ;;
    --lon-col)            LON_COL="${2:?--lon-col requires a name}"; shift 2 ;;
    --lon-col=*)          LON_COL="${1#*=}"; shift ;;
    --lat-col)            LAT_COL="${2:?--lat-col requires a name}"; shift 2 ;;
    --lat-col=*)          LAT_COL="${1#*=}"; shift ;;
    --decimals)           DECIMALS="${2:?--decimals requires a number}"; shift 2 ;;
    --decimals=*)         DECIMALS="${1#*=}"; shift ;;
    -t|--threads)         THREADS="${2:?--threads requires a number}"; shift 2 ;;
    --threads=*)          THREADS="${1#*=}"; shift ;;
    --in-format)          IN_FORMAT="${2:?--in-format requires a value}"; shift 2 ;;
    --in-format=*)        IN_FORMAT="${1#*=}"; shift ;;
    --bin)                BIN="${2:?--bin requires a path}"; shift 2 ;;
    --bin=*)              BIN="${1#*=}"; shift ;;
    -k|--keep)            KEEP=1; shift ;;
    -n|--dry-run)         DRYRUN=1; shift ;;
    -h|--help)            usage; exit 0 ;;
    --)                   shift; ARGS+=("$@"); break ;;
    -*)                   echo "Unknown option: $1" >&2; usage; exit 1 ;;
    *)                    ARGS+=("$1"); shift ;;
  esac
done

# ---- Logging -------------------------------------------------------------
# Announce each step (timestamped, to stderr). `log` prints a message; `run`
# logs the command and runs it (or, with --dry-run, only logs it).
log() { printf '[%s] %s\n' "$(date '+%H:%M:%S')" "$*" >&2; }
run() {
  log "RUN: $*"
  [[ "$DRYRUN" == 1 ]] && return 0
  "$@"
}

# ---- Resolve the seastamp binary ----------------------------------------
# Use --bin / $SEASTAMP_BIN if given; else seastamp on PATH; else a build in
# this repo (release preferred over debug), so the script works from a checkout.
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

# ---- Assemble the shared and per-module arguments ------------------------
# Common args go to every module; region args only to the region-aware ones.
common_args() {  # echoes the flags shared by all modules
  local -a a=()
  [[ -n "$LON_COL"  ]] && a+=(--lon-col "$LON_COL")
  [[ -n "$LAT_COL"  ]] && a+=(--lat-col "$LAT_COL")
  [[ -n "$DECIMALS" ]] && a+=(--decimals "$DECIMALS")
  [[ -n "$THREADS"  ]] && a+=(--threads "$THREADS")
  [[ ${#a[@]} -gt 0 ]] && printf '%s\n' "${a[@]}"
}
region_args() {  # echoes the region flag, for coast / sea / place
  [[ -n "$REGION" ]] && printf '%s\n' --region "$REGION"
}

# Echo the module-specific flags for <module>, one per line.
module_args() {  # <module>
  case "$1" in
    coast)
      printf '%s\n' --data "$COAST" --unit "$COAST_UNIT"
      region_args ;;
    depth)
      printf '%s\n' --data "$DEPTH"
      [[ "$DEPTH_POSITIVE" == 1 ]] && printf '%s\n' --positive
      [[ "$DEPTH_ON_LAND"  == 1 ]] && printf '%s\n' --on-land ;;
    sea)
      printf '%s\n' --data "$SEA" --name-field "$SEA_NAME_FIELD"
      region_args ;;
    place)
      printf '%s\n' --countries "$COUNTRIES" --unit "$PLACE_UNIT"
      [[ -n "$MUNICIPALITIES" ]] && printf '%s\n' --municipalities "$MUNICIPALITIES"
      [[ -n "$MAX_MUNI_DIST"  ]] && printf '%s\n' --max-municipality-dist "$MAX_MUNI_DIST"
      region_args ;;
    nearest)
      printf '%s\n' --to "$NEAREST" --name-field "$NEAREST_NAME_FIELD" --unit "$NEAREST_UNIT" ;;
  esac
}

main() {
  # Positional: <input> <output>.
  if [[ ${#ARGS[@]} -ne 2 ]]; then
    echo "Expected <input> and <output>." >&2; usage; return 1
  fi
  local input="${ARGS[0]}" output="${ARGS[1]}"
  [[ -e "$input" ]] || { echo "input not found: $input" >&2; return 1; }

  # Collect the enabled modules in a fixed order.
  local -a modules=()
  [[ -n "$COAST"     ]] && modules+=(coast)
  [[ -n "$DEPTH"     ]] && modules+=(depth)
  [[ -n "$SEA"       ]] && modules+=(sea)
  [[ -n "$COUNTRIES" ]] && modules+=(place)
  [[ -n "$NEAREST"   ]] && modules+=(nearest)
  if [[ -n "$MUNICIPALITIES" && -z "$COUNTRIES" ]]; then
    echo "--municipalities needs --countries (the place module)." >&2; return 1
  fi
  if [[ ${#modules[@]} -eq 0 ]]; then
    echo "Select at least one module (see --help)." >&2; usage; return 1
  fi

  resolve_bin || return 1

  # A temp dir for the intermediates is only needed when more than one module
  # runs (a hand-off between steps). In --dry-run nothing is written, so use a
  # label instead of creating one.
  if [[ ${#modules[@]} -gt 1 ]]; then
    if [[ "$DRYRUN" == 1 ]]; then
      TMP="${TMPDIR:-/tmp}/seastamp.DRYRUN"
    else
      TMP="$(mktemp -d "${TMPDIR:-/tmp}/seastamp.XXXXXX")"
      log "intermediate files in $TMP ($([[ "$KEEP" == 1 ]] && echo kept || echo removed on exit))"
    fi
  fi

  log "modules: ${modules[*]}  ($input -> $output)"

  # Chain: each module reads the previous step's output and appends its columns.
  # Intermediates are Parquet (lossless); the last module writes the final file,
  # whose format follows its extension.
  local current="$input" last_i=$(( ${#modules[@]} - 1 ))
  local i m out
  local -a cargs; mapfile -t cargs < <(common_args)
  for i in "${!modules[@]}"; do
    m="${modules[$i]}"
    if [[ "$i" -eq "$last_i" ]]; then out="$output"; else out="$TMP/step$i.$m.parquet"; fi

    local -a margs; mapfile -t margs < <(module_args "$m")
    # --in-format only describes the original input, so pass it to the first
    # module only; intermediates are Parquet and infer their format.
    local -a fmt=()
    [[ "$i" -eq 0 && -n "$IN_FORMAT" ]] && fmt=(--in-format "$IN_FORMAT")

    log "===== ${m} ($((i + 1))/${#modules[@]}) ====="
    run "$BIN" "$m" "$current" -o "$out" \
      ${fmt[@]+"${fmt[@]}"} \
      ${margs[@]+"${margs[@]}"} \
      ${cargs[@]+"${cargs[@]}"}
    current="$out"
  done

  log "done. wrote $output"
}

main
