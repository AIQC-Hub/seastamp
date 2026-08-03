# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.1] - 2026-08-03

### Fixed

- `depth` no longer segfaults on inputs of more than a handful of points. It now
  looks up its grid cells on a single thread. The reads were already serialized
  behind a mutex, but that is not enough for HDF5: a build without thread safety
  cannot be entered from several threads at all, even when locking guarantees the
  calls never overlap, because it keeps state that assumes one thread of
  execution. Spreading the lookups over rayon workers therefore crashed, as a
  SIGSEGV in release builds and an error-stack assertion in debug ones. Enrichers
  declare this with a new `Enricher::parallel` returning `false`, and the shared
  pipeline honors it. Nothing is lost: one lock already made the reads sequential,
  so there was no parallelism to give up.

  The crash needed a serial HDF5 to appear, which is why it hit the prebuilt
  release binaries (they vendor the C libraries through `static-netcdf`, which
  leaves thread safety off) while builds against a thread-safe system
  `libhdf5-dev` were unaffected. That is also why the test suite never caught it,
  so `tests/depth.rs` now drives 2000 locations through the parallel path and
  documents how to run it against the vendored library. Reported against 0.8.0 on
  a 397 point input, where `-t 1` was the workaround; that workaround is no longer
  needed.

  One related tightening: `silence_hdf5_diagnostics` calls HDF5 directly, past the
  lock the `netcdf` crate holds for every netcdf-c call, so it now runs under the
  file mutex rather than before it.

## [0.9.0] - 2026-08-03

### Changed

- Renamed the project from `geoenrich` to `seastamp`. The name `geoenrich` was
  already taken by a Copernicus Marine tool that enriches species occurrence
  data with ocean variables, which is close enough to this tool's purpose to be
  confusing. The rename covers the crate, the binary, the library path used by
  `use seastamp::...`, the `[seastamp]` log prefix, the `SEASTAMP_BIN`
  environment variable read by `scripts/enrich.sh`, and the repository and
  documentation URLs. Commands, flags, configuration keys, and output column
  names are all unchanged, so existing invocations work by swapping the binary
  name. Users of the old crate should install `seastamp` instead of
  `geoenrich`; the `geoenrich` crate stops at 0.8.0.

## [0.8.0] - 2026-07-24

### Added

- Prebuilt binary archives attached to each GitHub release, for Linux and macOS
  on x86_64 and arm64. They bundle HDF5 and netCDF (a new `static-netcdf` Cargo
  feature vendors the C libraries), so they run with no system libraries, and
  the helper scripts ship inside each archive. The release workflow builds them
  and creates the GitHub release with notes from this changelog.

## [0.7.1] - 2026-07-24

### Added

- Crate `repository`, `homepage`, and `documentation` metadata, and an
  `exclude` that keeps the docs book, CI config, and internal notes out of the
  published tarball.
- Continuous integration on GitHub Actions (build and `cargo test` on push and
  pull request), and automated publishing to crates.io on a version tag via
  Trusted Publishing (OIDC, no stored token).

## [0.7.0] - 2026-07-23

### Added

- `scripts/enrich.sh`: runs several modules over one input in sequence and
  writes a single output file carrying every selected module's new columns. A
  module runs when you give its data source (`--coast`, `--depth`, `--sea`,
  `--countries`, `--nearest`), each step chains onto the previous one's output,
  and the intermediate files are removed on exit (keep them with `--keep`,
  preview the commands with `--dry-run`).

## [0.6.1] - 2026-07-23

### Added

- Project documentation site built with mdBook and published to GitHub Pages
  at <https://aiqc-hub.github.io/seastamp/>: an introduction, installation, a
  page per command, reference pages (regions, output columns, configuration,
  technical notes), and a reference-datasets page. A Pages workflow rebuilds
  and deploys it on every change to `docs/`.

## [0.6.0] - 2026-07-23

### Added

- New `atlantic` region preset (box -83, 20, -60, 70), covering the Atlantic
  basin from the Nordic Seas to the Southern Ocean.
- README now shows a latest-release badge that tracks the GitHub release
  automatically.

## [0.5.0] - 2026-07-23

### Added

- New `nearest` command: for each input point, find the closest location in a
  second table (`--to`) and append its name (`nearest_name`) and the distance
  to it (`nearest_dist`). The two sets can be anything (measurements and fish
  farms, stations and ports). The reference coordinate columns default to
  `longitude`/`latitude` (`--to-lon-col`/`--to-lat-col`) and the name column to
  `name` (`--name-field`); `--unit km|m` and `--name-column`/`--dist-column`
  rename the outputs. Distances are great-circle, computed with a unit-sphere
  R-tree, so they are exact anywhere on the globe with no region or projection
  center. Reference rows with a missing coordinate are skipped.

## [0.4.0] - 2026-07-23

### Added

- Three region presets for `--region`: `arctic` (north of 60N), `europe`, and
  `mediterranean`, alongside the existing `baltic`, `norway`, and `global`.

### Changed

- The default region is now `global` (the whole globe) instead of the Baltic
  Sea box. Pass `--region baltic` (or explicit bounds) for the previous default.

## [0.3.0] - 2026-07-23

### Added

- `scripts/download_data.sh`: downloads and unpacks the five reference
  datasets into `data/`, one sub-directory per source, matching the README
  example paths. Selected datasets download in parallel; existing archives
  are kept (`--force` re-downloads) and the multi-GB GEBCO grid resumes an
  interrupted download. The Marine Regions (IHO) download submits the site's
  form, so it needs `--mr-name`, `--mr-email`, and `--mr-country`, and it
  fails loudly when the form rejects the request instead of leaving a broken
  archive. The GISCO LAU bundle's EPSG 4326 (lon/lat) shapefile is unpacked
  from its nested zip, since seastamp needs lon/lat coordinates.
- New `--overwrite` flag on every command: when an output column already
  exists in the input, it is replaced in place (keeping its position and
  getting the output dtype) instead of the run failing. Without the flag a
  clashing column is still an error, now caught before enrichment starts and
  naming the column(s) and the flag.

### Changed

- The default output file (when `--output` is omitted) now keeps the input's
  format and extension: `points.csv.gz` enriches to `points.<command>.csv.gz`
  instead of `points.<command>.parquet`, with the whole `.csv.gz` suffix
  replaced (no stray `.csv` in the stem). Inputs with an unrecognized
  extension still default to Parquet, and an explicit `--output` or
  `--out-format` behaves as before.

## [0.2.0] - 2026-07-23

### Added

- `depth` module implemented against GEBCO gridded NetCDF: reads the `lat`/`lon`
  axes once, then maps each point to its nearest grid cell by arithmetic (O(1),
  no nearest-neighbor search) and reads the single `elevation` cell. Longitudes
  are normalized to `[-180, 180)` and off-grid points yield NaN. New `--positive`
  flag reports depth as positive below sea level. Reads link the system HDF5 /
  NetCDF libraries (`netcdf` crate).
- `coast` module implemented against GSHHG shorelines: boundary segments of the
  L1 (land/ocean) shapefile are cropped to the region box plus a 5 degree
  margin, projected through the region LAEA, and indexed in an `rstar` R-tree;
  each point gets the planar distance to the nearest segment in the chosen unit
  (`--unit km|m`). Segments are dropped, never clipped, so cropping cannot
  create artificial shoreline. `--data` accepts the `GSHHS_*_L1.shp` file or a
  GSHHG resolution directory containing one.
- `sea` module implemented against IHO Sea Areas (Marine Regions GeoJSON or
  shapefile): features are cropped whole to the region box plus margin, feature
  bounding boxes are indexed in an R-tree, and each point is resolved by an
  even-odd point-in-polygon test with a nearest-boundary fallback for points
  that fall just inland (fjords). New `--name-field` flag selects the name
  property (default `NAME`).
- `place` module implemented against Natural Earth countries and, optionally,
  GISCO LAU municipalities: both lookups resolve a point by containment first
  and nearest boundary otherwise, appending `country`, `country_code` (ISO
  alpha-3 where available; the Natural Earth `-99` placeholder becomes null),
  and `municipality`. Attribute fields are auto-detected from candidate lists,
  so minor schema drift between dataset versions needs no flags.
- Shared vector-geometry helpers in `geo::vector`: point-to-segment distance,
  tagged R-tree segments, even-odd point in polygon, and a `PolygonIndex`
  combining containment with a nearest-boundary fallback.

### Changed

- `depth` now requires `--data <GEBCO NetCDF file>`, `coast` and `sea` require
  `--data`, and `place` requires `--countries`; each errors clearly when its
  data source is omitted. With every module implemented, the scaffold stub
  notice is gone. `--municipalities` stays optional: without it the
  `municipality` column is empty and a note says so.

## [0.1.0] - 2026-07-22

### Added

- Project scaffold: `coast`, `depth`, `sea`, and `place` commands sharing one
  pipeline (read, de-duplicate rounded locations, enrich in parallel, join back,
  write).
- Multi-format I/O: Parquet (default), CSV, TSV, `csv.gz`, `tsv.gz`.
- Pure-Rust geometry: spherical LAEA projection and great-circle distance, with
  unit tests. No PROJ / GDAL dependency.
- Config resolution with a Baltic default, `--region` presets, and an optional
  TOML config file overridden by CLI flags.

### Not yet implemented

- The four modules' spatial lookups are stubs that emit NaN / empty values and
  print a notice; the reference-data readers (GSHHG, GEBCO, IHO, Natural Earth,
  GISCO) and their spatial indexes are pending.
