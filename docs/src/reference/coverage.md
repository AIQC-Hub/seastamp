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
| [`coast`](../commands/coast.md) | **Set the region first** | GSHHG is global, but the distance is measured in a projection centered on the region. |

## Set the region to match your data

`coast` and `place`'s `municipality_dist` measure distance in a plane centered on
the region (see [Technical notes](./technical-notes.md)). That is accurate near
the center and degrades away from it, and **the default region is the whole
globe, which puts the center at (0, 0)**, in the Gulf of Guinea. Running
without a region therefore mismeasures distances everywhere except the equatorial
Atlantic, and it does so quietly: the numbers look reasonable.

How far off, for a 10 km distance, against that default center:

| Data location | Distance from (0, 0) | Error |
|---------------|---------------------|-------|
| Canary Islands | 3 550 km | -1.7% |
| North Sea | 6 233 km | -11.6% |
| Baltic | 6 767 km | -9.1% |
| Gulf of Maine | 8 239 km | +22.6% |
| Bering Sea | 13 563 km | -50.8% |
| New Zealand | 15 341 km | -61.4% |

Note that this is not about being outside Europe. A default-settings North Sea
run is already about 12% out. Centering the projection on the data removes
essentially all of it.

Use a [preset](./regions.md) when one fits:

```bash
seastamp coast cores.parquet --data ./data/gshhg/... --region norway
```

Anywhere else, give the box and the center directly:

```bash
# a run off New Zealand
seastamp coast cores.parquet --data ./data/gshhg/... \
  --min-lon 160 --max-lon 180 --min-lat -50 --max-lat -30 \
  --proj-lon0 170 --proj-lat0 -40
```

The center defaults to the middle of the box, so setting a sensible box is
usually enough on its own.

seastamp warns when the input sits far enough from the center to matter:

```
[seastamp] warning: the farthest input point is 15127 km from the projection
center (0.0, 0.0), where planar distances are off by roughly 63%.
[seastamp] warning: pass --region, or --proj-lon0 / --proj-lat0, centered on your data.
```

A large region is not free either. A box spanning a whole ocean puts its own
edges thousands of km from its center, so the warning can fire on a region that
is genuinely as tight as the data allows. Splitting the run into several smaller
regions is the fix when the accuracy matters.

## The antimeridian is not supported

Region boxes are plain lon/lat rectangles compared without wrapping, so a box
crossing 180 degrees cannot be expressed. Such a box is rejected:

```
Error: region min-lon (170) is greater than max-lon (-170). A region crossing
the antimeridian is not supported; split the run into an eastern and a western
box, or use a whole-globe region
```

Cropping is affected too: a `(170, 180)` region drops reference features lying
just across the line, so a coastline a few km east of 180 is invisible to a run
bounded at 180. For work spanning the dateline, run the eastern and western
halves separately and concatenate, or accept a whole-globe region with its
distance cost.

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
