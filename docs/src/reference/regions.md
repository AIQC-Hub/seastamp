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

## Presets

Pick a region with `--region <NAME>`:

| Preset | Box (min_lon, max_lon, min_lat, max_lat) |
|--------|------------------------------------------|
| `global` (default) | -180, 180, -90, 90 |
| `baltic` | 8, 31, 53, 66 |
| `norway` | -10, 45, 55, 85 |
| `arctic` | -180, 180, 60, 90 |
| `atlantic` | -83, 20, -60, 70 |
| `europe` | -25, 45, 34, 72 |
| `mediterranean` | -6, 37, 30, 46 |

The default is `global` (the whole globe).

## Explicit box and projection center

Any preset value can be overridden with explicit bounds:

```bash
seastamp coast cores.parquet --data ./data/gshhg/... \
  --min-lon 8 --max-lon 31 --min-lat 53 --max-lat 66
```

By default the LAEA projection is centered on the region box center. Override it
with `--proj-lon0` / `--proj-lat0` if you want the center elsewhere (for example
to reduce distortion over an asymmetric region).

## Precedence

For the region box and projection center, later sources win:

```
preset / built-in default  <  config file  <  CLI flag
```

So a `--region` preset sets the box, a [config file](./configuration.md) can
override individual bounds, and a CLI `--min-lon` (etc.) overrides both.

## Choosing a region

Cropping keeps the reference data small and the lookups fast, but a point whose
true nearest feature lies outside the region-plus-margin box can be wrong (for
`coast`, an over-estimate). If in doubt, widen the box or use `global`; the
unique-location pipeline keeps even a global run affordable.
