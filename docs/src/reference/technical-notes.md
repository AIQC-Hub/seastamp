# Technical notes

## The shared pipeline

Every command implements one small trait (declare the output columns, compute
their values for one location) and shares the rest: extract `longitude` and
`latitude` (cast to float, nulls to NaN), round and de-duplicate into unique
locations with integer-scaled keys (so the join never compares floats), enrich
the unique set in parallel with rayon, expand the results back to one value per
input row, append the columns, and write. A NaN coordinate gets no key and
therefore null output.

## Geometry: no PROJ, no GDAL

All geometry is hand-rolled in pure Rust, so downstream projects need no extra
system libraries. Two projections do the work:

- A spherical **LAEA** (Lambert Azimuthal Equal-Area) centered on the region,
  used for planar distances by `coast` (and for the nearest-boundary fallbacks
  in `sea` and `place`). Planar distance in that projection is accurate for the
  regional-scale nearest-feature query. The reference R workflow used EPSG:3035
  LAEA for the same reason. A single sphere (authalic radius) is used rather
  than the GRS80 ellipsoid; the error is well under coastline resolution for
  regional work. Sub-meter accuracy, if ever needed, means an ellipsoidal LAEA.
- The **unit sphere**, used by `nearest`. Reference points become
  `(x, y, z)` vectors on the unit sphere; nearest-by-chord equals
  nearest-by-great-circle, and the squared chord converts back to an exact
  great-circle distance in meters. This is why `nearest` needs no projection
  center and has none of a planar projection's distortion far from a center.

The `haversine_m` great-circle distance is used for reference and to refine
index candidates.

## Spatial indexes

Nearest-feature and point-in-polygon queries use `rstar` R-trees:

- `coast` indexes projected shoreline **segments**; a query takes the nearest
  segment's planar distance.
- `sea` and `place` index feature **bounding boxes** for the containment test
  and boundary **segments** for the nearest fallback.
- `nearest` indexes reference **points** in 3D on the unit sphere.

Cropping keeps whole features: a feature is dropped if its bounding box misses
the region-plus-margin box, but it is never clipped, so containment stays exact
and cropping cannot invent geometry.

## Memory and streaming

The input is read whole into memory because the join back touches every row; the
enrichment set itself is only the unique locations, which stays small. For very
large inputs a streamed two-pass version (collect unique locations, then append
columns chunk by chunk) is the natural next step, noted in the source.

## Threading and HDF5

Enrichment normally spreads the unique locations across rayon workers. `depth` is
the exception and runs them on one thread, because it reads through HDF5, which
is frequently built serial. A serial build is not merely unsafe for overlapping
calls: it cannot be entered from several threads at all, even under a lock that
makes the calls strictly sequential. Spreading the reads across workers therefore
crashed, as a segfault in release builds and an error-stack assertion in debug
ones. An `Enricher` declares this with `parallel() -> false`.

This only bites where HDF5 lacks thread safety, which is why it showed up in the
prebuilt release binaries (they vendor the C libraries through `static-netcdf`,
which leaves thread safety off) and not against a distribution `libhdf5-dev`
built thread-safe. To exercise that configuration:

```bash
cargo test --features static-netcdf --test depth
```

For the same reason all the depth cases live in a single `#[test]`: the harness
runs each test on its own thread, which is enough to trip a serial HDF5.

## Parquet writes

Parquet is written single-threaded (`set_parallel(false)`), matching `ctddump`:
the parallel column encoder in the pinned Polars version leaks memory per call,
and the single-thread path is safe and deterministic.
