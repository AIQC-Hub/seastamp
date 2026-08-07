# seastamp

[![Latest release](https://img.shields.io/github/v/release/AIQC-Hub/seastamp?label=release)](https://github.com/AIQC-Hub/seastamp/releases/latest)

`seastamp` is a Rust command-line tool that stamps a table of points with sea
attributes. Give it a file with `longitude` and `latitude` columns and it
appends any of:

- **coast**: distance to the nearest shoreline (GSHHG).
- **depth**: bathymetric depth at the point (GEBCO).
- **sea**: the sea or ocean name at the point (IHO Sea Areas).
- **place**: the nearest country and municipality (Natural Earth + GISCO).
- **nearest**: the nearest location in a second table you supply, and the
  distance to it (any two sets, for example measurements and fish farms).

Two further commands take no points: **regions** lists sea and ocean bounding
boxes, so you can find a region for the commands above, and **completions**
prints a shell completion script.

It reads and writes Parquet (default), CSV, TSV, and the gzip variants `csv.gz`
and `tsv.gz`. Each enrichment command reduces the input to unique rounded
locations, processes those in parallel, and joins the results back onto every
row, so a file with millions of rows but few distinct positions is cheap to
enrich.

> **Status:** all five enrichment modules are implemented and tested: `coast`
> (nearest GSHHG shoreline by projected R-tree lookup), `depth` (GEBCO grid
> lookup), `sea` (IHO point in polygon with a nearest fallback), `place`
> (nearest Natural Earth country and GISCO LAU municipality), and `nearest`
> (nearest point of a caller-supplied table by unit-sphere R-tree), plus
> `regions` for listing bounding boxes and `completions` for shell completion.
> See the [documentation site](https://aiqc-hub.github.io/seastamp/) for the
> algorithm and caveats per module.

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

### Shell completion

`seastamp completions <shell>` prints a completion script for `bash`, `zsh`,
`fish`, `elvish`, or `powershell`. Tab then fills in subcommands, flags, their
enumerated values, and every name `--region` accepts.

```bash
# bash
seastamp completions bash > ~/.local/share/bash-completion/completions/seastamp
# zsh (with ~/.zfunc on $fpath)
seastamp completions zsh > ~/.zfunc/_seastamp
# fish
seastamp completions fish > ~/.config/fish/completions/seastamp.fish
```

Start a new shell afterwards, and regenerate the script when you upgrade. See
the [completions page](https://aiqc-hub.github.io/seastamp/commands/completions.html)
for the details, including how to make bash list candidates on the first Tab.

## Usage

```bash
seastamp <command> <input> [options]
```

The five enrichment commands share these options (`regions` and `completions`
take none of them, having no input table):

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

The `coast`, `sea`, and `place` commands also take region options. `--region`
accepts `auto` (the default), a preset (`global`, `baltic`, `norway`, `arctic`,
`atlantic`, `europe`, `mediterranean`), or any of the 101 IHO Sea Areas names
such as `"Barentsz Sea"`, baked into the binary so no data file is needed. There
are also explicit `--min-lon/--max-lon/--min-lat/--max-lat` and
`--proj-lon0/--proj-lat0` for the distance projection center.

> **The region is where distances are measured from,** not just a crop. `coast`
> distances (and `place`'s `municipality_dist`) are accurate near the region's
> center and degrade away from it. `--region auto` handles this by deriving the
> box and the center from your own points, including around a pole and across
> the antimeridian, where no rectangle can put the center in the right place. It
> warns when the points are spread too widely for any one projection to serve
> them. `depth`, `nearest`, `sea`, and `place`'s `country` are unaffected and
> work anywhere. Municipalities are Europe only (GISCO LAU). See
> [Coverage and limits](https://aiqc-hub.github.io/seastamp/reference/coverage.html).

For data spread too widely for any one projection, `--partition` splits it into
sub-regions and measures each in its own, then joins the results back together.
The split is driven by accuracy rather than a cell size: seastamp keeps splitting
until nothing is more than 2% out and reports what it achieved, so data that
already fits one projection is left as one piece. It works on `coast`, `sea`, and
`place`.

```bash
# two survey areas an ocean apart: coast's mean error goes from 8.4% to 0.2%
seastamp coast stations.parquet --data ./data/gshhg/... --partition

# and place stops assigning the wrong country to points in the open Pacific
seastamp place stations.parquet --countries ./data/naturalearth/... --partition
```

See [auto or partition](https://aiqc-hub.github.io/seastamp/reference/auto-or-partition.html)
for which to use when, and for the one case where partitioning trades one kind of
error for another.

`seastamp regions` lists every name `--region` accepts, and with `--data`
re-derives the boxes from an IHO Sea Areas file:

```bash
seastamp regions --name "bering"
```

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

# Sea name, cropping the reference data to a named IHO sea
seastamp sea cores.parquet --region "Norwegian Sea" \
  --data ./data/iho/iho_sea_areas.geojson

# Nearest country and municipality
seastamp place cores.parquet \
  --countries ./data/naturalearth/ne_10m_admin_0_countries.shp \
  --municipalities ./data/gisco/lau.shp

# Nearest fish farm to each measurement, distance in km
seastamp nearest cores.parquet --to farms.parquet \
  --name-field farm_name -o cores.nearest.parquet

# Every region name seastamp accepts, saved as a table
seastamp regions -o regions.parquet
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

`regions` writes a standalone table instead: `name`, `min_lon`, `max_lon`,
`min_lat`, `max_lat`, `crosses_antimeridian`, `source`.

## Data

The reference datasets are downloaded separately (they are large and not
bundled). `scripts/download_data.sh` fetches and unpacks any of them into
`./data/`, one sub-directory per source, matching the example paths above:

```bash
# GSHHG, GEBCO, Natural Earth, and GISCO need no details
scripts/download_data.sh download gshhg gebco countries lau

# the Marine Regions (IHO) download sits behind a short form
scripts/download_data.sh --mr-name "Your Name" --mr-org "Your Institute" \
  --mr-email you@example.org --mr-country Norway download iho
```

Run `scripts/download_data.sh --help` for all options. Sources:

- GSHHG shorelines: https://www.soest.hawaii.edu/pwessel/gshhg/
- GEBCO bathymetry: https://www.gebco.net/
- IHO Sea Areas: https://www.marineregions.org/
- Natural Earth: https://www.naturalearthdata.com/
- Eurostat GISCO LAU: https://ec.europa.eu/eurostat/web/gisco

## License

MIT.
