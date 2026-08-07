# auto or partition

`coast`, `sea`, and `place` measure in a flat map projection, and a projection is
only accurate near the point it is centered on. There are two ways to handle
that:

- **`--region auto`** (the default) picks one projection centered on all your
  points.
- **`--partition`** splits the points into groups and gives each group its own.

This page is about choosing between them, and about how accurate the result
actually is in each case.

## The short answer

| Your data | Use | Why |
|-----------|-----|-----|
| Spans less than about 5000 km | `auto` | `--partition` produces a single partition, so the two give the same answer to within centimetres. |
| Falls into a few distant clusters, however far apart | **`--partition`** | This is the case it wins outright. Measured below: 8.6% mean error becomes 0.15%. |
| Is spread continuously across an ocean or the globe | **`--partition`** | It cuts the average error from tens of percent to well under one. |

If you are not sure, run the default and read what seastamp prints. If it does
not warn, `auto` was fine and `--partition` would have changed nothing.

```
[seastamp] warning: the farthest input point is 15127 km from the projection
center (0.0, 0.0), where planar distances are off by roughly 63%.
```

## What the numbers actually are

Six point sets, each run both ways against GSHHG `f`. **Every point** is
compared against a reference run of that point **alone**, with the whole globe
indexed and the projection centered on itself, so the reference carries neither
projection distortion nor cropping loss. These are full populations, not samples.

| Dataset | Span | Partitions | `auto` mean | `auto` worst | `--partition` mean | `--partition` worst |
|---------|------|-----------|------------|-------------|-------------------|--------------------|
| North Sea survey | 322 km | 1 | \- | \- | identical to `auto` | within 1 mm |
| Norwegian shelf | 1 314 km | 1 | \- | \- | identical to `auto` | within 2 cm |
| Nordic seas | 3 055 km | 1 | \- | \- | identical to `auto` | within 2 cm |
| North Atlantic | 4 806 km | 1 | \- | \- | identical to `auto` | within 22 cm |
| Two clusters, Atlantic and Pacific | 14 298 km | 2 | 8.60% | 16.91% | **0.15%** | **0.45%** |
| Global grid | 19 999 km | 34 | 30.54% | 974.72% | **0.54%** | **2.23%** |

Three things to take from this.

**Below about 5000 km the two are the same thing.** The partitioner found nothing
worth splitting and built one partition covering everything, which is what `auto`
already does. The tiny differences are floating-point noise, not method. Passing
`--partition` to regional data costs nothing and changes nothing.

**Clustered data is where partitioning wins outright.** Two survey areas on
opposite sides of the world went from 8.60% mean error to 0.15%, with no point
worse than 0.45%. Nothing was lost and everything improved.

**A single projection degrades further than most people expect.** On the global
grid `auto`'s mean error was 30.54%, and its worst point came out nearly ten
times its true distance. That is not a rounding problem, it is a different
number. Partitioning brought the same grid to 0.54% mean and 2.23% worst.

## Two errors, and how each is handled

A partitioned run has two independent ways of being wrong, and it is worth
knowing which is which.

### Projection error, bounded by splitting

A flat projection scales distances more and more the further you go from its
center. seastamp splits until every point is within 2% of its own partition's
center, so this error is bounded by construction, and every run reports what it
achieved:

```
[seastamp] --partition: 33 partitions over 144 unique locations, worst distortion 1.99%
```

### Cropping error, bounded by widening

Every region only indexes reference data inside its own box plus a margin. A
point whose true nearest coastline lies beyond that would get the nearest one
that *is* inside, which is always an over-estimate.

Partitions crop tightly, which is what makes them cheap, and tight crops suit
points near a coast but not points in open water, whose nearest shoreline can be
two thousand km away. So seastamp checks: **if a partition's answer reached
further than the data that partition was given, the answer is not final.** Those
partitions are rebuilt with a crop widened by exactly as much as their own
answers asked for, and re-run.

```
[seastamp] --partition: 31 partition rebuild(s) with a wider crop, where an
answer reached past the data the first crop held.
```

Only the partitions that need it pay for it. A run whose crops were already
sufficient reports no rebuilds and costs nothing extra, and clustered data
typically needs one or none.

> **The reported "worst distortion" describes the projection only.** It is not a
> bound on the total error of the output, and the widening pass above is what
> handles the other half.

### The limit of it

Widening stops at 40 degrees, about 4400 km of reference data around a
partition, which is further than any real nearest-coast distance. Going all the
way to a global crop for each partition would cost a full-world index apiece and
undo the memory saving partitioning exists for.

A point can still come back null if its reference dataset genuinely holds nothing
in range. The usual cause is not the crop but the data: **GSHHG's L1 shoreline
stops at 69S and holds no Antarctic coastline** (Antarctica is in L5 and L6,
which `coast` does not read), so points deep in the Southern Ocean have no nearby
L1 land to find. seastamp says so rather than leaving you to notice:

```
[seastamp] warning: 2 location(s) had no reference feature within reach even
after widening the crop, so their columns are null. A dataset that does not
cover the area is the usual cause: GSHHG's L1 shoreline, for one, stops at 69S
and holds no Antarctic coast.
```

## What it costs

Each partition crops its own copy of the reference data, so partitioning usually
does more work. On the global grid above:

| Run | Time | Peak memory |
|-----|------|-------------|
| `auto` | 7.1 s | 0.97 GB |
| `--partition` | 28.7 s | 1.43 GB |

Roughly half of that is the widening pass rebuilding the 31 partitions whose
first crop turned out too tight.

**But clustered data is cheaper partitioned, not dearer.** Tight boxes index far
less than one box stretched across the gap between the clusters:

| Two clusters, Atlantic and Pacific | Time | Peak memory |
|---|------|-------------|
| `auto` | 1.8 s | 262 MB |
| `--partition` | 1.8 s | 90 MB |

One box spanning both oceans has to hold the Americas in between, which no point
in the input is anywhere near.

When the partitions will not fit in memory together, seastamp reads the reference
file again rather than growing, and says how often it had to:

```
[seastamp] --partition: reference data read 7 times.
```

## Which commands it affects

| Command | Does `--partition` help? |
|---------|--------------------------|
| `coast` | **Yes, most.** `dist_to_coast` is planar throughout. |
| `place` | **Yes, and categorically.** A distorted projection does not return a slightly wrong distance, it names the wrong place. On 540 globally spread points the two runs named a different nearest country for 91 of them, and every spot check went to `--partition`. |
| `sea` | **Barely.** Which sea a point is in is decided by an exact lon/lat containment test that no projection touches. Only the fallback for points inside no polygon changes. |
| `depth` | No. It reads a global grid by arithmetic and takes no region at all. |
| `nearest` | No. It works in exact great-circle distance on the sphere and takes no region at all. |

`depth` and `nearest` reject the flag rather than accepting and ignoring it.

## What does not change

- **Reproducibility.** Partitioning is deterministic: the same table always gives
  the same partitions, and two runs give byte-identical output. There is no
  random seed and no iteration count.
- **Output shape.** The same columns and rows in the same order. Rows with no
  usable coordinate stay null in place.
- **Everything `auto` is good at.** Partitions derive their centers the same way,
  so stations ringing a pole still center on the pole, and points either side of
  the antimeridian still center near 180.

## Constraints

`--partition` derives every box and center from your points, so it cannot be
combined with `--region`, with `--min-lon` and friends, or with `--proj-lon0` /
`--proj-lat0`. seastamp rejects the combination rather than silently picking one:

```
error: the argument '--partition' cannot be used with '--region <REGION>'
```

For a pinned, reproducible region instead of a derived one, use the
[presets and IHO sea names](./regions.md).
