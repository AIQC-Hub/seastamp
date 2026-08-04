# regions

List sea and ocean bounding boxes: the built-in `--region` presets, or one box
per named area in an IHO Sea Areas file.

```bash
seastamp regions [OPTIONS]
```

This is the one command that takes no input table. It enriches nothing: it
answers "which seas exist, and where are they", so you can find a region for
data the presets do not cover. The presets are mostly European, and the IHO Sea
Areas layer names roughly a hundred seas and oceans worldwide.

## The two listings

Without `--data`, the built-in presets are listed. These are the names
`--region` accepts:

```bash
seastamp regions
```

```
name            min_lon   max_lon   min_lat   max_lat  antimeridian
global          -180.00    180.00    -90.00     90.00
baltic             8.00     31.00     53.00     66.00
norway           -10.00     45.00     55.00     85.00
arctic          -180.00    180.00     60.00     90.00
atlantic         -83.00     20.00    -60.00     70.00
europe           -25.00     45.00     34.00     72.00
mediterranean     -6.00     37.00     30.00     46.00
```

With `--data`, every named area in the IHO Sea Areas file is reduced to one box.
The v3 layer holds 101 of them:

```bash
seastamp regions --data ./data/iho/World_Seas_IHO_v3.shp
```

```
name                  min_lon   max_lon   min_lat   max_lat  antimeridian
Arctic Ocean          -180.00    180.00     71.39     90.00
Baltic Sea               9.52     23.51     52.65     59.94
Bering Sea             161.82   -156.23     51.35     66.56  yes
Coral Sea              141.02    169.87    -30.00     -6.79
North Sea               -4.45     12.01     51.00     61.02
Southern Ocean        -180.00    180.00    -85.56    -60.00
```

Those names are not `--region` values. Use the box directly:

```bash
seastamp coast cores.parquet --data ./data/gshhg/... \
  --min-lon 141 --max-lon 170 --min-lat -30 --max-lat -6
```

## Saving the list

`--output` writes the same table in any supported format, so it can be joined
or scripted against:

```bash
seastamp regions --data ./data/iho/iho_sea_areas.geojson -o seas.parquet
```

Add `--quiet` to write the file without printing.

## Options

| Option | Default | Meaning |
|--------|---------|---------|
| `--data <PATH>` | none | IHO Sea Areas polygons (GeoJSON or shapefile). Omit to list the presets |
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
the Bering Sea, the Chukchi Sea, and the North and South Pacific Oceans. Run
those as an eastern box and a western box and concatenate the results.

**An IHO area is not the colloquial sea.** IHO follows the S-23 limits, which
split water bodies more finely than everyday usage. The `Baltic Sea` box stops
at 23.5 E and 59.9 N because the Gulf of Bothnia, the Gulf of Finland, and the
Gulf of Riga are three separate areas; the built-in `baltic` preset covers all
four. Check the neighbouring names before taking one area's box as your region,
or the crop will cut off part of your study area.

**A big sea makes a poor region for `coast`.** The region is also where planar
distances are measured from, so a box spanning an ocean puts its own edges tens
of percent out. Crop to the part of the sea your data is actually in. This does
not affect `sea`, `depth`, `nearest`, or `place`'s `country`.

**The box is a rectangle, not the sea.** It is the extent of the polygons, so it
includes land and neighbouring water. As a crop box that is exactly what is
wanted; as a description of the sea it is coarse.

## See also

- [Regions](../reference/regions.md) for what a region does and its precedence
- [Coverage and limits](../reference/coverage.md) for where distances stay accurate
- [sea](./sea.md) for stamping points with the name of the sea they fall in
