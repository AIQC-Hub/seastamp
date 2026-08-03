# Output columns

Each command appends its columns to a copy of the input and writes the result;
the input columns are preserved and their order is kept.

| Command | Appended columns | Type |
|---------|------------------|------|
| [`coast`](../commands/coast.md) | `dist_to_coast` | float (km or m) |
| [`depth`](../commands/depth.md) | `bathymetry`, plus `on_land` with `--on-land` | float (m), bool |
| [`sea`](../commands/sea.md) | `sea_name` | text |
| [`place`](../commands/place.md) | `country`, `country_code`, `municipality`, plus `municipality_dist` when `--municipalities` is given | text, text, text, float (km or m) |
| [`nearest`](../commands/nearest.md) | `nearest_name`, `nearest_dist` | text, float (km or m) |

The single-column commands rename their output with `--column`; `nearest`
renames with `--name-column` / `--dist-column`.

## Null values

A row gets null (or NaN for float columns) output when:

- its `longitude` or `latitude` is null or NaN, or
- the lookup finds nothing (for example a `depth` point off the GEBCO grid, or a
  `nearest` run against an empty reference set), or
- `place` discards a municipality match for exceeding
  `--max-municipality-dist`, which clears `municipality` and
  `municipality_dist` together.

`depth` is deliberately not one of these cases for a point on land: GEBCO gives a
real elevation there, positive above sea level, and it is reported rather than
nulled. `--on-land` flags such points in a boolean column, which is null itself
where there is no reading at all.

## Clashing columns

If an output column name already exists in the input, the run fails before any
enrichment, naming the clashing column(s). Pass `--overwrite` to replace such a
column in place instead: it keeps its position but takes the output value and
dtype.

## Output format

By default the output file is written beside the input as
`<stem>.<command>.<input format>`, so the output format follows the input's
(a `points.csv.gz` input enriches to `points.coast.csv.gz`). Override the path
with `--output` or the format with `--out-format`. An input with an unrecognized
extension defaults to Parquet.
