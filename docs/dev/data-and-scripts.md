# Data sources and helper scripts

Everything in `scripts/` is mode 755 in git and must stay that way: `publish.yml`
copies the directory verbatim into each release archive, so a file committed 644
ships unrunnable. Add a new script with `git update-index --chmod=+x`, since a
local `chmod` alone does not reach the index.

## Data sources (not bundled)

The reference datasets are large and are not committed or shipped. A module takes
its data path by flag (`--data`, or `--countries` / `--municipalities` for
`place`). Sources:

- GSHHG shorelines: https://www.soest.hawaii.edu/pwessel/gshhg/ (ESRI shapefiles,
  resolution `f`).
- GEBCO bathymetry: https://www.gebco.net/ (gridded NetCDF; the depth module
  links HDF5 via the `netcdf` crate, same system dependency as ctddump).
- IHO Sea Areas v3: Marine Regions (https://www.marineregions.org/), GeoJSON or
  shapefile.
- Natural Earth countries: https://www.naturalearthdata.com/.
- Eurostat GISCO LAU (municipalities): https://ec.europa.eu/eurostat/web/gisco.

## scripts/download_data.sh

ctddump-style bash: the header comment doubles as `--help`, `log`/`run` tracing,
a confirm prompt, parallel per-dataset workers. Downloads and unpacks any of the
five sources into `data/`, one sub-directory each, matching the README example
paths.

Caveats baked into it:

- The GEBCO grid is multi-GB and resumes an interrupted download. The zip is
  saved as `gebco_<year>_sub_ice_topo.zip` to match the `.nc` inside it; up to
  0.17.1 it was `gebco_<year>_sub_ice.zip`, and `download_gebco` renames such a
  zip (or its `.part`) in place, since `fetch` decides whether to skip by the
  file name. Drop that fallback once nobody has the old name.
- The GISCO LAU bundle nests one zip per projection, and only the EPSG 4326
  (lon/lat) layer is unpacked, since the modules expect lon/lat.
- The Marine Regions (IHO) download submits the site's statistics form, so it
  requires every field the form marks required (`--mr-name` / `--mr-org` /
  `--mr-email` / `--mr-country`, plus `--mr-category` and `--mr-purpose`, which
  default to `academia` and `Research`), posts back the form's hidden anti-bot
  field empty, and verifies the response is a zip, failing loudly when the form
  rejects it. The two dropdowns accept only fixed values, held in
  `MR_CATEGORIES` / `MR_PURPOSES` and checked by `check_mr` before any download
  starts; re-scrape them from the form page if Marine Regions changes them.
  Because the details go to a third party and the download accepts
  CC BY-NC-SA 4.0, `show_config` prints them for confirmation first.

## scripts/stamp.sh

Same ctddump-style bash: header doubles as `--help`, `log`/`run` tracing. Chains
several modules over one input, each reading the previous step's output so their
columns accumulate into a single final file.

`region_args` emits `--partition` or `--region`, never both, and only for coast,
sea, and place. The script rejects the pair itself rather than letting clap do
it, so the run fails before the first module instead of part way through a chain.

A module runs when its data flag is given (`--coast`, `--depth`, `--sea`,
`--countries`, `--nearest`); intermediates are Parquet in a `mktemp -d` dir
removed by an EXIT trap (`--keep` to retain, `--dry-run` to preview). The trap
and the temp-dir variable are global on purpose: a `local` in `main` is out of
scope when the EXIT trap fires under `set -u`. The last module writes the final
output, whose format follows its extension.

The trap body is an `if`, not `[[ ... ]] && rm`. A script that falls off its
end takes the EXIT trap's last status as its own, so the `&&` form made every
single-module, `--keep`, or `--dry-run` run exit 1 after succeeding.
`stamp_data.sh` checks that status, so keep it an `if`.

`scripts/enrich.sh` is the pre-0.16 name, kept as a three-line `exec` wrapper.
Keep it that thin: `usage()` here reads its own header comment out of `$0`, so
anything the wrapper prints for `--help` would be the wrapper's own header rather
than this script's. Delete it once the deprecation has had a release or two.

## scripts/stamp_data.sh

The batch form of `stamp.sh`, modelled on ctddump's `dedup_data.sh`: the same
region list, `stems_for` table, `show_config` / `confirm`, and
`--by-region` / `--sequential` dispatch (one background worker per file by
default). It reads ctddump's final stage, `output/dedup/dedup/<stem>.parquet`,
and runs the sibling `stamp.sh` once per stem. If ctddump adds a product or
renames a stem, update `stems_for` here too.

- Option forwarding is explicit. `STAMP_VALUE_OPTS` and `STAMP_FLAG_OPTS` mirror
  the `stamp.sh` parser; without them an option's value would be read as a
  region. An option in neither list is rejected. Add a new `stamp.sh` option to
  one of them.
- `preflight` runs `stamp.sh --dry-run` on the first input before any worker
  starts, so `--region` with `--partition`, no module selected, or a data path
  that does not exist is reported once instead of nine times. The path check
  lives in `stamp.sh` (it runs in `--dry-run` too) and is what makes the last of
  those reach the preflight; a first real run lost minutes of `coast` work per
  product to a mistyped GEBCO name before it existed.
- `tag_lines` puts `[stem]` after the timestamp of each `stamp.sh` line so
  workers running side by side stay readable. It is a `read` loop rather than
  `sed -u`, which BSD sed lacks. `pipefail` carries the exit status through
  that pipe.
- `--threads` defaults to 2, as in ctddump, because nine processes each taking
  every core would oversubscribe the machine.
- A leading command (`stamp`, `report`, `all`) picks the steps, as in ctddump.
  `report` reads `<out>/<stem>.<format>` instead of the input, so it skips the
  preflight and needs no module flags. The report step runs in the same worker,
  right after the stamp step, and is skipped with it.
- `REPORT_VALUE_OPTS` / `REPORT_FLAG_OPTS` pick out the forwarded options that
  also go to `seastamp report stats` (the units, `--depth-positive`, the
  coordinate columns, `--decimals`), collected in `RPASS`. They work because
  `report stats` uses the same flag names as `stamp.sh`; keep it that way.
  `--bin` is read here as well as forwarded, since the report step calls
  seastamp itself.
- Running `depth` in several workers is fine: the HDF5 rule (see
  [depth-hdf5.md](./depth-hdf5.md)) is about threads within one process, and
  each worker is a separate process opening the grid read-only.

## scripts/summary_data.sh and scripts/summary_site.sh

Ports of ctddump's scripts of the same names. `summary_data.sh` runs
`seastamp report summary` per stem with a title and notes from its "Page text"
section; the titles are ctddump's, so the two sites name products alike.
`summary_site.sh` is ctddump's almost line for line (only the wording and the
default title differ), so a fix to one usually applies to the other. It needs
mdBook and assembles the book in a `mktemp -d` dir.

## scripts/gen_iho_areas.py

Regenerates `src/config/iho_areas.rs`, the baked-in IHO name table that
`--region "<sea name>"` resolves against, from `seastamp regions` output:

```bash
seastamp regions --data <IHO shapefile> -q -o areas.csv
scripts/gen_iho_areas.py areas.csv > src/config/iho_areas.rs
```

That generated file is never edited by hand. See [Regions](./regions.md).
