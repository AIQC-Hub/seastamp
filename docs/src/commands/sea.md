# sea

Sea or ocean name at each point, from the IHO Sea Areas polygons.

```bash
seastamp sea <INPUT> --data <IHO> [OPTIONS]
```

Appends one column, `sea_name` (rename with `--column`), holding the name of the
sea or ocean the point falls in.

## How it works

The IHO Sea Areas features (Marine Regions GeoJSON or shapefile) are cropped
whole to the region box plus a margin, and their bounding boxes are indexed in
an R-tree. Each point is resolved by an even-odd point-in-polygon test over the
candidate features, with a nearest-boundary fallback for points that fall just
inland (for example inside a fjord not covered by a sea polygon).

`--name-field` selects which feature property holds the name (default `NAME`);
the command errors clearly if that field is absent.

Containment is an exact lon/lat test, so no projection is involved and most
results are unaffected by which region you pick. Only the nearest-boundary
fallback is planar. For points spread too widely for one projection center, pass
[`--partition`](../reference/regions.md#partition-for-data-one-projection-cannot-serve),
which also crops the polygons per area rather than keeping one global set.

## Options

Beyond the [shared options](../reference/configuration.md) and the
[region options](../reference/regions.md):

| Option | Default | Meaning |
|--------|---------|---------|
| `--data <PATH>` | required | IHO Sea Areas polygons (GeoJSON or shapefile) |
| `--name-field <NAME>` | `NAME` | Property / attribute field holding the sea name |
| `--column <NAME>` | `sea_name` | Output column name |

## Example

```bash
seastamp sea cores.parquet --region norway \
  --data ./data/iho/iho_sea_areas.geojson
```

See [Reference datasets](../data.md) for how to obtain the IHO Sea Areas.
