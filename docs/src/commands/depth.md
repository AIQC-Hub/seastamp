# depth

Bathymetric depth at each point, from a GEBCO gridded NetCDF file.

```bash
seastamp depth <INPUT> --data <GEBCO.nc> [OPTIONS]
```

Appends one column, `bathymetry` (rename with `--column`), holding the GEBCO
elevation at each point in meters, and a second boolean `on_land` column with
`--on-land`.

## How it works

GEBCO is a regular lon/lat grid, so no nearest-neighbor search is needed. The
`lat` and `lon` axes are read once to learn each axis origin and spacing, then
every point maps straight to its nearest grid cell by arithmetic and reads the
single `elevation` cell. Longitudes are normalized to `[-180, 180)`, and points
off the grid yield a null.

Unlike the other commands, `depth` looks up its points on a single thread, so
`--threads` does not change how the grid is read. HDF5, which the NetCDF reader
sits on, is often built without thread safety, and such a build cannot be entered
from more than one thread even when the calls are locked so they never overlap.
Nothing is lost by it: the reads were funnelled through one lock in any case, so
they never ran concurrently.

GEBCO elevation is negative below sea level. By default the value is reported as
stored (negative under water); `--positive` flips the sign so depth reads
positive under water and land reads negative.

This command needs no [region](../reference/regions.md): a grid lookup is
global by construction.

## Options

Beyond the [shared options](../reference/configuration.md):

| Option | Default | Meaning |
|--------|---------|---------|
| `--data <FILE>` | required | GEBCO bathymetry NetCDF file |
| `--positive` | off | Report depth as positive below sea level |
| `--column <NAME>` | `bathymetry` | Output column name |

## Example

```bash
# Reading and writing gzipped CSV
seastamp depth cores.csv.gz \
  --data ./data/gebco/GEBCO_2024_sub_ice.nc \
  --positive -o cores.depth.csv.gz
```

The `depth` command links the HDF5 / NetCDF C libraries; see
[Installation](../installation.md). See [Reference datasets](../data.md) for how
to obtain the GEBCO grid.
