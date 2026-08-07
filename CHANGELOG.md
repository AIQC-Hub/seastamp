# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.13.0] - 2026-08-07

### Added

- `seastamp completions <shell>`, which prints a shell completion script for
  bash, zsh, fish, elvish, or PowerShell. Tab completes subcommands, flags,
  enumerated values such as `--in-format`, and paths (as files, or as a
  directory for `coast --data`).

  `--region` completes from all 109 accepted names: `auto`, the seven presets,
  and the 101 IHO Sea Areas. They reach the shell through a value parser that
  advertises them without restricting what `--region` accepts, so an unknown
  name still reaches the word-level "did you mean" rather than a flat clap
  rejection. The same names are hidden from `--help`, where they would bury
  every other flag.

  In bash and zsh a name containing a space completes only to its first word,
  because both shells receive the candidates as one space-separated list. It
  fails loudly: `--region Barentsz` reports "Did you mean: Barentsz Sea?". fish
  completes such names whole.

### Fixed

- The `--region` help text offered `"Barents Sea"` as its example of an IHO Sea
  Areas name. IHO v3 spells it `Barentsz Sea`, so the example was one of the few
  strings the flag rejects.

## [0.12.0] - 2026-08-04

### Added

- `--region auto`, which derives the region from the input points once they are
  read: the crop box from their extent, and the projection center from their
  mean direction computed in three dimensions. Two cases no region box can
  express now work.

  Around a pole, a ring of stations at 75 N spanning every longitude centers on
  the pole itself, where a rectangle's center is forced down to the middle of
  its latitude band. Worst-case distortion drops from -3.4% under the `arctic`
  preset to -0.9%.

  Across the antimeridian, the projection has no seam: only the rectangular crop
  box does. `auto` separates the two, keeping a correct center and widening the
  crop to every longitude. Measured on three points around Fiji, `dist_to_coast`
  came out 7.31, 13.03, and 50.84 km against 1.23, 5.41, and 8.23 km under the
  old whole-globe default, wrong there by up to a factor of six.

  When the points are spread over too much of the globe for any projection to
  serve them, `auto` says so rather than returning a plausible wrong number. The
  measure is the length of the mean direction vector, 1.00 for points in one
  place and near 0 for points spread evenly over the globe. A polar ring scores
  0.97 despite spanning every longitude, which is the point.

- `--region` accepts any of the 101 IHO Sea Areas v3 names, for example
  `--region "Barentsz Sea"`, baked into the binary from a generated table so no
  data file is needed. `scripts/gen_iho_areas.py` regenerates it from
  `seastamp regions --data <IHO>` output.

  Four of them cannot be used by name, because their extent crosses the
  antimeridian and no box can express it: the Bering Sea, the Chukchi Sea, and
  the North and South Pacific Oceans. Naming one is an error that points at
  `--region auto`, which handles data in those areas correctly.

  `seastamp regions` now lists presets and IHO names together, from the baked-in
  table, so it needs no `--data` to show the full vocabulary. A `source` column
  says where each row came from. `--data` still re-derives the boxes from a
  file, which is what a newer IHO release would need.

- A `regions` command that lists sea and ocean bounding boxes: every name
  `--region` accepts, or, with `--data`, one box per named area in an IHO Sea
  Areas file. `--name` filters by substring and `--output` writes the table
  (`name`, `min_lon`, `max_lon`, `min_lat`, `max_lat`, `crosses_antimeridian`,
  `source`) in any supported format.

  The presets are mostly European, which left users elsewhere with no way to
  discover a sensible region short of reading a map. Since the region also sets
  where `coast` measures distances from, that was not a cosmetic gap. IHO Sea
  Areas v3 yields 101 areas, of which four cross the antimeridian and two (the
  Arctic and Southern Oceans) circle the globe.

  `regions` takes no input table and runs no enrichment pipeline, so it shares
  neither the common nor the region options with the other five commands.

  The longitude extent is the smallest arc covering the area's polygon edges,
  not the minimum and maximum of its vertices. Marine Regions splits its
  polygons at the antimeridian, so a Pacific area has vertices at both -180 and
  180, which a plain minimum and maximum would report as spanning the globe.
  Working from edges rather than vertices also keeps a long edge with no
  intermediate vertex from reading as a gap, which four contiguous parts tiling
  the globe would otherwise trip over.

### Changed

- **The default region is now `auto` rather than the whole globe.** Distances
  from `coast` and `place`'s `municipality_dist` will change for any run that
  did not set a region, and they change toward being correct: a three-point
  survey off Bergen went from 44.09, 0.423, and 77.65 km to 40.97, 0.400, and
  69.41 km, the old default having been about 8% out. Pass `--region global` for
  the previous behavior. `depth`, `nearest`, `sea`, and `place`'s `country` are
  unaffected, since none of them projects.

  Giving explicit bounds without a `--region` name also leaves you in auto mode
  now, so the box is yours and the center is still derived from the data. Name a
  region, or pass `--proj-lon0` / `--proj-lat0`, to pin the center.

- Reworded how the project describes itself, which still read as it did under
  the old `geoenrich` name: a tool that "adds geospatial attributes". That named
  neither this package nor what makes it distinct. The crate description, the
  `seastamp --help` header, the crate-level documentation that forms the docs.rs
  front page, the README, the documentation site introduction and its metadata,
  and the CLAUDE.md overview now agree on stamping points with sea attributes.
  Wording only: no command, flag, or output changed.

- The crate description also lists `nearest` at last. It has found the closest
  location in a caller-supplied table since 0.5.0, but the description still
  named only the other four commands.

### Fixed

- An unknown `--region` name is an error instead of a silent fall back to the
  whole globe, which used to turn a typo into quietly mismeasured distances. The
  error suggests near misses: `unknown region 'Barent Sea'. Did you mean:
  Barentsz Sea?`

## [0.11.0] - 2026-08-04

### Added

- `scripts/download_data.sh` asks for the Marine Regions details it needs
  instead of only refusing to continue. A missing name, organisation, email, or
  country is prompted for, and the user category and purpose are offered as
  numbered menus, so their exact spellings (`civil society`,
  `Data exploration & testing`) do not have to be typed or looked up. This makes
  `scripts/download_data.sh download iho` a workable command on its own.

  Prompting needs a terminal to ask at, so it is skipped when stdin is not one,
  and under `-y/--yes`, which asks to start immediately. Both keep the previous
  behavior of failing and naming the missing options, which is what a CI run or
  a pipeline needs. The answers are echoed in the confirmation summary before
  anything is submitted, as before.

- `--mr-email` is checked for an `@` and a dot before use. The form's field is
  `type=email` and rejects a malformed address with an HTML page, which is a
  confusing way to find out about a typo.

### Added

- seastamp warns when the input sits far from the projection center. `coast` and
  `place` measure distance in a plane centered on the region, which is accurate
  near that center and degrades away from it, and the default region is the whole
  globe, centered on (0, 0). A run without a matching region therefore returned
  quietly wrong distances: about 12% out in the North Sea, over 60% in the
  Pacific. The warning states the distance, the center, the approximate error,
  and what to pass instead, and it only fires past 2% so a run whose region fits
  its data stays silent. Modules declare this with a new
  `Enricher::projection_center`; `depth` and `nearest` return `None`, since
  neither uses a projection.

- A region box crossing the antimeridian is now rejected with an explanation.
  Longitude comparisons do not wrap, so such a box matched no reference feature
  and every row came back null with nothing said. A reversed latitude box is
  rejected too.

- `coast`, `sea`, and `place` say so when cropping to the region leaves no
  reference features, instead of returning an empty column for every row. For
  municipalities the message notes that GISCO LAU is Europe-only, which is the
  usual reason.

- New documentation page, Coverage and limits, stating which commands work
  anywhere (`depth`, `nearest`, `sea`, and `place`'s `country`), which need the
  region set first (`coast`, and `municipality_dist`), and which is Europe-only
  (`municipality`), with the size of the projection error by location, the
  antimeridian limitation, and the accuracy ceiling. Linked from the README and
  the regions and technical-notes pages.

### Changed

- The technical notes gained a section on how each distance is calculated: which
  commands measure in the region's LAEA plane (`dist_to_coast`,
  `municipality_dist`) and which on the unit sphere (`nearest_dist`), the steps
  and constants each uses, and what that means for accuracy. Planar distances are
  only dependable near the projection center, so the region needs to match the
  data; `nearest` is exact globally and needs no region at all.

- Removed the references to a "reference R workflow" from the documentation, the
  rendered API docs, and the source comments. It pointed at something readers
  have no access to and cannot identify, so it explained nothing. The provenance
  it carried is now stated directly: distances are taken the way a planar CRS
  such as EPSG:3035 would give them.

## [0.10.1] - 2026-08-03

### Fixed

- `scripts/download_data.sh` now supplies every field the Marine Regions form
  marks required for the IHO download. Organisation was optional in the script
  though the form requires it, and the user category and purpose dropdowns were
  hardcoded to `academia` and `Research` with no way to change them, so anyone
  who was neither could only submit values that misdescribed them. They are now
  `--mr-org`, `--mr-category`, and `--mr-purpose`. The two dropdowns accept only
  fixed values, so the script checks them against the form's own lists before
  downloading anything and prints the valid ones when a value is wrong, rather
  than letting the request fail after the fact.

### Changed

- `scripts/download_data.sh` shows the Marine Regions form values, and says that
  proceeding accepts the dataset licence, before it submits anything. These
  details go to a third party, so it is worth seeing them first. The block is
  printed with the rest of the configuration ahead of the confirmation prompt,
  and only when `iho` is among the selected datasets. The licence is also named
  correctly now: CC BY-NC-SA 4.0, not CC-BY as the script and its help said.

## [0.10.0] - 2026-08-03

### Added

- `depth --on-land` appends an `on_land` boolean column, true where the GEBCO
  elevation is at or above sea level. A point on land gets a real elevation from
  GEBCO, so a consumer could read a mountain top as a depth with nothing to say
  otherwise. The depth value is still reported either way, never nulled, and the
  flag only makes the distinction explicit. The flag reads the raw elevation, so
  it means the same under `--positive`, which inverts the reported sign. Points
  with no reading at all, off the grid, get a null rather than `false`.

- `place` reports how far away the municipality it matched is, and can refuse a
  match that is too far. The nearest-municipality lookup is unbounded and GISCO
  LAU covers Europe only, so a site outside that coverage was assigned whatever
  municipality was closest, however distant, with no way to tell it from a point
  that genuinely sits in one. A `municipality_dist` column now accompanies
  `municipality` whenever `--municipalities` is given, holding `0` for a real
  containment and the distance to the boundary otherwise, and
  `--max-municipality-dist` discards matches beyond a limit, clearing the name
  and the distance together so a row never carries one without the other. Both
  read in `--unit`, `km` by default, matching `coast` and `nearest`. A run
  without `--municipalities` keeps exactly the columns it had before. `country`
  needs none of this, since Natural Earth is global.

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
