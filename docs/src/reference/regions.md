# Regions

The `coast`, `sea`, and `place` commands crop their reference data to a region
box (plus a margin) and, for distance work, project through a Lambert Azimuthal
Equal-Area (LAEA) projection centered on that region. The `depth` command needs
no region (a grid lookup is global), and `nearest` computes exact great-circle
distances on the unit sphere, so it needs no region either.

The region is not just a cropping filter: it sets where distances are measured
from, so the wrong region silently returns wrong distances rather than an error.
[Coverage and limits](./coverage.md) gives the size of that error and how to pick
a region, and is worth reading before running on data outside northern Europe.

## auto, the default

With no `--region` at all, or with `--region auto`, seastamp derives the region
from your own points once it has read them:

```
[seastamp] --region auto: box (-1.30, 9.90, 55.10, 65.90), projection centered on (4.23, 60.47)
```

The **center** is the mean direction of the points computed in three dimensions,
not an average of their degrees. That matters in two places no rectangle can
reach:

- **Around a pole.** A ring of stations at 75N spanning every longitude averages
  to the pole itself, which is the correct center. The `arctic` preset's box
  center is forced down to 75N, four times worse.
- **Across the antimeridian.** The projection has no seam, so a survey either
  side of 180 is centered correctly. Only the crop box has a seam, and `auto`
  widens that to every longitude rather than distorting the center. Cropping
  gets looser; distances stay right.

The **box** is the points' own extent padded by 5 degrees, on top of which each
command adds its usual 5 degree margin. The reference data kept therefore
reaches about 10 degrees, roughly 1100 km, beyond the outermost point, which is
deliberately generous: an offshore point's nearest coast can be hundreds of km
away, and cropping to the points alone would cut the coastline out and overstate
the distance.

Explicit bounds still win. `--region auto --min-lon 0` derives everything except
the western bound.

### When auto cannot help

One projection cannot serve points spread over the whole globe, and `auto` says
so rather than returning a plausible wrong number:

```
[seastamp] warning: the points are spread over too much of the globe for any one
projection to serve them (clustering 0.10 of a possible 1.00).
[seastamp] warning: --region auto cannot help here. Pass --partition to measure
each area in its own projection.
```

The clustering figure is the length of the mean direction vector: 1.00 when
every point sits in one place, near 0 when they are spread evenly over the
globe. A polar ring scores 0.97 despite spanning every longitude, because those
points really do share a direction.

## --partition, for data one projection cannot serve

`--partition` is the answer to that warning. It splits the input into
sub-regions, gives each its own crop box and its own projection center, and
joins the results back together, so a globally spread table comes out as
accurate as running each area separately would have been:

```bash
seastamp coast global-stations.parquet --data ./data/gshhg/... --partition
```

```
[seastamp] --partition: 52 partitions over 540 unique locations, worst distortion 1.97%
[seastamp] 540 rows, 540 unique locations -> global-stations.coast.parquet
```

**The split is driven by accuracy, not by a cell size or a count.** There is no
number to choose. seastamp keeps halving a group while any of its points would
be more than 2% out, and stops as soon as none are, so the partition count is
whatever the data needs. That last figure is the run's own accuracy claim: no
distance in the output is more than that far from what a projection centered on
its own point would have given.

Data that already fits one projection is left as one piece and comes out
identical to an ordinary `--region auto` run, so the flag is safe to leave on:

```
[seastamp] --partition: 1 partition over 10 unique locations, worst distortion 0.98%
```

### What it is worth

Measured with 540 points spread over the globe, against each point run on its
own with a projection centered on it.

`coast`, against GSHHG `f`:

| Run | Mean error | Worst error |
|-----|-----------|-------------|
| `--region global` | 8.2% | 14.0% |
| `--partition` | 0.6% | 1.4% |

`place`, against Natural Earth countries, is not about a percentage: a distorted
projection picks the wrong country outright. The two runs disagreed on 91 of the
540 points, and on a sample of 20 of those, `--partition` matched the per-point
run 20 times and `--region global` none. A point west of the Kermadecs came out
as "Fiji" under one global projection and "New Zealand" under `--partition`.

`sea` benefits least. A point inside a sea polygon is resolved by an exact
lon/lat containment test that no projection touches, so only the fallback for
points inside no polygon changes.

### What it costs

Each partition crops its own copy of the reference data, so a partitioned run
does more work than a single-projection one. The same 540-point global run:

| Command | `--region global` | `--partition` |
|---------|-------------------|---------------|
| `coast` (GSHHG `f`) | 6.0 s, 0.95 GB | 21 s, 1.4 GB |
| `place` (Natural Earth) | 0.5 s, 0.09 GB | 3.0 s, 0.36 GB |

Regional data is much cheaper, because it needs few partitions or only one.

seastamp reads the reference file once for as many partitions as it can hold at
a time, and says so when it needs more than one pass:

```
[seastamp] --partition: 52 partitions over 540 unique locations, worst distortion
1.97%, reference data read 3 times to stay within memory
```

### Limits

- **`coast`, `sea`, and `place` take it.** `depth` reads a grid and `nearest`
  works on the sphere, so neither has a projection to improve and neither takes
  the flag at all.
- **It takes no region or bounds of its own.** `--partition` derives every box
  and center from your points, so combining it with `--region`, `--min-lon` and
  friends, or `--proj-lon0` is an error rather than a precedence question.
- **Results are no longer perfectly smooth across a partition boundary.** Two
  nearby points measured in different projections can disagree slightly, bounded
  by twice the reported distortion. It shows up only if you difference
  neighboring values; each value on its own is more accurate than it would have
  been without the flag.

## Presets

Pick a region with `--region <NAME>`:

| Preset | Box (min_lon, max_lon, min_lat, max_lat) |
|--------|------------------------------------------|
| `global` | -180, 180, -90, 90 |
| `baltic` | 8, 31, 53, 66 |
| `norway` | -10, 45, 55, 85 |
| `arctic` | -180, 180, 60, 90 |
| `atlantic` | -83, 20, -60, 70 |
| `europe` | -25, 45, 34, 72 |
| `mediterranean` | -6, 37, 30, 46 |

`global` is no longer the default; `auto` is. Pass `--region global` to get the
old whole-globe behavior back.

## IHO sea names

`--region` also takes any of the 101 IHO Sea Areas v3 names, baked into the
binary so no data file is needed:

```bash
seastamp coast cores.parquet --data ./data/gshhg/... --region "Barentsz Sea"
```

`seastamp regions` lists presets and IHO names together, with a `source` column
saying which is which. Two things to know:

- **An IHO area is not the colloquial sea.** S-23 splits the Gulfs of Bothnia,
  Finland, and Riga out of the Baltic, so `--region "Baltic Sea"` crops far more
  tightly than the `baltic` preset. Check the neighboring names first.
- **Four areas cannot be used by name**: the Bering Sea, the Chukchi Sea, and
  the North and South Pacific Oceans all cross the antimeridian, so their box
  cannot be expressed. seastamp refuses them and points at `--region auto`.

An unknown name is an error, not a silent fall back to the whole globe, and it
suggests the near misses:

```
Error: unknown region 'Barent Sea'. Did you mean: Barentsz Sea?
```

With [completions](../commands/completions.md) installed, Tab offers these names
directly, which saves looking them up. In bash and zsh a name containing a space
completes only to its first word, so `--region Bar<TAB>` stops at `Barentsz`;
running that reports the full name back at you, as above.

## Explicit box and projection center

Any preset value can be overridden with explicit bounds:

```bash
seastamp coast cores.parquet --data ./data/gshhg/... \
  --min-lon 8 --max-lon 31 --min-lat 53 --max-lat 66
```

Where the LAEA projection is centered depends on how the region was chosen. A
**named** region centers on its box center. Under **auto** the center comes from
the points themselves, which is better whenever the data sits off to one side of
its box. Either way, `--proj-lon0` / `--proj-lat0` override it.

Note that giving bounds without a `--region` name leaves you in auto mode, so
the box is yours and the center is still derived from the data. Pass
`--region global` (or any name) if you want the old box-center behavior.

## Precedence

For the region box and projection center, later sources win:

```
auto / preset / built-in default  <  config file  <  CLI flag
```

So a `--region` name sets the box, a [config file](./configuration.md) can
override individual bounds, and a CLI `--min-lon` (etc.) overrides both. Auto
sits at the bottom: it fills in whatever nothing else specified.

## Choosing a region

Cropping keeps the reference data small and the lookups fast, but a point whose
true nearest feature lies outside the region-plus-margin box can be wrong (for
`coast`, an over-estimate). `auto` pads generously for exactly this reason. If
you are pinning a region by hand and in doubt, widen the box; the unique-location
pipeline keeps even a global run affordable.
