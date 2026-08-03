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
  regional-scale nearest-feature query. A single sphere (authalic radius) is
  used rather than the GRS80 ellipsoid; the error is well under coastline
  resolution for regional work. Sub-meter accuracy, if ever needed, means an
  ellipsoidal LAEA.
- The **unit sphere**, used by `nearest`. Reference points become
  `(x, y, z)` vectors on the unit sphere; nearest-by-chord equals
  nearest-by-great-circle, and the squared chord converts back to an exact
  great-circle distance in meters. This is why `nearest` needs no projection
  center and has none of a planar projection's distortion far from a center.

The `haversine_m` great-circle distance is used for reference and to refine
index candidates.

## How distances are calculated

Three commands report a distance, and they do not all compute it the same way.
Which method a command uses decides how far from the projection center its
numbers stay trustworthy.

| Column | Command | Method | Accurate |
|--------|---------|--------|----------|
| `dist_to_coast` | `coast` | Planar, in the region LAEA | Regionally, near the projection center |
| `municipality_dist` | `place` | Planar, in the region LAEA | Regionally, near the projection center |
| `nearest_dist` | `nearest` | Great circle, on the unit sphere | Everywhere on the globe |

All three compute in meters and divide by 1000 for the `km` default; `--unit m`
reports meters unchanged.

### Planar, in the region LAEA

`coast` and `place` project the query point and the reference geometry into the
region's LAEA plane and take an ordinary Euclidean distance there:

1. Project the point to `(x, y)` meters with the spherical LAEA centered on
   `--proj-lon0` / `--proj-lat0` (a `--region` preset sets these).
2. Ask the R-tree of projected segments for the nearest one.
3. Take the point-to-segment distance, which is a straight line in that plane.

For `coast` the segments are the shoreline; for `place` they are municipality
boundaries, and the distance is `0` when the point falls inside the polygon
rather than beside it.

Because the projection is centered on the region, the distortion is small near
that center and grows with distance from it. Planar distance tracks the true
great-circle distance to well under 1% for points a few tens of km apart in the
middle of a region, which is the case these commands are built for. **Set the
region to match your data**, since points far outside it get progressively worse
numbers rather than an error.

The projection is spherical, using the authalic radius `6371007.181` m, and the
formulas are the spherical LAEA case from Snyder, *Map Projections: A Working
Manual* (USGS Professional Paper 1395).

### Great circle, on the unit sphere

`nearest` deliberately does not use the projection, because the two tables it
compares can be anywhere and need not share a region. Instead:

1. Map every reference point to an `(x, y, z)` vector on the unit sphere, and
   index those in a 3D R-tree.
2. Ask for the nearest by Euclidean **chord** distance. The chord grows
   monotonically with the central angle, so the nearest by chord is exactly the
   nearest by great-circle distance.
3. Convert the squared chord back to meters with `d = 2R asin(chord / 2)`, using
   the mean radius `6371008.8` m.

The result is exact anywhere on the globe, so `nearest` takes no region or
projection center at all. It agrees with `haversine_m` to within 1e-9 relative
even across a third of the Earth.

### Limits

What this means in practice, and how to pick a region, is in
[Coverage and limits](./coverage.md).

Every distance here is spherical, so none of it is ellipsoidal-accurate. Expect
agreement with an ellipsoidal calculation at the level of a few parts in a
thousand, which is far below the resolution of the shoreline and boundary data
being measured against. Sub-meter work means replacing the spherical LAEA with an
ellipsoidal one.

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
