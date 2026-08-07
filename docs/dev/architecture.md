# Architecture

Single-stage `clap` dispatch:

1. `src/cli.rs`: the `Cli` / `Commands` structure and the flattened `CommonArgs`
   (input, output, format, columns, decimals, threads) and `RegionArgs`
   (bounding box + projection center) shared across commands.
2. `src/lib.rs`: `run(cli)` matches the command and calls the module's `run`.
3. `src/config/mod.rs`: `resolve(common, region)` merges the built-in default,
   the optional TOML config, and the CLI flags into a `Settings`. Precedence for
   the region box / projection center is
   `auto/preset/default < config file < CLI flag`. `src/config/iho_areas.rs` is
   the generated IHO name table; see [Regions](./regions.md).

## Pipeline (`src/pipeline.rs`)

The `Enricher` trait is the entire per-module surface. A module declares its
`outputs()` (column name + `Float`/`Text`/`Bool`) and computes
`enrich(lon, lat) -> Vec<Value>`. It may also override `parallel() -> false` to
be enriched on one thread; only `depth` does, for the
[HDF5 reason](./depth-hdf5.md).

`run_module` does the rest: extract `lon`/`lat` (cast to f64, nulls to NaN),
round and de-duplicate into unique locations (integer-scaled keys, so the join
never compares floats), enrich the unique set with rayon (or sequentially when
the module opts out), expand the results back to one value per input row, hstack
the new columns, and write. NaN coordinates get no key and therefore null output.

An output column already present in the input is an error (caught before
enrichment) unless `--overwrite` is set, which replaces it in place, keeping its
position.

`pipeline::locations` exposes the input's `(lon, lat)` pairs separately, because
`--region auto` needs them before any reference data is opened.

## I/O (`src/io.rs`)

`resolve_format` infers the format from the extension (Parquet fallback);
`read_frame` / `write_frame` handle all five formats. Gzip is done with `flate2`
(decompress to memory on read, wrap the writer on write) rather than a Polars
feature, so it behaves the same across Polars versions. Parquet writes use
`set_parallel(false)` for the same reason as ctddump.

## Geometry (`src/geo/`)

Pure Rust, no PROJ / GDAL, so downstream projects need no extra system libraries.

`Laea` is a spherical Lambert Azimuthal Equal-Area projection centered on the
region, giving distances the way a planar CRS such as EPSG:3035 would; planar
distance in that projection is accurate for the nearest-coast query at regional
scale. `haversine_m` is the great-circle distance used for reference and for
refining index candidates. Sub-meter accuracy, if ever needed, means an
ellipsoidal LAEA in place of the spherical one.

`src/geo/arc.rs` holds the wrap-aware longitude and direction math shared by
`regions` and `--region auto`: `covering_arc` (smallest arc covering a set of
intervals, found as the complement of the widest gap) and `spherical_center`
(mean direction by 3D unit vectors, returning the resultant length as a spread
measure). Both exist because longitude wraps and a min/max does not.

## Modules (`src/modules/`)

`coast`, `depth`, `sea`, `place`, `nearest` each build an `Enricher` from a data
source (a bundled-dataset path, or the `--to` table for `nearest`) and options,
then call `run_module`. `regions` and `completions` build no enricher at all.

Shared helpers: `default_output` (the `<stem>.<tag>.<ext>` fallback, where
`<ext>` matches the input format, so the output format defaults to the input's)
and `shp_polygons` (whole-polygon shapefile read used by `sea` and `place`).

See [Modules](./modules.md) for what each one does.

## Streaming (future)

`read_frame` currently loads the whole input to memory because the join touches
every row; the enrichment set itself is only the unique locations, so it is
always small. For very large inputs, the ctddump pattern applies: a first pass to
collect unique locations, then a second streamed pass that appends columns
`chunk`-by-`chunk` via a `BatchedWriter`. Note that as a caveat in the module
docs before implementing it.
