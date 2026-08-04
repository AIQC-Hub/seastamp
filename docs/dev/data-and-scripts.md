# Data sources and helper scripts

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

- The GEBCO grid is multi-GB and resumes an interrupted download.
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

## scripts/enrich.sh

Same ctddump-style bash: header doubles as `--help`, `log`/`run` tracing. Chains
several modules over one input, each reading the previous step's output so their
columns accumulate into a single final file.

A module runs when its data flag is given (`--coast`, `--depth`, `--sea`,
`--countries`, `--nearest`); intermediates are Parquet in a `mktemp -d` dir
removed by an EXIT trap (`--keep` to retain, `--dry-run` to preview). The trap
and the temp-dir variable are global on purpose: a `local` in `main` is out of
scope when the EXIT trap fires under `set -u`. The last module writes the final
output, whose format follows its extension.

## scripts/gen_iho_areas.py

Regenerates `src/config/iho_areas.rs`, the baked-in IHO name table that
`--region "<sea name>"` resolves against, from `seastamp regions` output:

```bash
seastamp regions --data <IHO shapefile> -q -o areas.csv
scripts/gen_iho_areas.py areas.csv > src/config/iho_areas.rs
```

That generated file is never edited by hand. See [Regions](./regions.md).
