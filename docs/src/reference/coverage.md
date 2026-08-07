# Coverage and limits

What works anywhere in the world, what needs configuring first, and what does not
work yet. Read this before running seastamp on data outside northern Europe.

## What works where

| Command | Outside Europe | Why |
|---------|----------------|-----|
| [`depth`](../commands/depth.md) | Works anywhere | GEBCO is a global grid read by arithmetic, with no projection involved. |
| [`nearest`](../commands/nearest.md) | Works anywhere | Great-circle distance on the unit sphere, exact globally, and it takes no region or projection center. |
| [`sea`](../commands/sea.md) | Works anywhere | IHO Sea Areas is global, and a point inside a sea polygon is resolved by an exact lon/lat containment test. Only the fallback for points inside no polygon is projected. |
| [`place`](../commands/place.md), `country` | Works anywhere | Natural Earth is global, containment first. |
| [`place`](../commands/place.md), `municipality` | **Europe only** | GISCO LAU has no coverage elsewhere. |
| [`coast`](../commands/coast.md) | Works anywhere | The distance is measured in a projection centered on the region, and the default region centers itself on your data. |

## The region centers itself

`coast` and `place`'s `municipality_dist` measure distance in a plane centered on
the region (see [Technical notes](./technical-notes.md)). That is accurate near
the center and degrades away from it.

**Since 0.12.0 the default region is `auto`**, which derives the box and the
center from your own points, so this mostly takes care of itself. Real
measurements from a three-point survey off Bergen:

| Region | `dist_to_coast` |
|--------|-----------------|
| `auto` (default) | 40.97, 0.400, 69.41 km |
| `global` (the old default) | 44.09, 0.423, 77.65 km |

The old default was about 8% out there, and far worse further from (0, 0). See
[Regions](./regions.md) for what `auto` does and when it gives up.

The rest of this section is about the case `auto` cannot fix, and about what
happens if you pin a region that does not match your data. How far off a 10 km
distance is, against a center at (0, 0):

| Data location | Distance from (0, 0) | Error |
|---------------|---------------------|-------|
| Canary Islands | 3 550 km | -1.7% |
| North Sea | 6 233 km | -11.6% |
| Baltic | 6 767 km | -9.1% |
| Gulf of Maine | 8 239 km | +22.6% |
| Bering Sea | 13 563 km | -50.8% |
| New Zealand | 15 341 km | -61.4% |

Note that this was never about being outside Europe. A North Sea run under the
old default was already about 12% out. Centering the projection on the data
removes essentially all of it, which is what `auto` now does by default.

Use a [preset or an IHO sea name](./regions.md) when you want the region pinned
and reproducible rather than derived:

```bash
seastamp coast cores.parquet --data ./data/gshhg/... --region norway
```

Anywhere else, any of the 101 IHO sea names works as a region, no data file
needed. `seastamp regions` lists them:

```bash
seastamp coast cores.parquet --data ./data/gshhg/... --region "Tasman Sea"
```

Or give the box and the center directly:

```bash
# a run off New Zealand
seastamp coast cores.parquet --data ./data/gshhg/... \
  --min-lon 160 --max-lon 180 --min-lat -50 --max-lat -30 \
  --proj-lon0 170 --proj-lat0 -40
```

For a named region the center defaults to the middle of the box, so setting a
sensible box is usually enough on its own.

seastamp warns when the input sits far enough from the center to matter:

```
[seastamp] warning: the farthest input point is 15127 km from the projection
center (0.0, 0.0), where planar distances are off by roughly 63%.
[seastamp] warning: pass --region, or --proj-lon0 / --proj-lat0, centered on your
data, or --partition to measure each area in its own projection.
```

A large region is not free either. A box spanning a whole ocean puts its own
edges thousands of km from its center, so the warning can fire on a region that
is genuinely as tight as the data allows.

**When the data really is spread too wide for any one center, use
[`--partition`](./regions.md#partition-for-data-one-projection-cannot-serve),
and see [auto or partition](./auto-or-partition.md) for which to pick when.**
It splits the run into sub-regions, each measured in its own projection, and
keeps splitting until every point is within 2% of its own partition's center, and
widens any partition whose answer reached past the data it held. On a globally
spread grid that cut `coast`'s mean error from 30.5% to 0.5%, and on two distant
survey clusters from 8.6% to 0.2%. It also stopped `place` assigning the wrong
country to 91 of 540 globally spread points. It is available on `coast`, `sea`,
and `place`; `depth` and `nearest` have no projection to fix.

[auto or partition](./auto-or-partition.md) compares the two side by side.

## The antimeridian: distances now work, cropping still does not

A region **box** is a plain lon/lat rectangle compared without wrapping, so one
crossing 180 degrees cannot be expressed. The **projection** has no such
problem: a LAEA about a correct center is seamless.

`--region auto` separates the two. Points either side of the dateline get a
correct center and a crop box that keeps every longitude, so distances are
right and only cropping is loose. Measured on three points around Fiji:

| Region | `dist_to_coast` |
|--------|-----------------|
| `auto` (default) | 7.31, 13.03, 50.84 km |
| `global` (the old default) | 1.23, 5.41, 8.23 km |

The old default was wrong by up to a factor of six there. Nothing but the
projection center changed.

An explicit crossing box is still rejected:

```
Error: region min-lon (170) is greater than max-lon (-170). A region crossing
the antimeridian is not supported; split the run into an eastern and a western
box, or use --region auto, which keeps the projection centered correctly and
only widens the crop
```

So are the four IHO sea names whose extent crosses the line: the Bering Sea, the
Chukchi Sea, and the North and South Pacific Oceans. `seastamp regions` flags
them with `crosses_antimeridian` true, so you know before the run.

Cropping is the part that still suffers: a `(170, 180)` region drops reference
features lying just across the line, so a coastline a few km east of 180 is
invisible to a run bounded at 180. `auto` avoids that by keeping every longitude
when the data wraps, at the cost of a larger reference set to index.

## Municipalities are Europe only

`place --municipalities` uses Eurostat GISCO LAU, which covers the EU, EFTA, and
candidate countries. The lookup is a nearest match, so a site outside that
coverage is still assigned the nearest European municipality, thousands of km
away, and nothing in the name says so. Two ways to handle it:

- Leave `--municipalities` off entirely outside Europe. `country` still works,
  since Natural Earth is global.
- Or keep it and pass `--max-municipality-dist`, which clears the name beyond a
  limit. The `municipality_dist` column shows how far each match reached, and is
  `0` for a point genuinely inside a municipality.

If no LAU polygon survives cropping, seastamp says so rather than filling the
column with distant matches.

## Accuracy ceiling

All geometry is spherical, so nothing here is ellipsoidal-accurate. Expect
agreement with an ellipsoidal calculation at the level of a few parts in a
thousand, which is far below the resolution of the shoreline and boundary data
being measured against. Sub-meter work would mean replacing the spherical LAEA
with an ellipsoidal one.

`depth` samples the nearest GEBCO cell with no interpolation, so its resolution
is the grid's, about 450 m at the equator for the 15 arc-second product.
