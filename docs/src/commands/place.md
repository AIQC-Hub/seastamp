# place

Nearest country and municipality, from Natural Earth countries and, optionally,
Eurostat GISCO LAU municipalities.

```bash
seastamp place <INPUT> --countries <NE> [--municipalities <GISCO>] [OPTIONS]
```

Appends three columns:

| Column | Meaning |
|--------|---------|
| `country` | Country name |
| `country_code` | ISO alpha-3 code (the Natural Earth `-99` placeholder becomes null) |
| `municipality` | Municipality name (empty unless `--municipalities` is given) |

## How it works

Both the country and the municipality lookups resolve a point by containment
first (an even-odd point-in-polygon test over R-tree candidates) and fall back
to the nearest boundary otherwise, so an offshore point still gets the closest
land unit. Attribute fields are auto-detected from candidate name lists, so
minor schema drift between dataset versions needs no flags.

`--municipalities` is optional: without it the `municipality` column is left
empty and a note says so.

## Options

Beyond the [shared options](../reference/configuration.md) and the
[region options](../reference/regions.md):

| Option | Default | Meaning |
|--------|---------|---------|
| `--countries <PATH>` | required | Natural Earth countries shapefile |
| `--municipalities <PATH>` | none | GISCO LAU municipalities shapefile |

## Example

```bash
seastamp place cores.parquet \
  --countries ./data/naturalearth/ne_10m_admin_0_countries.shp \
  --municipalities ./data/gisco/lau.shp
```

See [Reference datasets](../data.md) for how to obtain Natural Earth and GISCO.
