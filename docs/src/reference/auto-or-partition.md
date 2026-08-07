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
| Falls into a few distant clusters, however far apart | **`--partition`** | This is the case it wins outright. Measured below: 8.4% mean error becomes 0.16%. |
| Is spread continuously across an ocean or the globe | `--partition`, **and check for nulls** | It cuts the average error several-fold, but the worst points stay bad for a different reason, explained below. |

If you are not sure, run the default and read what seastamp prints. If it does
not warn, `auto` was fine and `--partition` would have changed nothing.

```
[seastamp] warning: the farthest input point is 15127 km from the projection
center (0.0, 0.0), where planar distances are off by roughly 63%.
```

## What the numbers actually are

Seven point sets, each run both ways against GSHHG `f`. **Every point** is
compared against a reference run of that point **alone**, with the whole globe
indexed and the projection centered on itself, so the reference carries neither
projection distortion nor cropping loss. These are full populations, not samples.

| Dataset | Span | Partitions | `auto` mean | `auto` worst | `--partition` mean | `--partition` worst |
|---------|------|-----------|------------|-------------|-------------------|--------------------|
| North Sea survey | 322 km | 1 | \- | \- | identical to `auto` | within 1 mm |
| Norwegian shelf | 1 314 km | 1 | \- | \- | identical to `auto` | within 2 cm |
| Nordic seas | 3 055 km | 1 | \- | \- | identical to `auto` | within 2 cm |
| North Atlantic | 4 806 km | 1 | \- | \- | identical to `auto` | within 22 cm |
| Two clusters, Atlantic and Pacific | 14 298 km | 2 | 8.40% | 16.92% | **0.16%** | **0.62%** |
| Atlantic basin | 9 865 km | 7 | 2.54% | 8.47% | **0.80%** | 20.56% |
| Global grid | 19 999 km | 34 | 23.85% | 99.11% | **6.68%** | 98.79% |

Three things to take from this.

**Below about 5000 km the two are the same thing.** The partitioner found nothing
worth splitting and built one partition covering everything, which is what `auto`
already does. The tiny differences are floating-point noise, not method. Passing
`--partition` to regional data costs nothing and changes nothing.

**Clustered data is where partitioning wins outright.** Two survey areas on
opposite sides of the world went from 8.40% mean error to 0.16%, with no point
worse than 0.62%. Nothing was lost and everything improved.

**Continuously spread data improves on average but keeps a bad tail.** The global
grid's mean error fell from 23.85% to 6.68%, a real gain, but its worst point was
still 98.79% out. That is not the projection failing. It is a second, different
error that partitioning makes worse rather than better.

## Two errors, and only one of them is bounded

This is the single most useful thing to understand on this page.

### Projection error, which partitioning does bound

A flat projection scales distances more and more the further you go from its
center. seastamp splits until every point is within 2% of its own partition's
center, so this error is bounded by construction, and every run reports what it
achieved:

```
[seastamp] --partition: 34 partitions over 144 unique locations, worst distortion 1.94%
```

### Cropping error, which it does not

Every region only indexes reference data inside its own box plus about 10
degrees, roughly 1100 km. A point whose true nearest coastline lies beyond that
gets the nearest one that *is* inside, which is always an **over-estimate**.

`auto` on globally spread data hardly suffers this, because its single box is
effectively the whole world. `--partition` uses many tight boxes, so a point far
out in open water can lose the coastline it should have matched. In the global
grid above, **17 of 138 points were still more than 2% out**, and the worst were
open-ocean points whose nearest land is further away than their partition can
see.

> **The reported "worst distortion" is not the accuracy of the output.** It
> describes the projection only. A run can report 1.94% and still contain a point
> that is 98% out, because that point lost its coastline to cropping rather than
> to the projection.

This is why the decision table says "check for nulls" for continuously spread
data. It is also why clustered data does so much better: a tight cluster's box
still comfortably contains the coast its own points care about.

### When a point gets no answer at all

Taken far enough, cropping leaves nothing to match at all and the output is null.
In the global grid, 6 of 144 points came back null under `--partition` and had a
number under `auto`.

Every one was in the Southern Ocean, and the cause is the dataset rather than
seastamp: **GSHHG's L1 shoreline stops at 69S and holds no Antarctic coastline**
(Antarctica is in L5 and L6, which `coast` does not read). The nearest L1 land to
those points is another continent entirely, far outside their partition. `auto`
returned a number for them only because its global box reached that far
continent, and that number was itself heavily distorted.

seastamp says so rather than leaving you to notice:

```
[seastamp] warning: 1 partition(s) have no shoreline within reach, so
dist_to_coast is null for points there. GSHHG L1 has no Antarctic coastline
(it stops at 69S), which is the usual cause.
```

A null is easy to spot and filter. A confidently wrong number is not.

## What it costs

Each partition crops its own copy of the reference data, so partitioning usually
does more work. On the 540-point global set:

| Command | `auto` | `--partition` |
|---------|--------|---------------|
| `coast` (GSHHG `f`) | 6.0 s, 0.95 GB | 21 s, 1.4 GB |
| `place` (Natural Earth) | 0.5 s, 0.09 GB | 3.0 s, 0.36 GB |

**But clustered data is cheaper partitioned, not dearer.** Tight boxes index far
less than one box stretched across the gap between the clusters:

| Two clusters, Atlantic and Pacific | Time | Peak memory |
|---|------|-------------|
| `auto` | 1.80 s | 262 MB |
| `--partition` | 0.55 s | 40 MB |

One box spanning both oceans has to hold the Americas in between, which no point
in the input is anywhere near.

When the partitions will not fit in memory together, seastamp reads the reference
file more than once rather than growing:

```
[seastamp] --partition: 52 partitions over 540 unique locations, worst distortion
1.97%, reference data read 3 times to stay within memory
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
