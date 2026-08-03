# place

Nearest country and municipality, from Natural Earth countries and, optionally,
Eurostat GISCO LAU municipalities.

```bash
seastamp place <INPUT> --countries <NE> [--municipalities <GISCO>] [OPTIONS]
```

Appends three columns, or four when `--municipalities` is given:

| Column | Meaning |
|--------|---------|
| `country` | Country name |
| `country_code` | ISO alpha-3 code (the Natural Earth `-99` placeholder becomes null) |
| `municipality` | Municipality name (empty unless `--municipalities` is given) |
| `municipality_dist` | Distance to that municipality, `0` when the point is inside it. Only with `--municipalities` |

## How it works

Both the country and the municipality lookups resolve a point by containment
first (an even-odd point-in-polygon test over R-tree candidates) and fall back
to the nearest boundary otherwise, so an offshore point still gets the closest
land unit. Attribute fields are auto-detected from candidate name lists, so
minor schema drift between dataset versions needs no flags.

`--municipalities` is optional: without it the `municipality` column is left
empty, no `municipality_dist` column is added, and a note says so.

### Bounding the municipality match

The municipality fallback is unbounded, and GISCO LAU covers Europe only, so a
site outside that coverage still resolves to whatever municipality is nearest,
however far away. A point a few hundred km offshore picks up a coastal
municipality that it is in no sense part of.

`municipality_dist` makes that visible: `0` means the point really is inside the
polygon, and a large value means the fallback reached a long way.
`--max-municipality-dist` turns it into a filter, discarding matches beyond the
limit. The name and the distance are cleared together, so a row never carries one
without the other. Both are read in `--unit`, kilometres by default.

`country` needs none of this, since Natural Earth is global.

## Options

Beyond the [shared options](../reference/configuration.md) and the
[region options](../reference/regions.md):

| Option | Default | Meaning |
|--------|---------|---------|
| `--countries <PATH>` | required | Natural Earth countries shapefile |
| `--municipalities <PATH>` | none | GISCO LAU municipalities shapefile |
| `--max-municipality-dist <N>` | none | Discard a municipality match further than this, in `--unit` |
| `--unit km\|m` | `km` | Unit for `municipality_dist` and the cutoff |

## Example

```bash
seastamp place cores.parquet \
  --countries ./data/naturalearth/ne_10m_admin_0_countries.shp \
  --municipalities ./data/gisco/lau.shp \
  --max-municipality-dist 50
```

See [Reference datasets](../data.md) for how to obtain Natural Earth and GISCO.
