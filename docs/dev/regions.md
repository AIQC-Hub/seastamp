# Regions

**The region is not only a crop: it is where distances are measured from.**
`coast` and `place`'s `municipality_dist` are planar in a LAEA centered on the
region, so a region that does not match the data returns wrong distances rather
than an error. `Enricher::projection_center` exists so `run_module` can warn past
2% error; `depth` and `nearest` return `None` because neither uses a projection.
Keep that warning working when touching the pipeline.

`docs/src/reference/coverage.md` is the user-facing statement of what works
where, and `docs/src/reference/regions.md` the user-facing description of the
flag. Keep both in step with this file.

## auto is the default

**The default region is `auto`, not the whole globe.** `config::resolve` cannot
settle an auto region on its own (it has no points yet), so it leaves a
placeholder plus the explicit overrides in `Settings::auto`, and each
region-using module calls `config::apply_auto_region` after `read_frame` and
before opening its reference data. That ordering is the whole mechanism: do not
move the enricher construction above it. `depth` and `nearest` never call it,
having no region.

Auto derives the center with `geo::arc::spherical_center` (3D unit-vector mean)
rather than any average of degrees, because that is what makes a ring of stations
around the North Pole center on the pole, and points either side of 180 center
near 180.

**The projection has no antimeridian seam; only the rectangular crop box does.**
When the data wraps, auto keeps a correct center and widens the crop to every
longitude, which is why it works across the dateline where a box cannot.

Auto also reports the resultant length as a spread measure and warns below 0.5
that no single projection can serve the input. **That case is now `--partition`**
(see below), and both warnings point at it.

The earlier measurement that ruled out a *fixed* partition still stands and is
why `--partition` does not use one: an IHO-area-based split came out 2.5 to 6.7%
out for the six ocean-sized areas, failing exactly where the old global default
failed, because those areas are as large as the globe was. Do not replace the
data-driven split with named cells.

The box is the data extent padded by `config::AUTO_PAD_DEG` (5 degrees), on top
of which each module adds its own `CROP_MARGIN_DEG`. The total, about 10 degrees,
is deliberately generous: an offshore point's nearest coast can be hundreds of km
away, and cropping to the points alone would cut the coastline out and overstate
the distance.

## Named regions

Regions otherwise come from `--region` presets (`global`, `baltic`, `norway`,
`arctic`, `atlantic`, `europe`, `mediterranean`), any of the 101 IHO Sea Areas
names in the generated `config::IHO_AREAS`, or explicit
`--min-lon/--max-lon/--min-lat/--max-lat` and `--proj-lon0/--proj-lat0`.

An unknown name is an error with suggestions, never a silent fall back to the
globe. Four IHO areas (Bering, Chukchi, North and South Pacific) cross the
antimeridian and are refused by name, pointing at `--region auto`.

Add presets in `config::preset_bbox` **and** in `config::PRESET_NAMES`, which is
what `seastamp regions` and the preset test iterate; a preset missing from the
list is invisible in both. The Baltic box (8, 31, 53, 66) is the `baltic` preset.
The `place` municipality lookup will also need a per-region country list of ISO3
codes.

The presets stay deliberately few and mostly European; the IHO names cover the
rest, so a wider preset list is not the way to broaden coverage.

Regenerate `src/config/iho_areas.rs` only with `scripts/gen_iho_areas.py` from
`seastamp regions --data <IHO> -q -o areas.csv`. It is generated, and hand edits
will be lost.

Note that an IHO area is not the colloquial sea: S-23 splits the Gulfs of
Bothnia, Finland, and Riga out of the Baltic, so the IHO `Baltic Sea` box is much
smaller than the `baltic` preset. Do not "correct" a preset to match one IHO box.

## --partition

`--partition` replaces the single region with one per piece of the input:
`geo::partition::partition` splits the unique locations, `pipeline::
run_partitioned` drives them, and each piece gets its own crop box and LAEA
center. `apply_auto_region` is not called at all on that path, which is why the
flag `conflicts_with_all` every region argument in `cli::RegionArgs`: there is no
single box or center for them to override.

**The split criterion is the warning threshold.** `projection::DISTORTION_LIMIT`
(2%) is both what `pipeline::warn_if_far_from_center` warns above and what the
partitioner splits below, so a partitioned run can never trip the warning it
exists to answer. Keep them the same constant.

Bisection is by farthest pair, deterministic on purpose: the output is a data
product, so k-means with a seed was rejected. Bisection alone overshoots badly
(it splits the moment one point falls outside, so a 25 degree cluster becomes two
of 15), which measured 88 partitions on a global grid; `coalesce` merges back any
pair whose union still fits, bringing that to 64. Do not remove it without
re-measuring.

Partitioning runs on the **unique rounded** locations, not the input rows, unlike
`apply_auto_region`. The accuracy bound is unaffected, since every row's location
is some unique location, and only the center's placement within the tolerance
differs. That is the justification for the inconsistency; it is not an oversight.

Partition crops overlap (each is padded like `auto`'s, about 10 degrees), so
building them all at once holds several copies of the reference data: a global
input measured 4.35 globes of crop across 64 partitions. `pipeline::batches`
therefore groups partitions to `CROP_BUDGET_GLOBES` and the module builds one
batch per pass. That is what `CoastEnricher::open_many` is for: **one** shapefile
read fanned out to every crop in the batch. A module that builds per partition
instead multiplies the dominant cost by the partition count, which for `coast` is
the 154 MB GSHHG parse.

`coast` streams its shapefile and crops as it goes, so `open_many` fans each
segment out to every crop in the batch. `sea` and `place` instead parse whole
polygon sets into memory first, so theirs goes through
`PolygonIndex::build_many`, which crops the shared feature slice once per region
and clones only what each keeps. Feature bounding boxes are computed once there
rather than per region: a box costs a pass over every vertex, and with dozens of
partitions that would dominate.

Measured on 540 globally spread points. `coast` on GSHHG `f`: `--region global`
6.0 s and 0.95 GB at 8.2% mean error, `--partition` 21 s and 1.4 GB at 0.6% mean
error over 52 partitions in 3 passes. `place` on Natural Earth: 0.5 s and 0.09 GB
against 3.0 s and 0.36 GB, and the two runs named a different nearest country for
91 of the 540 points, `--partition` being the right one on every spot check.

The per-partition "nothing overlapped this region" warnings are aggregated into
one line (`N of M partitions matched no ...`). Do not restore the per-partition
version: at 52 partitions it buries everything else in the log.

`place` decides its output column set in `run` rather than reading it off a built
enricher, because `run_partitioned` needs the columns before any enricher exists.
It turns only on whether `--municipalities` was given, so the two must be kept in
step with `PlaceEnricher::outputs`.

## Longitude extents are arcs, not intervals

Marine Regions splits its polygons at the antimeridian, so a Pacific area has
vertices at both -180 and 180 and a plain min/max would report the whole globe
for it. The extent is instead the complement of the widest longitude gap the
area's *edges* leave uncovered (edges, not vertices: a long edge with no
intermediate vertex would otherwise read as a gap). A gap under
`CIRCUMPOLAR_GAP_DEG` counts as none at all and the area reports -180..180.

When the arc crosses the line, `min_lon` is reported greater than `max_lon` with
`crosses_antimeridian` true, which is exactly the shape `config::validate_bbox`
refuses. That is intentional: the flag names the seas needing two runs rather
than pretending a usable box exists.

Against IHO v3, `seastamp regions --data <IHO>` lists 101 areas in about 35
seconds, nearly all of it parsing the 149 MB shapefile. Four cross the
antimeridian (Bering, Chukchi, North and South Pacific) and two circle the globe
(Arctic, Southern). That is the set to re-check if the extent logic is ever
touched.

## Known limits

- An explicit region box crossing the antimeridian cannot be expressed: lon
  comparisons do not wrap, so `config::validate_bbox` rejects
  `min_lon > max_lon`. `--region auto` is the way around it.
- GISCO LAU is Europe-only, so `place --municipalities` is meaningless elsewhere.
