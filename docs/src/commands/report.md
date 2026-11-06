# report

Summarise files the stamping commands have written, as a report table
(`report stats`), or turn such a table into a Markdown or HTML summary page
(`report summary`).

```
seastamp report stats   [options] <stamped file> [dest]
seastamp report summary [--report-dir report] [--format md|html]
                        [--title TEXT] [--note TEXT]... [--top N] [-o dest] <stem>
```

Neither command stamps anything. `stats` reads a stamped file, and `summary`
reads only the small table `stats` wrote, so a summary page can be rebuilt with
a new title or notes in an instant. The report is written to `dest` (or `-o`),
or to stdout when it is omitted.

## `report stats`

Finds the columns the stamping commands added, by their default names, and
reports on each one:

| Column | Written by | Reported as |
|--------|-----------|-------------|
| `dist_to_coast` | [`coast`](./coast.md) | distance |
| `bathymetry` | [`depth`](./depth.md) | depth |
| `on_land` | `depth --on-land` | flag |
| `sea_name` | [`sea`](./sea.md) | text |
| `country`, `municipality` | [`place`](./place.md) | text |
| `municipality_dist` | `place --municipalities` | distance |
| `nearest_name` | [`nearest`](./nearest.md) | text |
| `nearest_dist` | `nearest` | distance |

`country_code` is left out, since it restates `country`. A column renamed at
stamping time (`--column`, `--name-column`, `--dist-column`) is not found.

From a Parquet file only these columns and the coordinates are read, so memory
use is about their size (some 2 GB for 30 million rows with four stamped
columns), well below what stamping the same file takes.

### Rows and locations

Each stamping command rounds longitude and latitude (3 decimals by default),
looks every unique location up once, and copies the result to all the rows
there. The report counts the same way, so it gives every count twice:

- **rows**: what the file carries;
- **locations**: unique rounded positions, the lookups that were made.

Minimum, maximum, and mean are taken over locations, so a cast with hundreds of
rows weighs no more than a single surface sample. Rows without a usable
longitude and latitude get no stamped value; they are counted once, as
`(no position)`, and left out of everything else. Pass `--decimals` if the file
was stamped with a different rounding.

### The table

The report is one long table, so the same layout serves the whole file and a
`--by` breakdown:

| Column | Meaning |
|--------|---------|
| `column` | the stamped column, or `(all)` for the file totals |
| `kind` | `total`, `distance`, `depth`, `text`, or `flag` |
| `item` | what the row counts (below) |
| `unit` | `km` or `m` for distances, `m` for depth |
| `rows`, `locations` | the two counts |
| `min`, `max`, `mean` | over locations, on the `(value)` row only |

The items for each kind:

| Kind | Items |
|------|-------|
| `total` | `(located)`: rows with a position and their locations; `(no position)` |
| `distance` | `(value)`, `(null)`, then one per distance band |
| `depth` | `(value)`, `(null)`, `land`, then one per band of depth below sea level |
| `text` | one per value, most locations first, then `(null)` |
| `flag` | `true`, `false`, `(null)` |

Every item is written even when its counts are zero, so a missing row never has
to be read as zero.

### Bands

Distances are banded at 1, 5, 20, and 100 km (`< 1`, `1-5`, `5-20`, `20-100`,
`>= 100`), and depth below sea level at 10, 50, 200, and 1000 m. Each band
includes its lower edge and excludes its upper one. A depth location whose
elevation is at or above sea level is `land`, the same test `depth --on-land`
uses.

- `--dist-bands 1,5,20,100` sets the distance edges, in km whatever unit the
  column is in (a column in metres has them scaled to 1000, 5000, ...).
- `--depth-bands 10,50,200,1000` sets the depth edges, in m.

### Telling it how the file was stamped

A stamped file does not record its units or the depth sign convention, so give
them with the same flags `scripts/stamp.sh` uses:

| Flag | Default | When to pass it |
|------|---------|-----------------|
| `--coast-unit km\|m` | `km` | `coast --unit m` |
| `--place-unit km\|m` | `km` | `place --unit m` |
| `--nearest-unit km\|m` | `km` | `nearest --unit m` |
| `--depth-positive` | off | `depth --positive` |

Without `--depth-positive` a depth value is read as GEBCO elevation, negative
under water, which is what `depth` writes by default.

### Other options

- `-f, --format tsv|text|json`: TSV (default), an aligned text table, or a JSON
  array of objects (empty cells become `null`).
- `--by COL`: one block of rows per distinct value of `COL` (for example
  `platform_code`), under a leading column of that name. A position shared by
  two groups is a location of each.
- `--lon-col`, `--lat-col`, `--in-format`: as for the stamping commands.
- `-t, --threads`: worker threads for reading the file.

```bash
# Whole-file report, as TSV
seastamp report stats cores.stamped.parquet report/stamp/cores.tsv

# Per-platform, readable on screen
seastamp report stats --by platform_code -f text cores.stamped.parquet

# Stamped with --unit m and --positive
seastamp report stats --coast-unit m --depth-positive cores.stamped.parquet
```

## `report summary`

Builds a single page for one file stem from `<report-dir>/stamp/<stem>.tsv`,
the path [`stamp_data.sh`](../helper-scripts.md#stamp_datash) writes its reports
to. The page has an **Overview** of rows and locations, then a section per
module the file was stamped with, in the order the modules run:

| Section | Tables |
|---------|--------|
| Distance to coast | statistics, distance bands |
| Depth | statistics, depth bands with land first, the land flag when present |
| Sea area | locations and rows per sea area |
| Place | per country, per municipality, distance to municipality |
| Nearest location | per reference location, distance to it |

A module the file was not stamped with has no section. Percentages are of the
located rows and locations. A name table lists the `--top` values (default 20)
with the most locations and sums the rest in one row.

The page needs a whole-file report, so a TSV written with `--by` is refused.

### Title and notes

The section prose is generic, the same for every file. Anything specific to a
file goes in:

- `--title TEXT`: replaces the default `Summary: <stem>` heading.
- `--note TEXT`: a note under the title; repeat for several.

Both are plain text and are escaped in HTML output. In the pipeline they are
supplied per stem by [`summary_data.sh`](../helper-scripts.md#summary_datash),
which is the place to edit what a page says about a given product.

```bash
# Markdown page, to stdout
seastamp report summary cores

# HTML page with a title and a note
seastamp report summary nrt_bo_bo --format html \
  --title "Baltic Sea: Near Real Time, regional product (BO)" \
  --note "Stamped against GSHHG f and GEBCO 2024." \
  -o summary/nrt_bo_bo.html
```

The Markdown pages are what
[`summary_site.sh`](../helper-scripts.md#summary_sitesh) builds a web site from.
