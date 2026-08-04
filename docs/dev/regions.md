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
that no single projection can serve the input. That is the case partitioning
would eventually address, and it is deliberately left unsolved: measured, an
IHO-area-based partition is 2.5 to 6.7% out for the six ocean-sized areas, so it
would fail exactly where the old global default failed.

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
