# regions

List sea and ocean bounding boxes: every name `--region` accepts, or one box per
named area in an IHO Sea Areas file.

```bash
seastamp regions [OPTIONS]
```

This is the one command that takes no input table. It enriches nothing: it
answers "which seas exist, and where are they", so you can find a region for
your data. The presets are mostly European; the 101 IHO Sea Areas names that sit
behind them cover the world.

## The two listings

Without `--data`, every name `--region` accepts is listed: the seven presets,
then the 101 IHO Sea Areas v3 areas baked into the binary. No data file needed.

```bash
seastamp regions
```

```
name                  min_lon   max_lon   min_lat   max_lat  source  antimeridian
global                -180.00    180.00    -90.00     90.00  preset
baltic                   8.00     31.00     53.00     66.00  preset
...
Arctic Ocean          -180.00    180.00     71.39     90.00  iho
Baltic Sea               9.52     23.51     52.65     59.94  iho
Bering Sea             161.82   -156.23     51.35     66.56  iho     yes
Coral Sea              141.02    169.87    -30.00     -6.79  iho
North Sea               -4.45     12.01     51.00     61.02  iho
Southern Ocean        -180.00    180.00    -85.56    -60.00  iho
```

Any of those names goes straight into `--region`:

```bash
seastamp coast cores.parquet --data ./data/gshhg/... --region "Coral Sea"
```

With `--data`, the boxes are re-derived from an IHO Sea Areas file instead of
read from the baked-in table. That is what you want for a newer IHO release, or
for any other named polygon layer:

```bash
seastamp regions --data ./data/iho/World_Seas_IHO_v3.shp
```

`scripts/gen_iho_areas.py` turns that output back into the baked-in table; see
its header for the refresh procedure.

## Saving the list

`--output` writes the same table in any supported format, so it can be joined
or scripted against:

```bash
seastamp regions -o regions.parquet
seastamp regions --data ./data/iho/World_Seas_IHO_v3.shp -o seas.parquet
```

Add `--quiet` to write the file without printing.

## Options

| Option | Default | Meaning |
|--------|---------|---------|
| `--data <PATH>` | none | IHO Sea Areas polygons (GeoJSON or shapefile). Omit to list the built-in names |
| `--name-field <NAME>` | `NAME` | Property or attribute holding the area name |
| `--name <TEXT>` | none | Keep only areas whose name contains this text (case-insensitive) |
| `-o, --output <FILE>` | none | Also write the list to a file |
| `--out-format <FMT>` | inferred, else parquet | `parquet`, `csv`, `tsv`, `csv.gz`, `tsv.gz` |
| `-q, --quiet` | off | Do not print the table, only write it |

## Output columns

| Column | Meaning |
|--------|---------|
| `name` | The area name, one row per name |
| `min_lon`, `max_lon` | Longitude bounds in degrees |
| `min_lat`, `max_lat` | Latitude bounds in degrees |
| `crosses_antimeridian` | True when the box runs east past 180 and continues from -180 |
| `source` | `preset`, `iho` (the baked-in table), or `data` (derived from `--data`) |

## How the box is computed

Latitude is the plain minimum and maximum of the vertices. Longitude is not.
Marine Regions splits its polygons at the antimeridian, so a Pacific area has
vertices at both -180 and 180, and taking the minimum and maximum of those would
report the whole globe for it. Instead seastamp finds the widest arc of
longitude the area's edges leave uncovered, and reports everything else: the
smallest arc that actually contains the area.

The extent is built from edges rather than vertices, so a long edge with no
intermediate vertex does not read as a gap. An area covering every longitude
(the Arctic and Southern Oceans) reports the full -180 to 180 range, while one
that merely reaches the line without crossing it (the East Siberian Sea, which
ends at exactly 180.00) is left unflagged.

Reading the full IHO v3 shapefile takes about half a minute, nearly all of it
parsing the 149 MB of geometry. Use `--output` if you want the list to hand.

## Caveats

**A crossing box cannot be used as-is.** When `crosses_antimeridian` is true,
`min_lon` is greater than `max_lon`, and seastamp rejects such a region (see
[Coverage and limits](../reference/coverage.md)). In IHO v3 this is four areas:
the Bering Sea, the Chukchi Sea, and the North and South Pacific Oceans.
`--region auto` handles data in those areas correctly; naming them does not.

**An IHO area is not the colloquial sea.** IHO follows the S-23 limits, which
split water bodies more finely than everyday usage. The `Baltic Sea` box stops
at 23.5 E and 59.9 N because the Gulf of Bothnia, the Gulf of Finland, and the
Gulf of Riga are three separate areas; the built-in `baltic` preset covers all
four. Check the neighboring names before taking one area's box as your region,
or the crop will cut off part of your study area.

**A big sea makes a poor region for `coast`.** The region is also where planar
distances are measured from, so a box spanning an ocean puts its own edges tens
of percent out: `--region "Indian Ocean"` is about 5% off at the Arabian Sea
edge, and `--region "Southern Ocean"` about 7%. The default `--region auto`
does not have this problem, because it centers on the data rather than on the
sea. This does not affect `sea`, `depth`, `nearest`, or `place`'s `country`.

**The box is a rectangle, not the sea.** It is the extent of the polygons, so it
includes land and neighboring water. As a crop box that is exactly what is
wanted; as a description of the sea it is coarse.

## See also

- [Regions](../reference/regions.md) for what a region does and its precedence
- [Coverage and limits](../reference/coverage.md) for where distances stay accurate
- [sea](./sea.md) for stamping points with the name of the sea they fall in
- `scripts/gen_iho_areas.py` for regenerating the baked-in table
