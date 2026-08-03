# coast

Distance to the nearest shoreline, from GSHHG shoreline polygons.

```bash
seastamp coast <INPUT> --data <GSHHG> [OPTIONS]
```

Appends one column, `dist_to_coast` (rename with `--column`), holding the
distance from each point to the nearest coastline in kilometers (default) or
meters (`--unit m`).

## How it works

The GSHHG L1 (land / ocean) boundary segments are cropped to the region box plus
a 5 degree margin, projected through the region's Lambert Azimuthal Equal-Area
(LAEA) projection into meters, and indexed in an R-tree. Each point is projected
the same way and takes the planar distance to the nearest segment. Segments are
dropped, never clipped, so cropping cannot invent artificial shoreline; a point
whose true nearest coast lies beyond the region-plus-margin box gets an
over-estimate, so widen the [region](../reference/regions.md) for such cases.

## Options

Beyond the [shared options](../reference/configuration.md) and the
[region options](../reference/regions.md):

| Option | Default | Meaning |
|--------|---------|---------|
| `--data <PATH>` | required | GSHHG shapefile directory (resolution `f` recommended) or a `GSHHS_*_L1.shp` file |
| `--unit <km\|m>` | `km` | Distance unit for the output column |
| `--column <NAME>` | `dist_to_coast` | Output column name |

## Example

```bash
seastamp coast cores.parquet \
  --data ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f \
  --unit km -o cores.coast.parquet
```

See [Reference datasets](../data.md) for how to obtain GSHHG.
