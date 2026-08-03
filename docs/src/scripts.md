# Enrich with several modules

`scripts/enrich.sh` runs several modules over one input in sequence, so the
result is a single file carrying the new columns of every module you selected.
Each module is chained onto the previous one's output, its columns accumulating,
and the in-between files are written to a temporary directory that is removed
when the script ends. Only the final output file remains.

```bash
scripts/enrich.sh [options] <input> <output>
```

## Selecting modules

A module runs only when you give its data source, so the flags pick both the
modules and their inputs. At least one module must be selected.

| Flag | Module | Data source |
|------|--------|-------------|
| `--coast PATH` | [`coast`](./commands/coast.md) | GSHHG shapefile dir or `GSHHS_*_L1.shp` |
| `--depth FILE` | [`depth`](./commands/depth.md) | GEBCO bathymetry NetCDF |
| `--sea PATH` | [`sea`](./commands/sea.md) | IHO Sea Areas GeoJSON or shapefile |
| `--countries FILE` | [`place`](./commands/place.md) | Natural Earth countries shapefile |
| `--nearest FILE` | [`nearest`](./commands/nearest.md) | reference table of named locations |

The modules run in a fixed order (coast, depth, sea, place, nearest); the order
does not matter, since each adds distinct columns.

## Per-module options

| Option | Applies to | Meaning |
|--------|-----------|---------|
| `--municipalities FILE` | place | GISCO LAU municipalities (optional) |
| `--coast-unit km\|m` | coast | Distance unit (default `km`) |
| `--depth-positive` | depth | Report depth positive below sea level |
| `--depth-on-land` | depth | Add an `on_land` boolean column |
| `--max-municipality-dist N` | place | Drop a municipality match further than N away |
| `--place-unit km\|m` | place | Unit for `municipality_dist` and its cutoff (default `km`) |
| `--sea-name-field STR` | sea | Feature field with the sea name (default `NAME`) |
| `--nearest-name-field STR` | nearest | Reference name column (default `name`) |
| `--nearest-unit km\|m` | nearest | Distance unit (default `km`) |

## Common and other options

`--region`, `--lon-col`, `--lat-col`, `--decimals`, and `--threads` are passed to
every module that accepts them (`--region` only to coast, sea, and place).
`--threads` reaches `depth` too, but not its grid lookup, which always runs on one
thread. `--in-format` describes the original input. Other options:

| Option | Meaning |
|--------|---------|
| `--bin PATH` | seastamp binary (default: `$SEASTAMP_BIN`, else the one on `PATH`, else `./target/release` or `./target/debug`) |
| `-k, --keep` | Keep the intermediate files (default: remove them) |
| `-n, --dry-run` | Print the commands without running them |

## Example

```bash
scripts/enrich.sh cores.parquet cores.enriched.parquet \
  --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f \
  --depth ./data/gebco/GEBCO_2024_sub_ice.nc \
  --nearest farms.parquet --nearest-name-field farm_name
```

This writes one file, `cores.enriched.parquet`, with the original columns plus
`dist_to_coast`, `bathymetry`, `nearest_name`, and `nearest_dist`. The
intermediate coast-only and coast+depth files are removed on exit.

The intermediate files are Parquet (lossless); the final file's format follows
its extension, so `cores.enriched.csv.gz` would be written as gzipped CSV.
