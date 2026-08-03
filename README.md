# seastamp

[![Latest release](https://img.shields.io/github/v/release/AIQC-Hub/seastamp?label=release)](https://github.com/AIQC-Hub/seastamp/releases/latest)

`seastamp` is a Rust command-line tool that adds geospatial attributes to a
table of points. Give it a file with `longitude` and `latitude` columns and it
appends any of:

- **coast**: distance to the nearest shoreline (GSHHG).
- **depth**: bathymetric depth at the point (GEBCO).
- **sea**: the sea or ocean name at the point (IHO Sea Areas).
- **place**: the nearest country and municipality (Natural Earth + GISCO).
- **nearest**: the nearest location in a second table you supply, and the
  distance to it (any two sets, for example measurements and fish farms).

It reads and writes Parquet (default), CSV, TSV, and the gzip variants `csv.gz`
and `tsv.gz`. Each command reduces the input to unique rounded locations,
processes those in parallel, and joins the results back onto every row, so a file
with millions of rows but few distinct positions is cheap to enrich.

> **Status:** all five modules are implemented and tested: `coast` (nearest
> GSHHG shoreline by projected R-tree lookup), `depth` (GEBCO grid lookup),
> `sea` (IHO point in polygon with a nearest fallback), `place` (nearest
> Natural Earth country and GISCO LAU municipality), and `nearest` (nearest
> point of a caller-supplied table by unit-sphere R-tree). See `CLAUDE.md` for
> the algorithm and caveats per module.

## Install

### Prebuilt binary

Each release attaches prebuilt archives for Linux and macOS (x86_64 and arm64)
to its [GitHub release](https://github.com/AIQC-Hub/seastamp/releases/latest).
They bundle HDF5 and netCDF, so they need no system libraries: download, unpack,
and run. The helper scripts ship inside the archive.

### From crates.io

```bash
cargo install seastamp
```

This builds the `depth` command against the system HDF5 / NetCDF libraries, so
install their dev headers first (see below).

### From source

```bash
cargo build --release
# binary at target/release/seastamp
```

The `depth` command reads GEBCO NetCDF and links the HDF5 / NetCDF C libraries,
so a source or `cargo install` build needs the dev headers (as with ctddump):

```bash
# Ubuntu / Debian
sudo apt-get install libhdf5-dev libnetcdf-dev
# macOS
brew install hdf5
```

To build a self-contained binary that vendors those libraries instead (as the
release archives do), add `--features static-netcdf` (this needs `cmake`).

## Usage

```bash
seastamp <command> <input> [options]
```

Every command shares these options:

| Option | Default | Meaning |
|--------|---------|---------|
| `-o, --output <FILE>` | `<stem>.<command>.<input format>` | Output file |
| `--in-format <FMT>` | inferred, else parquet | `parquet`, `csv`, `tsv`, `csv.gz`, `tsv.gz`, `auto` |
| `--out-format <FMT>` | inferred, else parquet | same set |
| `--overwrite` | off | Replace clashing output columns instead of failing |
| `--lon-col <NAME>` | `longitude` | Longitude column |
| `--lat-col <NAME>` | `latitude` | Latitude column |
| `--decimals <N>` | `3` | Rounding applied before de-duplicating |
| `-t, --threads <N>` | all cores | Worker threads. `depth` always looks up its grid on one thread |
| `-c, --config <TOML>` | none | Config file (CLI flags override it) |

The `coast`, `sea`, and `place` commands also take region options: a `--region`
preset (`global`, `baltic`, `norway`, `arctic`, `atlantic`, `europe`,
`mediterranean`) or explicit `--min-lon/--max-lon/--min-lat/--max-lat`, plus
`--proj-lon0/--proj-lat0` for the distance projection center. The default region
is the whole globe.

The `nearest` command instead takes a second table (`--to`), the set of named
locations to measure the distance to. Its coordinate columns default to
`longitude`/`latitude` (`--to-lon-col`/`--to-lat-col`) and the name column to
`name` (`--name-field`). Distances are great-circle (exact anywhere on the
globe, so this command has no region or projection center), in kilometers by
default or meters with `--unit m`.

### Examples

```bash
# Distance to coast, GSHHG resolution 'f', result in kilometers
seastamp coast cores.parquet \
  --data ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f \
  --unit km -o cores.coast.parquet

# Bathymetric depth from a GEBCO grid, reading and writing gzipped CSV
seastamp depth cores.csv.gz --data ./data/gebco/GEBCO_2024_sub_ice.nc \
  -o cores.depth.csv.gz

# Sea name, cropping the reference data to the Norway region
seastamp sea cores.parquet --region norway \
  --data ./data/iho/iho_sea_areas.geojson

# Nearest country and municipality
seastamp place cores.parquet \
  --countries ./data/naturalearth/ne_10m_admin_0_countries.shp \
  --municipalities ./data/gisco/lau.shp

# Nearest fish farm to each measurement, distance in km
seastamp nearest cores.parquet --to farms.parquet \
  --name-field farm_name -o cores.nearest.parquet
```

Run `seastamp <command> --help` for the full interface.

To run several modules over one input and get a single file with all their new
columns, use `scripts/enrich.sh`, which chains the selected modules and removes
the intermediate files:

```bash
scripts/enrich.sh cores.parquet cores.enriched.parquet \
  --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f \
  --depth ./data/gebco/GEBCO_2024_sub_ice.nc \
  --nearest farms.parquet --nearest-name-field farm_name
```

Run `scripts/enrich.sh --help` for all options.

## Output columns

| Command | Appended columns |
|---------|------------------|
| `coast` | `dist_to_coast` (rename with `--column`) |
| `depth` | `bathymetry` (rename with `--column`), plus `on_land` with `--on-land` |
| `sea`   | `sea_name` (rename with `--column`) |
| `place` | `country`, `country_code`, `municipality`, plus `municipality_dist` with `--municipalities` |
| `nearest` | `nearest_name`, `nearest_dist` (rename with `--name-column` / `--dist-column`) |

## Data

The reference datasets are downloaded separately (they are large and not
bundled). `scripts/download_data.sh` fetches and unpacks any of them into
`./data/`, one sub-directory per source, matching the example paths above:

```bash
# GSHHG, GEBCO, Natural Earth, and GISCO need no details
scripts/download_data.sh download gshhg gebco countries lau

# the Marine Regions (IHO) download sits behind a short form
scripts/download_data.sh --mr-name "Your Name" --mr-email you@example.org \
  --mr-country Norway download iho
```

Run `scripts/download_data.sh --help` for all options. Sources:

- GSHHG shorelines: https://www.soest.hawaii.edu/pwessel/gshhg/
- GEBCO bathymetry: https://www.gebco.net/
- IHO Sea Areas: https://www.marineregions.org/
- Natural Earth: https://www.naturalearthdata.com/
- Eurostat GISCO LAU: https://ec.europa.eu/eurostat/web/gisco

## License

MIT.
