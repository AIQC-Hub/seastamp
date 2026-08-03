#!/usr/bin/env bash
#
# download_data.sh: download the seastamp reference datasets into a local
# data/ tree, one sub-directory per source, and unpack them so the paths in
# the README examples work as-is.
#
# Usage:
#   scripts/download_data.sh [options] [command] [dataset ...]
#
# Commands:
#   download    Download and unpack the selected datasets.  (default)
#   help        Show this help.
#
# Datasets:  gshhg  gebco  iho  countries  lau   (default: all five; "all"
# also works)
#
#   gshhg       GSHHG shorelines (SOEST), for `coast`. Zip of ESRI shapefiles
#               in every resolution; `coast` wants GSHHS_shp/f.
#   gebco       GEBCO sub-ice bathymetry grid (BODC), for `depth`. One NetCDF
#               inside a zip. Large: several GB, resumes on re-run.
#   iho         IHO Sea Areas v3 (Marine Regions), for `sea`. The download
#               sits behind the marineregions.org form, which collects user
#               statistics and requires accepting the dataset licence
#               (CC BY-NC-SA 4.0). Every field it marks required has to be
#               supplied: --mr-name, --mr-org, --mr-email, --mr-country,
#               --mr-category, and --mr-purpose. The last two have defaults.
#               Running this download submits the form and accepts the licence,
#               so the values are shown for confirmation before anything starts.
#   countries   Natural Earth 10m admin 0 countries, for `place --countries`.
#   lau         Eurostat GISCO LAU boundaries, for `place --municipalities`.
#               The 4326 (lon/lat) shapefile is unpacked from the bundle;
#               seastamp needs lon/lat coordinates.
#
# Options (may appear anywhere on the command line):
#   -d, --data DIR    root of the data tree  (default: data)
#   --gebco-year N    GEBCO grid year  (default: 2024)
#   --lau-year N      GISCO LAU reference year  (default: 2021)
#   --mr-name STR     your name, for the Marine Regions form (iho only)
#   --mr-org STR      your organisation, for the form (iho only)
#   --mr-email STR    your email, for the Marine Regions form (iho only)
#   --mr-country STR  your country, in English, as the form spells it (iho only)
#   --mr-category STR user category (default: academia). One of:
#                       academia, industry, government, civil society
#   --mr-purpose STR  purpose (default: Research). One of: Conservation,
#                       Data exploration & testing, Education & workshops,
#                       Fisheries, Policy & Marine Spatial Planning,
#                       Mapping & visualisation,
#                       Maritime transport & cruise planning,
#                       Industry & offshore activities, Research, GIS Analysis,
#                       Personal information, Other
#   --force           Re-download files that already exist (default: an
#                     existing archive is kept and only unpacked again).
#   --sequential      Download datasets one at a time (default: selected
#                     datasets download in parallel when more than one is
#                     chosen).
#   -y, --yes         Skip the confirmation prompt and start immediately.
#   -h, --help        Show this help.
#
# Requires: curl and unzip on PATH. No accounts are needed; the iho dataset
# submits the Marine Regions form non-interactively with the --mr-* details
# you provide (see above).

set -euo pipefail

usage() { awk 'NR<3 {next} /^#/ {sub(/^# ?/, ""); print; next} {exit}' "$0"; }

# ---- Configuration (defaults; override with the options below) -----------
DATA=data
GEBCO_YEAR=2024
LAU_YEAR=2021
GSHHG_VER=2.3.7
MR_NAME=
MR_EMAIL=
MR_COUNTRY=
MR_ORG=
MR_CATEGORY=academia
MR_PURPOSE=Research

# The form's two dropdowns accept only these values, spelled exactly like this;
# anything else is rejected with an HTML page instead of a zip. Scraped from
# https://www.marineregions.org/download_file.php?name=World_Seas_IHO_v3.zip
MR_CATEGORIES=("academia" "industry" "government" "civil society")
MR_PURPOSES=(
  "Conservation" "Data exploration & testing" "Education & workshops"
  "Fisheries" "Policy & Marine Spatial Planning" "Mapping & visualisation"
  "Maritime transport & cruise planning" "Industry & offshore activities"
  "Research" "GIS Analysis" "Personal information" "Other"
)
FORCE=0
ASSUME_YES=0
SEQUENTIAL=0

# ---- Parse options -------------------------------------------------------
# Options may appear anywhere; the remaining words are the command and datasets.
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -d|--data)      DATA="${2:?--data requires a directory}"; shift 2 ;;
    --data=*)       DATA="${1#*=}"; shift ;;
    --gebco-year)   GEBCO_YEAR="${2:?--gebco-year requires a year}"; shift 2 ;;
    --gebco-year=*) GEBCO_YEAR="${1#*=}"; shift ;;
    --lau-year)     LAU_YEAR="${2:?--lau-year requires a year}"; shift 2 ;;
    --lau-year=*)   LAU_YEAR="${1#*=}"; shift ;;
    --mr-name)      MR_NAME="${2:?--mr-name requires a value}"; shift 2 ;;
    --mr-name=*)    MR_NAME="${1#*=}"; shift ;;
    --mr-email)     MR_EMAIL="${2:?--mr-email requires a value}"; shift 2 ;;
    --mr-email=*)   MR_EMAIL="${1#*=}"; shift ;;
    --mr-country)   MR_COUNTRY="${2:?--mr-country requires a value}"; shift 2 ;;
    --mr-country=*) MR_COUNTRY="${1#*=}"; shift ;;
    --mr-org)       MR_ORG="${2:?--mr-org requires a value}"; shift 2 ;;
    --mr-org=*)     MR_ORG="${1#*=}"; shift ;;
    --mr-category)  MR_CATEGORY="${2:?--mr-category requires a value}"; shift 2 ;;
    --mr-category=*) MR_CATEGORY="${1#*=}"; shift ;;
    --mr-purpose)   MR_PURPOSE="${2:?--mr-purpose requires a value}"; shift 2 ;;
    --mr-purpose=*) MR_PURPOSE="${1#*=}"; shift ;;
    --force)        FORCE=1; shift ;;
    --sequential)   SEQUENTIAL=1; shift ;;
    -y|--yes)       ASSUME_YES=1; shift ;;
    -h|--help)      usage; exit 0 ;;
    --)             shift; ARGS+=("$@"); break ;;
    -*)             echo "Unknown option: $1" >&2; usage; exit 1 ;;
    *)              ARGS+=("$1"); shift ;;
  esac
done

DATASETS=(gshhg gebco iho countries lau)

# ---- Logging -------------------------------------------------------------
# Announce each step (timestamped, to stderr) so the currently running process
# is visible. `log` prints a message; `run` logs the command then executes it.

# Each parallel dataset worker sets DATASET so its lines are tagged "[dataset]".
log() {
  local p=""
  [[ -n "${DATASET:-}" ]] && p="[$DATASET] "
  printf '[%s] %s%s\n' "$(date '+%H:%M:%S')" "$p" "$*" >&2
}
run() { log "RUN: $*"; "$@"; }

# Is <needle> one of the remaining arguments? Used for the form's dropdowns,
# whose values contain spaces and ampersands, so a substring match would lie.
in_list() {  # <needle> <candidate...>
  local needle="$1"; shift
  local c
  for c in "$@"; do [[ "$c" == "$needle" ]] && return 0; done
  return 1
}

# Check the Marine Regions form values before anything is downloaded, so a typo
# in a dropdown fails immediately instead of after the multi-GB GEBCO grid.
# Every field the form marks required is required here too.
check_mr() {
  local missing=()
  [[ -z "$MR_NAME"    ]] && missing+=(--mr-name)
  [[ -z "$MR_ORG"     ]] && missing+=(--mr-org)
  [[ -z "$MR_EMAIL"   ]] && missing+=(--mr-email)
  [[ -z "$MR_COUNTRY" ]] && missing+=(--mr-country)
  if [[ ${#missing[@]} -gt 0 ]]; then
    log "the Marine Regions form requires: ${missing[*]}"
    log "see -h/--help, or fetch the iho zip by hand from"
    log "https://www.marineregions.org/downloads.php"
    return 1
  fi
  if ! in_list "$MR_CATEGORY" "${MR_CATEGORIES[@]}"; then
    log "invalid --mr-category: $MR_CATEGORY"
    local c
    for c in "${MR_CATEGORIES[@]}"; do log "  valid: $c"; done
    return 1
  fi
  if ! in_list "$MR_PURPOSE" "${MR_PURPOSES[@]}"; then
    log "invalid --mr-purpose: $MR_PURPOSE"
    local p
    for p in "${MR_PURPOSES[@]}"; do log "  valid: $p"; done
    return 1
  fi
  return 0
}

# Print the resolved configuration, then ask for confirmation unless -y/--yes was
# given. In a non-interactive shell without -y there is nothing to read, so abort
# with a hint rather than hang.
show_config() {  # <cmd> <dataset...>
  local cmd="$1"; shift
  local mode="sequential"
  [[ "$SEQUENTIAL" != 1 && $# -gt 1 ]] && mode="parallel (per dataset)"
  {
    echo "Configuration:"
    echo "  command  : $cmd"
    echo "  datasets : $*"
    echo "  data     : $DATA"
    echo "  gebco    : $GEBCO_YEAR"
    echo "  lau      : $LAU_YEAR"
    echo "  mode     : $mode"
    if in_list iho "$@"; then
      # These are submitted to a third party, so show them in full and say what
      # accepting means, before anything is sent.
      echo "Marine Regions form (iho), submitted on your behalf:"
      echo "  name     : $MR_NAME"
      echo "  org      : $MR_ORG"
      echo "  email    : $MR_EMAIL"
      echo "  country  : $MR_COUNTRY"
      echo "  category : $MR_CATEGORY"
      echo "  purpose  : $MR_PURPOSE"
      echo "  licence  : accepting CC BY-NC-SA 4.0 (non-commercial, share-alike)"
    fi
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

# ---- Download primitives -------------------------------------------------

# Fetch <url> to <dest> unless it already exists (and --force is not set).
# Downloads land in <dest>.part first and are renamed on success, so an
# interrupted run never leaves a truncated file under the final name; -C -
# resumes a partial .part (matters for the multi-GB GEBCO grid). Extra
# arguments are passed to curl (the iho form fields).
fetch() {  # <url> <dest> [curl args...]
  local url="$1" dest="$2"; shift 2
  if [[ -e "$dest" && "$FORCE" != 1 ]]; then
    log "have $dest (use --force to re-download)"
    return 0
  fi
  run curl -fL --retry 3 -C - -o "$dest.part" "$@" "$url"
  mv "$dest.part" "$dest"
}

# Unpack <zip> into <dir> (idempotent; -o overwrites what is already there).
unpack() {  # <zip> <dir>
  run unzip -o -q "$1" -d "$2"
}

# ---- Datasets ------------------------------------------------------------
# One download_<dataset> function each: fetch the archive into $DATA/<source>/
# and unpack it where the README example paths expect it.

download_gshhg() {
  local dir="$DATA/gshhg" zip="gshhg-shp-$GSHHG_VER.zip"
  mkdir -p "$dir"
  fetch "https://www.soest.hawaii.edu/pwessel/gshhg/$zip" "$dir/$zip"
  # The zip has GSHHS_shp/ at its top level; unpack under a versioned
  # directory so `coast` gets $DATA/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f.
  unpack "$dir/$zip" "$dir/gshhg-shp-$GSHHG_VER"
}

download_gebco() {
  local dir="$DATA/gebco" zip="gebco_${GEBCO_YEAR}_sub_ice.zip"
  mkdir -p "$dir"
  log "the GEBCO grid is several GB; an interrupted download resumes on re-run"
  fetch "https://www.bodc.ac.uk/data/open_download/gebco/gebco_${GEBCO_YEAR}_sub_ice_topo/zip/" "$dir/$zip"
  unpack "$dir/$zip" "$dir"
}

download_iho() {
  local dir="$DATA/iho" zip="World_Seas_IHO_v3.zip"
  local url="https://www.marineregions.org/download_file.php?name=$zip"
  mkdir -p "$dir"
  if [[ ! -e "$dir/$zip" || "$FORCE" == 1 ]]; then
    # marineregions.org gates downloads behind a short form: user statistics
    # (name, organisation, email, country, user category, and purpose are all
    # required) plus acceptance of the dataset licence. Submit it
    # non-interactively with the details given on the command line; running this
    # download means accepting CC BY-NC-SA 4.0. check_mr has already validated
    # them and show_config has displayed them for confirmation.
    check_mr || return 1
    # The form page carries a hidden anti-bot field that must be posted back
    # empty; scrape its per-page name first.
    local honeypot
    honeypot="$(curl -fsL --max-time 60 "$url" \
      | grep -oE 'name="firstname-[a-f0-9]+"' | head -1 | cut -d'"' -f2 || true)"
    fetch "$url" "$dir/$zip" \
      --data-urlencode "name=$MR_NAME" \
      --data-urlencode "organisation=$MR_ORG" \
      --data-urlencode "email=$MR_EMAIL" \
      --data-urlencode "country=$MR_COUNTRY" \
      --data-urlencode "user_category=$MR_CATEGORY" \
      --data-urlencode "purpose_category=$MR_PURPOSE" \
      --data-urlencode "agree=1" \
      ${honeypot:+--data-urlencode "$honeypot="}
    # A rejected form submission returns an HTML page, not a zip: fail loudly
    # rather than leave a broken archive for `sea` to trip over.
    if ! unzip -t -q "$dir/$zip" >/dev/null 2>&1; then
      rm -f "$dir/$zip"
      log "the Marine Regions form rejected the download; check the --mr-*"
      log "values, or fetch $zip manually from"
      log "https://www.marineregions.org/downloads.php into $dir/"
      return 1
    fi
  else
    log "have $dir/$zip (use --force to re-download)"
  fi
  # The zip unpacks to World_Seas_IHO_v3/World_Seas_IHO_v3.shp.
  unpack "$dir/$zip" "$dir"
}

download_countries() {
  local dir="$DATA/naturalearth" zip="ne_10m_admin_0_countries.zip"
  mkdir -p "$dir"
  # naturalearthdata.com download links redirect to this S3 bucket.
  fetch "https://naturalearth.s3.amazonaws.com/10m_cultural/$zip" "$dir/$zip"
  unpack "$dir/$zip" "$dir"
}

download_lau() {
  local dir="$DATA/gisco" zip="ref-lau-$LAU_YEAR-01m.shp.zip"
  mkdir -p "$dir"
  fetch "https://gisco-services.ec.europa.eu/distribution/v2/lau/download/$zip" "$dir/$zip"
  unpack "$dir/$zip" "$dir"
  # The bundle nests one zip per layer and projection; seastamp needs the
  # lon/lat (EPSG 4326) polygon layer.
  local inner="$dir/LAU_RG_01M_${LAU_YEAR}_4326.shp.zip"
  if [[ ! -e "$inner" ]]; then
    log "no LAU_RG_01M_${LAU_YEAR}_4326.shp.zip inside $zip; check --lau-year"
    return 1
  fi
  unpack "$inner" "$dir"
}

# ---- Dispatch ------------------------------------------------------------

is_dataset() {
  local d
  for d in "${DATASETS[@]}"; do [[ "$d" == "$1" ]] && return 0; done
  return 1
}

# Run <cmd> for every dataset. Datasets run in parallel (one background worker
# each, with stdin detached) unless --sequential is set or only one dataset is
# selected. Worker failures are collected and reported; exit is non-zero if any
# dataset failed.
run_datasets() {  # <cmd> <dataset...>
  local cmd="$1"; shift
  local -a datasets=("$@")

  if [[ "$SEQUENTIAL" == 1 || ${#datasets[@]} -le 1 ]]; then
    local d fail=0
    for d in "${datasets[@]}"; do
      log "===== $cmd: $d ====="
      "download_$d" || { log "dataset '$d' FAILED"; fail=1; }
    done
    return "$fail"
  fi

  log "starting ${#datasets[@]} datasets in parallel (--sequential to disable)"
  local -a pids=() sets=()
  local d
  for d in "${datasets[@]}"; do
    ( DATASET="$d"; log "===== $cmd: $d ====="; "download_$d" ) </dev/null &
    pids+=("$!"); sets+=("$d")
  done
  local fail=0 i
  for i in "${!pids[@]}"; do
    if ! wait "${pids[$i]}"; then
      log "dataset '${sets[$i]}' FAILED"; fail=1
    fi
  done
  return "$fail"
}

main() {
  local cmd="${1:-download}"
  [[ $# -gt 0 ]] && shift

  case "$cmd" in
    -h|--help|help) usage; return 0 ;;
    download) ;;
    *) echo "Unknown command: $cmd" >&2; usage; return 1 ;;
  esac

  local tool
  for tool in curl unzip; do
    command -v "$tool" >/dev/null || { echo "$tool not found on PATH" >&2; return 1; }
  done

  # Remaining args are datasets; default to all, and "all" is an alias.
  local -a datasets=("$@")
  if [[ ${#datasets[@]} -eq 0 || "${datasets[0]}" == "all" ]]; then
    datasets=("${DATASETS[@]}")
  fi
  local d
  for d in "${datasets[@]}"; do
    is_dataset "$d" || { echo "Unknown dataset: $d" >&2; usage; return 1; }
  done

  # Validate the form values before the summary, so what is shown is what will
  # actually be sent, and a bad value costs nothing.
  if in_list iho "${datasets[@]}"; then
    check_mr || return 1
  fi

  show_config "$cmd" "${datasets[@]}"
  confirm || { log "aborted."; return 1; }

  run_datasets "$cmd" "${datasets[@]}" || { log "one or more datasets failed."; return 1; }
  log "done."
}

main ${ARGS[@]+"${ARGS[@]}"}
