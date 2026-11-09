#!/usr/bin/env bash
#
# summary_site.sh: build a static local web site from the Markdown summary pages
# produced by summary_data.sh, using mdBook. The pages are assembled into a book
# (grouped into one part per region), rendered to self-contained HTML, and written
# to a site directory you can open in a browser or serve as-is.
#
# Usage:
#   scripts/summary_site.sh [options] [region ...]
#
# Regions:  arctic  baltic  mediterranean   (default: all three; "all" also works)
#
# Options (may appear anywhere on the command line):
#   -s, --src DIR      directory holding <stem>.md summary pages (default: summary)
#   -d, --dest DIR     directory to write the built site into     (default: site)
#   -c, --config FILE  custom book.toml to use instead of the built-in template
#   -t, --title TEXT   book title (built-in template only)
#                      (default: "seastamp: stamped data summary reports")
#   -l, --license FILE LICENSE to copy into the built site
#                      (default: the LICENSE beside this script; "" to skip)
#   --citation FILE    CITATION.cff to copy into the built site
#                      (default: site-CITATION.cff beside this script, the
#                      citation of the published example site; "" to skip).
#                      When it has a top-level `doi:`, the site README also gets
#                      a DOI badge and a Citation section.
#   -y, --yes          Skip the confirmation prompt and start immediately.
#   -h, --help         Show this help.
#
# Each page's chapter name is taken from its own top-level "# " heading (the title
# summary_data.sh gave it), so titles live in one place. A region with no pages is
# skipped; if no pages are found at all, that is an error rather than an empty site.
#
# The built site directory is made self-contained and publishable: mdBook writes a
# .nojekyll (so GitHub Pages serves its asset folders), and this script adds a
# LICENSE (see --license), a CITATION.cff (see --citation), and a short
# README.md describing the site. mdBook empties the site directory before each
# build, so files added there by hand do not survive a rebuild: pass them here.
#
# The built-in book.toml is written for you and covers the common case. With
# --config, your file is used verbatim, so it must keep mdBook's default `src =
# "src"`: the script assembles the chapters into a `src/` directory beside it.
#
# Requires mdbook on PATH (cargo install mdbook) and summary_data.sh's Markdown
# pages in <src>. Run summary_data.sh first; it must have been run with `-f md`
# (its default), since this site is built from Markdown, not HTML.

set -euo pipefail

usage() { awk 'NR<3 {next} /^#/ {sub(/^# ?/, ""); print; next} {exit}' "$0"; }

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

# ---- Configuration (defaults; override with the options below) -----------
SRC=summary
DEST=site
CONFIG=""
TITLE="seastamp: stamped data summary reports"
# The project's own LICENSE lives one level up from scripts/. Empty skips it.
LICENSE_SRC="$SCRIPT_DIR/../LICENSE"
# The site's own citation metadata (not seastamp's), kept beside this script so
# every build carries it without a flag. Empty skips it. A missing default is
# skipped quietly (e.g. only the .sh was copied); a missing --citation path is an
# error.
CITATION_SRC="$SCRIPT_DIR/site-CITATION.cff"
[[ -f "$CITATION_SRC" ]] || CITATION_SRC=""
ASSUME_YES=0

# ---- Parse options -------------------------------------------------------
# Options may appear anywhere; the remaining words are the regions.
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -s|--src)     SRC="${2:?--src requires a directory}"; shift 2 ;;
    --src=*)      SRC="${1#*=}"; shift ;;
    -d|--dest)    DEST="${2:?--dest requires a directory}"; shift 2 ;;
    --dest=*)     DEST="${1#*=}"; shift ;;
    -c|--config)  CONFIG="${2:?--config requires a book.toml path}"; shift 2 ;;
    --config=*)   CONFIG="${1#*=}"; shift ;;
    -t|--title)   TITLE="${2:?--title requires a title}"; shift 2 ;;
    --title=*)    TITLE="${1#*=}"; shift ;;
    -l|--license) LICENSE_SRC="${2?--license requires a path (\"\" to skip)}"; shift 2 ;;
    --license=*)  LICENSE_SRC="${1#*=}"; shift ;;
    --citation)   CITATION_SRC="${2?--citation requires a path (\"\" to skip)}"; shift 2 ;;
    --citation=*) CITATION_SRC="${1#*=}"; shift ;;
    -y|--yes)     ASSUME_YES=1; shift ;;
    -h|--help)    usage; exit 0 ;;
    --)           shift; ARGS+=("$@"); break ;;
    -*)           echo "Unknown option: $1" >&2; usage; exit 1 ;;
    *)            ARGS+=("$1"); shift ;;
  esac
done

if ! command -v mdbook >/dev/null 2>&1; then
  echo "Error: 'mdbook' not found. Install with: cargo install mdbook" >&2
  exit 1
fi
if [[ -n "$CONFIG" && ! -f "$CONFIG" ]]; then
  echo "Error: book.toml not found: $CONFIG" >&2
  exit 1
fi
if [[ -n "$LICENSE_SRC" && ! -f "$LICENSE_SRC" ]]; then
  echo "Error: LICENSE not found: $LICENSE_SRC (pass --license \"\" to skip)" >&2
  exit 1
fi
if [[ -n "$CITATION_SRC" && ! -f "$CITATION_SRC" ]]; then
  echo "Error: CITATION.cff not found: $CITATION_SRC (pass --citation \"\" to skip)" >&2
  exit 1
fi

# The top-level `doi:` of the citation file, without quotes; empty when there is
# none. Indented `doi:` lines (those inside `references:`) are not matched.
citation_doi() {
  [[ -n "$CITATION_SRC" ]] || return 0
  local line
  line=$(grep -m 1 '^doi:' "$CITATION_SRC" || true)
  line=${line#doi:}
  line=${line//\"/}
  line=${line//\'/}
  printf '%s' "${line//[[:space:]]/}"
}

REGIONS=(arctic baltic mediterranean)

# The (region, product) stems, per region: the same
# stems summary_data.sh writes its pages for. The order sets the sidebar order.
stems_for() {  # <region>
  case "$1" in
    arctic)        echo nrt_ar_ar nrt_ar_gl cora_ar ;;
    baltic)        echo nrt_bo_bo nrt_bo_gl cora_bo ;;
    mediterranean) echo nrt_mo_mo nrt_mo_gl cora_mo ;;
  esac
}

# Part heading for a region's group of chapters.
region_label() {  # <region>
  case "$1" in
    arctic)        echo "Arctic Ocean" ;;
    baltic)        echo "Baltic Sea" ;;
    mediterranean) echo "Mediterranean Sea" ;;
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
  {
    echo "Configuration:"
    echo "  regions : ${rs[*]}"
    echo "  src     : $SRC"
    echo "  dest    : $DEST"
    echo "  config  : ${CONFIG:-<built-in template>}"
    # `if`, not `[[ ]] && echo`: the latter returns 1 when the test is false, which
    # `set -e` would treat as a failure if it ever ended up last in the function.
    if [[ -z "$CONFIG" ]]; then echo "  title   : $TITLE"; fi
    echo "  license : ${LICENSE_SRC:-<none>}"
    echo "  citation: ${CITATION_SRC:-<none>}"
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

# ---- Book assembly -------------------------------------------------------
# The chapter name is the page's own "# " heading, so summary_data.sh's --title
# stays the single source of truth. Under a region part heading the region prefix
# is redundant ("Arctic Ocean: NRT ..." below a part named "Arctic Ocean"), so
# strip a leading "<label>: " when present. Falls back to the stem.
chapter_title() {  # <file> <region-label>
  local t
  t=$(awk '/^# /{sub(/^# +/, ""); print; exit}' "$1")
  [[ -z "$t" ]] && { basename "$1" .md; return; }
  printf '%s\n' "${t#"$2": }"
}

# The built-in book.toml. Written only when --config was not given.
write_default_config() {  # <path>
  cat > "$1" <<EOF
[book]
title = "$TITLE"
description = "Per-product summary reports of the data stamped by seastamp."
language = "en"
src = "src"

[output.html]
default-theme = "light"
preferred-dark-theme = "navy"

[output.html.fold]
enable = true
level = 1

[output.html.search]
enable = true
EOF
}

# Landing page, so the book opens on something other than the first report.
write_index() {  # <path> <region...>
  local f="$1"; shift
  {
    printf '# %s\n\n' "$TITLE"
    printf 'Summary reports of the data stamped by `seastamp` (distance to coast,\n'
    printf 'depth, sea area, place, nearest location), one page per\n'
    printf '(region, product) unit. Pick a report from the sidebar.\n\n'
    printf 'Regions in this build: %s.\n\n' "$*"
    printf 'Generated on %s from `%s`.\n' "$(date '+%Y-%m-%d')" "$SRC"
  } > "$f"
}

# README for the built site directory, so the published directory is a
# self-describing repository (shown by GitHub beside the served pages). The
# License section is emitted only when a LICENSE is being copied in, and the DOI
# badge and Citation section only when a CITATION.cff is.
write_site_readme() {  # <path> <region...>
  local f="$1"; shift
  local doi
  doi=$(citation_doi)
  {
    printf '# %s\n\n' "$TITLE"
    if [[ -n "$doi" ]]; then
      printf '[![DOI](https://zenodo.org/badge/DOI/%s.svg)](https://doi.org/%s)\n\n' "$doi" "$doi"
    fi
    printf 'A static web site of summary reports for data stamped by seastamp, one page per\n'
    printf '(region, product) unit, generated by the\n'
    printf '[`seastamp`](https://github.com/AIQC-Hub/seastamp) helper script\n'
    printf '`summary_site.sh` (built with [mdBook](https://rust-lang.github.io/mdBook/)).\n\n'
    printf 'This directory is the built site: open `index.html` locally, or publish\n'
    printf 'it as-is with GitHub Pages (a `.nojekyll` is included).\n\n'
    printf 'Regions in this build: %s.\n\n' "$*"
    printf 'Generated on %s.\n' "$(date '+%Y-%m-%d')"
    if [[ -n "$CITATION_SRC" ]]; then
      printf '\n## Citation\n\nSee [`CITATION.cff`](./CITATION.cff)'
      if [[ -n "$doi" ]]; then
        printf ', or cite <https://doi.org/%s>' "$doi"
      fi
      printf '.\n'
    fi
    if [[ -n "$LICENSE_SRC" ]]; then
      printf '\n## License\n\nSee [`LICENSE`](./LICENSE).\n'
    fi
  } > "$f"
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
  local r
  for r in "${regions[@]}"; do
    is_region "$r" || { echo "Unknown region: $r" >&2; usage; return 1; }
  done

  show_config "${regions[@]}"
  confirm || { log "aborted."; return 1; }

  # Assemble the book in a scratch directory; only the rendered site is kept.
  local work
  work=$(mktemp -d)
  # shellcheck disable=SC2064  # expand $work now, not at trap time
  trap "rm -rf '$work'" EXIT
  mkdir -p "$work/src"

  local summary="$work/src/SUMMARY.md"
  printf '# Summary\n\n[Introduction](./index.md)\n' > "$summary"

  local s label title n=0
  for r in "${regions[@]}"; do
    REGION="$r"
    label=$(region_label "$r")

    # Emit the part heading only once a region turns out to have pages, so a
    # region with none (e.g. one not yet run) leaves no empty section behind.
    local part_done=0
    for s in $(stems_for "$r"); do
      if [[ ! -f "$SRC/$s.md" ]]; then
        log "skip $s (no page in $SRC)"
        continue
      fi
      if [[ "$part_done" == 0 ]]; then
        printf '\n# %s\n\n' "$label" >> "$summary"
        part_done=1
      fi
      cp "$SRC/$s.md" "$work/src/$s.md"
      title=$(chapter_title "$SRC/$s.md" "$label")
      printf -- '- [%s](./%s.md)\n' "$title" "$s" >> "$summary"
      log "add $s -> $title"
      n=$((n + 1))
    done
  done
  REGION=""

  if [[ "$n" == 0 ]]; then
    echo "Error: no summary pages found in '$SRC'. Run summary_data.sh first (-f md)." >&2
    return 1
  fi

  write_index "$work/src/index.md" "${regions[@]}"
  if [[ -n "$CONFIG" ]]; then
    log "using custom config: $CONFIG"
    cp "$CONFIG" "$work/book.toml"
  else
    write_default_config "$work/book.toml"
  fi

  # mdbook resolves -d relative to the book root, so pass an absolute path.
  mkdir -p "$DEST"
  local dest_abs
  dest_abs=$(cd "$DEST" && pwd)
  log "building $n page(s) -> $dest_abs"
  mdbook build "$work" -d "$dest_abs" >&2

  # Make the built directory a self-contained, publishable repository. mdBook
  # already wrote .nojekyll; add the LICENSE, the CITATION.cff, and a README
  # describing the site.
  if [[ -n "$LICENSE_SRC" ]]; then
    cp "$LICENSE_SRC" "$dest_abs/LICENSE"
    log "added LICENSE (from $LICENSE_SRC)"
  fi
  if [[ -n "$CITATION_SRC" ]]; then
    cp "$CITATION_SRC" "$dest_abs/CITATION.cff"
    log "added CITATION.cff (from $CITATION_SRC)"
  fi
  write_site_readme "$dest_abs/README.md" "${regions[@]}"
  log "added README.md"

  log "done. Open $dest_abs/index.html"
}

main ${ARGS[@]+"${ARGS[@]}"}
