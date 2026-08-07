# Modules

What each module does and the caveats worth not rediscovering. Each module file's
header comment states its own algorithm; this is the cross-module view.

The scaffold (CLI, config resolution, multi-format I/O, and the shared pipeline
`pipeline::run_module`) and all five enrichment modules are implemented and
tested.

## depth (`src/modules/depth.rs`)

GEBCO NetCDF grid lookup keyed on `netcdf` (linking system HDF5). Nearest-cell by
arithmetic, HDF5 diagnostic silencing, and a `tests/depth.rs` integration test
that builds a small synthetic grid.

Enrichment is single-threaded here, unlike every other module: see
[HDF5 threading](./depth-hdf5.md), which is a hard rule.

`--on-land` adds a boolean column flagging elevations at or above sea level, read
off the raw elevation so `--positive` does not change its meaning.

## coast (`src/modules/coast.rs`)

GSHHG L1 shoreline segments cropped to the region plus a 5 degree margin,
projected through the region LAEA, indexed in an `rstar` R-tree; nearest-segment
planar distance in km or m. Segments are dropped, never clipped, so cropping
cannot create artificial shoreline.

`open` is `open_many` with a one-element slice, so there is a single crop-and-
project path. `open_many` exists for `--partition` and streams the shapefile
once, testing each segment against every crop in the batch; see
[regions.md](./regions.md) for why that matters.

## sea (`src/modules/sea.rs`)

IHO Sea Areas from GeoJSON or shapefile, features cropped whole, even-odd point
in polygon over R-tree bbox candidates with a nearest-boundary fallback for
points just inland. `open_many` serves `--partition` by reading the file once and
handing the features to `PolygonIndex::build_many`.

## place (`src/modules/place.rs`)

Natural Earth countries plus optional GISCO LAU municipalities, both resolved
containment-first with a nearest-boundary fallback; DBF attribute fields
auto-detected from candidate lists (the Natural Earth `-99` code placeholder
reads as missing). `read_features` parses both shapefiles and is shared by `open`
and by `open_many`, the `--partition` path.

With `--municipalities` it also appends `municipality_dist` (0 for a containment,
else the boundary distance via `PolygonIndex::locate_with_dist`), and
`--max-municipality-dist` drops matches past a limit, clearing the name and
distance together. That cutoff exists because GISCO LAU is Europe-only, so an
unbounded nearest match assigns a distant municipality to any site outside the
coverage.

## nearest (`src/modules/nearest.rs`)

Nearest point of a second table the caller passes with `--to` (not a bundled
dataset). Reference points are mapped to unit-sphere `(x, y, z)` and indexed in a
3D `rstar` R-tree; the nearest by Euclidean chord is the nearest by great-circle
distance, so the result is exact anywhere on the globe and the command takes no
region or projection center (unlike `coast`, which uses the region LAEA).

Appends `nearest_name` and `nearest_dist` (km or m). `geo::{unit_sphere,
chord2_to_m}` hold the sphere math; `tests/nearest.rs` checks it against
`haversine_m` cross-globe.

## regions (`src/modules/regions.rs`)

The odd one out. No `Enricher`, no `run_module`, no input table, no `CommonArgs` /
`RegionArgs`.

Without `--data` it lists everything `--region` accepts (`config::PRESET_NAMES`
then `config::IHO_AREAS`); with it, it reduces an IHO Sea Areas file (via
`sea::read_features`, shared for this) to one box per name. Prints on stdout,
optionally writes with `--output`. See [Regions](./regions.md) for the
antimeridian rule that governs its longitude extents.

## completions (`src/modules/completions.rs`)

The other odd one out, and the smaller of the two: `clap_complete::generate`
over `Cli::command()`, straight to stdout. Nothing to compute, and nothing to
keep in step by hand, since the script is derived from the parser's own
command tree.

The exception is `--region`, a plain `String` that clap would have no
candidates for. `cli::RegionNameParser` supplies `cli::region_names()` (109
names: `auto`, `PRESET_NAMES`, `IHO_AREAS`) through `TypedValueParser::
possible_values`, while `parse_ref` still accepts any string, so an unknown
region keeps reaching `config::region_bbox` and its word-level "did you mean".
The arg carries `hide_possible_values` to keep those 109 names out of `--help`;
`tests/completions.rs` pins both halves of that, since they pull in opposite
directions.

Known limitation, also pinned by a test: the bash and zsh generators emit
candidates as one space-separated list, so multi-word IHO names complete only to
their first word. fish is fine. `region_bbox`'s suggestion covers the gap.

## Shared geometry and test conventions

The shared vector geometry (point-to-segment distance, tagged R-tree segments,
even-odd point in polygon, and the containment-plus-nearest `PolygonIndex` used
by `sea` and `place`) lives in `src/geo/vector.rs` and is hand-rolled, so the
`geo` crate is not a dependency.

Geometry tests run against in-memory features (`from_rings` / `from_features`
constructors), so no large fixture files are committed; `tests/sea.rs` also
exercises the GeoJSON open path, and `tests/regions.rs` builds a small synthetic
GeoJSON covering the plain, antimeridian, and circumpolar cases.
