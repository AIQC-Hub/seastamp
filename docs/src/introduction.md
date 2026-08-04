# seastamp

**seastamp** is a small, fast command-line tool that stamps a table of points
with sea attributes. Give it a file with `longitude` and `latitude` columns and
it appends any of: distance to the nearest coast, bathymetric depth, the sea or
ocean name, the nearest country and municipality, or the nearest location in a
second table you supply.

It is written in Rust, uses no PROJ or GDAL (the geometry is hand-rolled and
pure Rust), and reads and writes Parquet, CSV, TSV, and the gzip variants
`csv.gz` and `tsv.gz`.

## What you can do

| Command | Purpose |
|---------|---------|
| [`coast`](./commands/coast.md) | Distance to the nearest shoreline (GSHHG). |
| [`depth`](./commands/depth.md) | Bathymetric depth at the point (GEBCO grid). |
| [`sea`](./commands/sea.md) | Sea or ocean name at the point (IHO Sea Areas). |
| [`place`](./commands/place.md) | Nearest country and municipality (Natural Earth + GISCO). |
| [`nearest`](./commands/nearest.md) | Nearest location in a second table you supply, with its distance. |
| [`regions`](./commands/regions.md) | List sea and ocean bounding boxes, to find a region for the commands above. |

## How it works

The five enrichment commands follow the same pipeline: read the input, reduce it to unique
locations with rounded coordinates (3 decimals by default), enrich those unique
locations in parallel, then join the results back onto every input row. A file
with millions of rows but few distinct positions is therefore cheap to enrich,
because only the distinct positions are ever looked up. `regions` stands outside
that pipeline: it takes no points and only lists boxes.

## Quick example

```bash
# Distance to the nearest coast, in kilometers
seastamp coast cores.parquet \
  --data ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f

# Nearest fish farm to each measurement
seastamp nearest cores.parquet --to farms.parquet --name-field farm_name
```

New here? Start with [Installation](./installation.md), skim the
[Commands](./commands/coast.md), then see the
[reference datasets](./data.md) each command needs.
