# Configuration

## Shared options

Every command accepts the same input, output, and pipeline options:

| Option | Default | Meaning |
|--------|---------|---------|
| `-o, --output <FILE>` | `<stem>.<command>.<input format>` | Output file (beside the input) |
| `--in-format <FMT>` | inferred, else parquet | `parquet`, `csv`, `tsv`, `csv.gz`, `tsv.gz`, `auto` |
| `--out-format <FMT>` | inferred, else parquet | same set |
| `--overwrite` | off | Replace clashing output columns instead of failing |
| `--lon-col <NAME>` | `longitude` | Longitude column |
| `--lat-col <NAME>` | `latitude` | Latitude column |
| `--decimals <N>` | `3` | Rounding applied before de-duplicating |
| `-t, --threads <N>` | all cores | Worker threads. `depth` always looks up its grid on one thread, see [depth](../commands/depth.md) |
| `-c, --config <TOML>` | none | Config file (CLI flags override it) |

The `coast`, `sea`, and `place` commands also take the
[region options](./regions.md).

## Formats

Input and output can be Parquet (default), CSV, TSV, and the gzip variants
`csv.gz` and `tsv.gz`. The format is inferred from the file extension; an
unrecognized extension falls back to Parquet. Gzip is handled directly (not via
a Polars feature), so it behaves the same across Polars versions.

## Config file

A TOML config file supplies the region box and projection center, sitting
between the built-in default and the CLI flags (see
[precedence](./regions.md#precedence)). Every field is optional:

```toml
# region.toml
region    = "baltic"   # a named preset
min_lon   = 8.0        # or explicit bounds, overriding the preset
max_lon   = 31.0
min_lat   = 53.0
max_lat   = 66.0
proj_lon0 = 19.5       # LAEA projection center (default: region center)
proj_lat0 = 59.5
```

```bash
seastamp coast cores.parquet --data ./data/gshhg/... -c region.toml
```

## Rounding and de-duplication

Coordinates are rounded to `--decimals` places (3 by default) before the input
is reduced to unique locations. Only the unique locations are looked up, then
the results are joined back onto every row. Raising `--decimals` distinguishes
closer points at the cost of more unique lookups; lowering it collapses more
rows together.
