# seastamp

[![crates.io](https://img.shields.io/crates/v/seastamp.svg)](https://crates.io/crates/seastamp)
[![CI](https://github.com/AIQC-Hub/seastamp/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/AIQC-Hub/seastamp/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/AIQC-Hub/seastamp/graph/badge.svg)](https://codecov.io/gh/AIQC-Hub/seastamp)
[![CodeFactor](https://www.codefactor.io/repository/github/aiqc-hub/seastamp/badge)](https://www.codefactor.io/repository/github/aiqc-hub/seastamp)

A Rust CLI tool that stamps a table of `longitude`/`latitude` points with sea attributes: distance to coast, depth, sea area, nearest country and municipality, and the nearest of your own locations.

**Links:**

- 📦 [crates.io](https://crates.io/crates/seastamp)
- 📖 [Documentation](https://aiqc-hub.github.io/seastamp/)
- 🌊 [Live example report site](https://aiqc-hub.github.io/seastamp-report-example/index.html): sample output of the `summary_site.sh` pipeline phase

Five stamping commands each append columns to every input row:

| Command | Adds | Reference data |
|---------|------|----------------|
| `coast` | distance to the nearest shoreline | [GSHHG](https://www.soest.hawaii.edu/pwessel/gshhg/) |
| `depth` | bathymetric depth at the point | [GEBCO](https://www.gebco.net/) grid |
| `sea` | sea or ocean name | [IHO Sea Areas](https://www.marineregions.org/) |
| `place` | nearest country and municipality | [Natural Earth](https://www.naturalearthdata.com/) + [GISCO](https://ec.europa.eu/eurostat/web/gisco) |
| `nearest` | nearest location in a second table you supply, and the distance to it | none (for example measurements and fish farms) |

Three further commands stamp nothing: `regions` lists sea and ocean bounding
boxes, `completions` prints a shell completion script, and `report` summarises a
stamped file as a report table or a summary page.

It reads and writes Parquet (default), CSV, TSV, and the gzip variants `csv.gz`
and `tsv.gz`. Each stamping command reduces the input to unique rounded
locations, processes those in parallel, and joins the results back onto every
row, so a file with millions of rows but few distinct positions is cheap to
stamp. The [documentation site](https://aiqc-hub.github.io/seastamp/) gives the
algorithm and caveats per command.

## Installation

### Download a prebuilt binary (no Rust needed)

Grab an archive for your platform from the
[latest release](https://github.com/AIQC-Hub/seastamp/releases/latest). HDF5 and
netCDF are built into the binary, so nothing else has to be installed:

```bash
VERSION=v0.18.1
TARGET=x86_64-unknown-linux-gnu    # or aarch64-unknown-linux-gnu,
                                   # aarch64-apple-darwin, x86_64-apple-darwin
curl -LO "https://github.com/AIQC-Hub/seastamp/releases/download/$VERSION/seastamp-$VERSION-$TARGET.tar.gz"
tar -xzf "seastamp-$VERSION-$TARGET.tar.gz"
cd "seastamp-$VERSION-$TARGET" && ./seastamp --version
```

The Linux binaries are linked against glibc 2.17, so they run on RHEL / CentOS 7,
Rocky Linux 8, Ubuntu 18.04, and anything newer. The archive also carries the
helper scripts, so the pipeline works as soon as `seastamp` is on your `PATH`.

### Install from crates.io

Needs a Rust toolchain and the HDF5 / netCDF development headers, which the
`depth` command links against:

```bash
# Ubuntu / Debian
sudo apt-get install libhdf5-dev libnetcdf-dev
# macOS (Homebrew)
brew install hdf5

cargo install seastamp
```

The binary lands in `~/.cargo/bin`. To build a self-contained binary that
vendors those libraries instead (as the release archives do), add
`--features static-netcdf` (this needs `cmake`). See the
[installation guide](https://aiqc-hub.github.io/seastamp/installation.html) for
details.

### Build from source

Needs the same system dependencies as above.

```bash
git clone https://github.com/AIQC-Hub/seastamp.git
cd seastamp
cargo build --release
```

The binary is placed at `target/release/seastamp`.

## Commands

Every command and subcommand supports `-h` / `--help`.

The five stamping commands share these options:

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

### Regions and `--partition`

`coast`, `sea`, and `place` also take region options. `--region` accepts `auto`
(the default), a preset (`global`, `baltic`, `norway`, `arctic`, `atlantic`,
`europe`, `mediterranean`), or any of the 101 IHO Sea Areas names such as
`"Barentsz Sea"`, baked into the binary so no data file is needed. There are
also explicit `--min-lon/--max-lon/--min-lat/--max-lat` and
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
already fits one projection is left as one piece.

```bash
# two survey areas an ocean apart: coast's mean error goes from 8.4% to 0.2%
seastamp coast stations.parquet --data ./data/gshhg/... --partition

# and place stops assigning the wrong country to points in the open Pacific
seastamp place stations.parquet --countries ./data/naturalearth/... --partition
```

See [auto or partition](https://aiqc-hub.github.io/seastamp/reference/auto-or-partition.html)
for which to use when, and for the one case where partitioning trades one kind of
error for another.

### `coast`: distance to the nearest shoreline

```
seastamp coast [OPTIONS] --data <GSHHS_DIR> <input>
```

Nearest GSHHG shoreline by projected R-tree lookup, in kilometers (`--unit km`,
the default) or meters (`--unit m`). Resolution `f` is recommended.

```bash
seastamp coast cores.parquet \
  --data ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f \
  --unit km -o cores.coast.parquet
```

### `depth`: bathymetric depth from a GEBCO grid

```
seastamp depth [OPTIONS] --data <GEBCO.nc> <input>
```

GEBCO stores elevation (negative below sea level); `--positive` reports depth as
positive below sea level instead, and `--on-land` adds an `on_land` column.

```bash
# reading and writing gzipped CSV
seastamp depth cores.csv.gz --data ./data/gebco/GEBCO_2024_sub_ice_topo.nc \
  -o cores.depth.csv.gz
```

### `sea`: sea or ocean name

```
seastamp sea [OPTIONS] --data <IHO_SEA_AREAS> <input>
```

IHO point in polygon, with a nearest-polygon fallback for points just outside
every area.

```bash
# cropping the reference data to a named IHO sea
seastamp sea cores.parquet --region "Norwegian Sea" \
  --data ./data/iho/iho_sea_areas.geojson
```

### `place`: nearest country and municipality

```
seastamp place [OPTIONS] --countries <SHP> [--municipalities <SHP>] <input>
```

Nearest Natural Earth country, and with `--municipalities` the nearest GISCO LAU
municipality and the distance to it.

```bash
seastamp place cores.parquet \
  --countries ./data/naturalearth/ne_10m_admin_0_countries.shp \
  --municipalities ./data/gisco/lau.shp
```

### `nearest`: nearest location in a table you supply

```
seastamp nearest [OPTIONS] --to <TABLE> <input>
```

`--to` is the set of named locations to measure the distance to. Its coordinate
columns default to `longitude`/`latitude` (`--to-lon-col`/`--to-lat-col`) and
the name column to `name` (`--name-field`). Distances are great-circle (exact
anywhere on the globe, so this command has no region or projection center), in
kilometers by default or meters with `--unit m`.

```bash
# nearest fish farm to each measurement, distance in km
seastamp nearest cores.parquet --to farms.parquet \
  --name-field farm_name -o cores.nearest.parquet
```

### `regions`: list sea and ocean bounding boxes

```
seastamp regions [--name <TEXT>] [--data <IHO_SEA_AREAS>] [-o <FILE>]
```

Lists every name `--region` accepts, and with `--data` re-derives the boxes from
an IHO Sea Areas file.

```bash
seastamp regions --name "bering"
seastamp regions -o regions.parquet     # saved as a table
```

### `report`: summarise a stamped file

```
seastamp report stats   [--by COL] [--format tsv|text|json] <stamped file> [dest]
seastamp report summary [--format md|html] [-o <FILE>] <stem>
```

`report stats` summarises what a stamped file holds: rows and unique locations,
and for each stamped column the statistics, distance or depth bands, or the count
per name, optionally broken down per value of `--by`. `report summary` renders
that table as a Markdown or HTML page.

```bash
seastamp report stats cores.stamped.parquet report/stamp/cores.tsv
seastamp report stats --by platform_code -f text cores.stamped.parquet
seastamp report summary cores --format html -o cores.html
```

See the [report page](https://aiqc-hub.github.io/seastamp/commands/report.html)
for the table layout and the options.

### `completions`: shell tab-completion

```
seastamp completions <bash|zsh|fish|elvish|powershell>
```

Prints a completion script to stdout. Tab then fills in subcommands, flags, their
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

## Helper scripts

To run several modules over one input and get a single file with all their new
columns, use `scripts/stamp.sh`, which chains the selected modules and removes
the intermediate files:

```bash
scripts/stamp.sh cores.parquet cores.stamped.parquet \
  --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f \
  --depth ./data/gebco/GEBCO_2024_sub_ice_topo.nc \
  --nearest farms.parquet --nearest-name-field farm_name
```

To stamp every product of a ctddump pipeline at once, use
`scripts/stamp_data.sh`. It runs `stamp.sh` on each de-duplicated product of the
Arctic, Baltic, and Mediterranean seas in parallel, with the same module flags,
and writes a report per product with `seastamp report stats`, to
`report/stamp/<stem>.tsv` (`stamp_data.sh report` re-runs only that step):

```bash
scripts/stamp_data.sh -i ../ctddump/output/dedup/dedup -o output/stamp \
  --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f --partition \
  --depth ./data/gebco/GEBCO_2024_sub_ice_topo.nc
```

Two more scripts turn the reports into a web site, as ctddump's do (see the
[live example](https://aiqc-hub.github.io/seastamp-report-example/index.html)):

```bash
scripts/summary_data.sh -y        # summary/<stem>.md, one page per product
scripts/summary_site.sh -y        # site/index.html, built with mdBook
```

Every script takes `--help`. See the
[helper scripts page](https://aiqc-hub.github.io/seastamp/helper-scripts.html)
for the whole chain.

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

Run `scripts/download_data.sh --help` for all options, and see the
[data page](https://aiqc-hub.github.io/seastamp/data.html) for the sources.

## Development

```bash
# Run all tests
cargo test

# Run a single test
cargo test stats_counts_rows_and_locations
```

Releases are versioned, changelogged, and published following
[docs/dev/release.md](docs/dev/release.md).

## License

Licensed under the [MIT License](LICENSE).
