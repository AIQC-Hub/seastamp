# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project Overview

`seastamp` is a Rust CLI that enriches a table of `longitude`/`latitude` points
with geospatial attributes, one per top-level command:

- `coast`: distance to the nearest shoreline (GSHHG shorelines).
- `depth`: bathymetric depth at the point (GEBCO gridded bathymetry).
- `sea`: sea / ocean name (IHO Sea Areas, point in polygon).
- `place`: nearest country and municipality (Natural Earth + Eurostat GISCO).
- `nearest`: nearest location in a caller-supplied second table, and the
  distance to it (any two sets, no bundled dataset).

It is a sibling to `ctddump` and follows the same house style, but is a separate
package on purpose: it must stay light and reusable across several downstream
projects, so it does not depend on `ctddump` and adds spatial dependencies only
as each algorithm needs them.

Input and output can be Parquet (default), CSV, TSV, and the gzip variants
`csv.gz` / `tsv.gz`. Every module reads the input, reduces it to unique locations
with rounded coordinates (3 decimals by default), enriches those unique locations
in parallel, then joins the results back onto every input row.

## Documentation style

Do not use em dashes in any human-facing text: `README.md`, `CHANGELOG.md`, docs,
generated output, help text, and log lines. Use a colon, comma, parentheses, a
semicolon, or a reworded sentence instead. (Carried over from `ctddump`.)

## Implementation status

The scaffold (CLI, config resolution, multi-format I/O, and the shared pipeline
`pipeline::run_module`) and all five modules are implemented and tested:

- `depth` (`src/modules/depth.rs`): GEBCO NetCDF grid lookup keyed on `netcdf`
  (linking system HDF5). Nearest-cell by arithmetic, serialized reads under a
  mutex (the system HDF5 serial build is not thread safe), per-thread HDF5
  diagnostic silencing, and a `tests/depth.rs` integration test that builds a
  small synthetic grid.
- `coast` (`src/modules/coast.rs`): GSHHG L1 shoreline segments cropped to the
  region plus a 5 degree margin, projected through the region LAEA, indexed in
  an `rstar` R-tree; nearest-segment planar distance in km or m. Segments are
  dropped, never clipped, so cropping cannot create artificial shoreline.
- `sea` (`src/modules/sea.rs`): IHO Sea Areas from GeoJSON or shapefile,
  features cropped whole, even-odd point in polygon over R-tree bbox candidates
  with a nearest-boundary fallback for points just inland.
- `place` (`src/modules/place.rs`): Natural Earth countries plus optional GISCO
  LAU municipalities, both resolved containment-first with a nearest-boundary
  fallback; DBF attribute fields auto-detected from candidate lists (the
  Natural Earth `-99` code placeholder reads as missing).
- `nearest` (`src/modules/nearest.rs`): nearest point of a second table the
  caller passes with `--to` (not a bundled dataset). Reference points are mapped
  to unit-sphere `(x, y, z)` and indexed in a 3D `rstar` R-tree; the nearest by
  Euclidean chord is the nearest by great-circle distance, so the result is
  exact anywhere on the globe and the command takes no region or projection
  center (unlike `coast`, which uses the region LAEA). Appends `nearest_name`
  and `nearest_dist` (km or m). `geo::{unit_sphere, chord2_to_m}` hold the
  sphere math; `tests/nearest.rs` checks it against `haversine_m` cross-globe.

The shared vector geometry (point-to-segment distance, tagged R-tree segments,
even-odd point in polygon, and the containment-plus-nearest `PolygonIndex` used
by `sea` and `place`) lives in `src/geo/vector.rs` and is hand-rolled, so the
`geo` crate is not a dependency. Each module file's header comment states its
algorithm and caveats. Geometry tests run against in-memory features
(`from_rings` / `from_features` constructors), so no large fixture files are
committed; `tests/sea.rs` also exercises the GeoJSON open path.

## Commands

```bash
cargo build
cargo test                         # unit + integration tests
cargo run -- <command> --help      # discover any command's interface
cargo run -- coast input.parquet --data ./data/gshhg/.../GSHHS_shp/f
```

The full CLI is defined with `clap` in `src/cli.rs` and is self-documenting via
`--help` at every level.

## Architecture

Single-stage `clap` dispatch:

1. `src/cli.rs`: the `Cli` / `Commands` structure and the flattened `CommonArgs`
   (input, output, format, columns, decimals, threads) and `RegionArgs`
   (bounding box + projection center) shared across commands.
2. `src/lib.rs`: `run(cli)` matches the command and calls the module's `run`.
3. `src/config.rs`: `resolve(common, region)` merges the built-in default, the
   optional TOML config, and the CLI flags into a `Settings`. Precedence for the
   region box / projection center is `preset/default < config file < CLI flag`.

**Pipeline** (`src/pipeline.rs`): the `Enricher` trait is the entire per-module
surface. A module declares its `outputs()` (column name + `Float`/`Text`) and
computes `enrich(lon, lat) -> Vec<Value>`. `run_module` does the rest: extract
`lon`/`lat` (cast to f64, nulls to NaN), round and de-duplicate into unique
locations (integer-scaled keys, so the join never compares floats), enrich the
unique set with rayon, expand the results back to one value per input row, hstack
the new columns, and write. NaN coordinates get no key and therefore null output.
An output column already present in the input is an error (caught before
enrichment) unless `--overwrite` is set, which replaces it in place, keeping its
position.

**I/O** (`src/io.rs`): `resolve_format` infers the format from the extension
(Parquet fallback); `read_frame` / `write_frame` handle all five formats. Gzip is
done with `flate2` (decompress to memory on read, wrap the writer on write) rather
than a Polars feature, so it behaves the same across Polars versions. Parquet
writes use `set_parallel(false)` for the same reason as ctddump.

**Geometry** (`src/geo/`): pure Rust, no PROJ / GDAL, so downstream projects need
no extra system libraries. `Laea` is a spherical Lambert Azimuthal Equal-Area
projection centered on the region (the reference R workflow used EPSG:3035 LAEA
for distances); planar distance in that projection is accurate for the
nearest-coast query at regional scale. `haversine_m` is the great-circle distance
used for reference and for refining index candidates. Sub-meter accuracy, if ever
needed, means an ellipsoidal LAEA in place of the spherical one.

**Modules** (`src/modules/`): `coast`, `depth`, `sea`, `place`, `nearest`. Each
builds its `Enricher` from a data source (a bundled-dataset path, or the `--to`
table for `nearest`) and options and calls `run_module`. Shared helpers:
`default_output` (the `<stem>.<tag>.<ext>` fallback, where `<ext>` matches the
input format, so the output format defaults to the input's) and `shp_polygons`
(whole-polygon shapefile read used by `sea` and `place`).

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

`scripts/download_data.sh` (ctddump-style bash: the header comment doubles as
`--help`, `log`/`run` tracing, a confirm prompt, parallel per-dataset workers)
downloads and unpacks any of the five sources into `data/`, one sub-directory
each, matching the README example paths. Caveats baked into it: the GEBCO grid
is multi-GB and resumes an interrupted download; the GISCO LAU bundle nests one
zip per projection and only the EPSG 4326 (lon/lat) layer is unpacked, since
the modules expect lon/lat; the Marine Regions (IHO) download submits the
site's statistics form, so it requires `--mr-name` / `--mr-email` /
`--mr-country` (and posts back the form's hidden anti-bot field empty), and it
verifies the response is a zip, failing loudly when the form rejects it.

`scripts/enrich.sh` (same ctddump-style bash: header doubles as `--help`,
`log`/`run` tracing) chains several modules over one input, each reading the
previous step's output so their columns accumulate into a single final file. A
module runs when its data flag is given (`--coast`, `--depth`, `--sea`,
`--countries`, `--nearest`); intermediates are Parquet in a `mktemp -d` dir
removed by an EXIT trap (`--keep` to retain, `--dry-run` to preview). The trap
and the temp-dir variable are global on purpose: a `local` in `main` is out of
scope when the EXIT trap fires under `set -u`. The last module writes the final
output, whose format follows its extension.

## Regions

The default region is the whole globe. Other regions come from `--region`
presets (`global`, `baltic`, `norway`, `arctic`, `atlantic`, `europe`,
`mediterranean`) or explicit `--min-lon/--max-lon/--min-lat/--max-lat` and
`--proj-lon0/--proj-lat0`.
The Baltic box (8, 31, 53, 66) matches the R examples and is the `baltic` preset.
Add presets in `config::preset_bbox`. The `place` municipality lookup will also
need a per-region country list (the R snippet's ISO3 set).

## Streaming (future)

`read_frame` currently loads the whole input to memory because the join touches
every row; the enrichment set itself is only the unique locations, so it is
always small. For very large inputs, the ctddump pattern applies: a first pass to
collect unique locations, then a second streamed pass that appends columns
`chunk`-by-`chunk` via a `BatchedWriter`. Note that as a caveat in the module docs
before implementing it.

## Git Workflow

Match ctddump: permanent `main` (stable) and `develop` (integration) branches;
day-to-day work on `develop`. Commit and push only when the user asks. Commit
messages end with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

## CI and releases

Two GitHub workflows, both modeled on ctddump. `ci.yml` builds and runs
`cargo test` on push and PR to `main` (installs libhdf5-dev / libnetcdf-dev for
`depth`, strips debuginfo so the statically linked Polars test binaries fit the
runner disk). `publish.yml` fires on a `v*` tag: it re-runs the tests, then in
parallel (a) publishes to crates.io via Trusted Publishing (OIDC, no stored
token; the tag must match `Cargo.toml`), and (b) builds prebuilt binaries for
Linux and macOS (x86_64 and arm64) with `--features static-netcdf`
(`netcdf/static`, vendoring HDF5 / netCDF via cmake) and creates the GitHub
release, attaching the archives and `SHA256SUMS`, with notes extracted from the
matching `CHANGELOG.md` section. Because the workflow now creates the release,
do not also create it by hand for a tagged release. `Cargo.lock` is committed
(the workflow uses `--locked`), so bump it alongside the version.
