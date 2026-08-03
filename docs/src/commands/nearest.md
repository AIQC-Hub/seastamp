# nearest

Nearest location in a second table you supply, with the distance to it.

```bash
seastamp nearest <INPUT> --to <REFERENCE> [OPTIONS]
```

Unlike the other commands, the reference data is not a bundled dataset but a
second table you pass with `--to`: any set of named locations (fish farms,
ports, stations). For every input point it appends the name of the nearest
reference location and the great-circle distance to it.

Appends two columns:

| Column | Meaning |
|--------|---------|
| `nearest_name` | Name of the nearest reference location (rename with `--name-column`) |
| `nearest_dist` | Distance to it, in km (default) or m (rename with `--dist-column`) |

## How it works

Every reference point is mapped to a unit-sphere `(x, y, z)` vector and indexed
in a 3D R-tree. Each input point is projected the same way and takes the R-tree's
nearest neighbor. Euclidean chord distance on the unit sphere grows monotonically
with the great-circle distance, so the nearest by chord is the nearest on the
globe; the chord is then converted back to meters.

The unit sphere is used instead of the region LAEA on purpose: the two sets can
be anywhere and arbitrarily far apart, and a single planar projection distorts
distances away from its center. The sphere is exact everywhere, so this command
takes no region box or projection center. Reference rows with a missing
coordinate are skipped; an empty reference set leaves every output null.

## Options

Beyond the [shared options](../reference/configuration.md):

| Option | Default | Meaning |
|--------|---------|---------|
| `--to <FILE>` | required | Reference table: the second set of locations |
| `--to-format <FMT>` | inferred | Format of the reference table |
| `--to-lon-col <NAME>` | `longitude` | Longitude column in the reference table |
| `--to-lat-col <NAME>` | `latitude` | Latitude column in the reference table |
| `--name-field <NAME>` | `name` | Column in the reference table holding each location's name |
| `--unit <km\|m>` | `km` | Distance unit for the output distance column |
| `--name-column <NAME>` | `nearest_name` | Output column for the nearest name |
| `--dist-column <NAME>` | `nearest_dist` | Output column for the distance |

## Example

```bash
# Nearest fish farm to each measurement, distance in km
seastamp nearest cores.parquet --to farms.parquet \
  --name-field farm_name -o cores.nearest.parquet
```

The name column is cast to text, so numeric ids work as names too.
