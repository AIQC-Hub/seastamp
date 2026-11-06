# Helper scripts

The `scripts/` directory holds bash helpers that sit alongside the CLI: one to
fetch the reference datasets, one to run several modules over an input in a
single pass, one to do that for every product of a ctddump pipeline at once and
report on each, and two that turn those reports into summary pages and a static
web site. None is required to use seastamp, but together they cover the "get
the data, stamp, then report" workflow end to end.

All follow the same conventions:

- The header comment doubles as `--help`, so `scripts/<name> --help` prints the
  full interface.
- Steps are traced to stderr as they run (a timestamped `RUN:` line per command),
  so a long run shows what it is doing.
- They run under `set -euo pipefail` and stop at the first error.

## `download_data.sh`

Downloads and unpacks the reference datasets into a local `data/` tree, one
sub-directory per source, matching the paths the command pages use. Selected
datasets download in parallel, existing archives are kept unless `--force` is
given, and the multi-GB GEBCO grid resumes an interrupted download.

```bash
scripts/download_data.sh download gshhg gebco countries lau
```

See [Reference datasets](./data.md) for the full list of sources, the Marine
Regions (IHO) form details, and the caveats.

## `stamp.sh`

Runs several modules over one input in sequence and writes a single output file
carrying every selected module's new columns. A module runs when you give its
data source, each step chains onto the previous one's output, and the
intermediate files are removed when the script ends.

```bash
scripts/stamp.sh cores.parquet cores.stamped.parquet \
  --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f \
  --depth ./data/gebco/GEBCO_2024_sub_ice_topo.nc \
  --nearest farms.parquet --nearest-name-field farm_name
```

See [Stamp with several modules](./scripts.md) for the module flags, the
per-module and common options, and how the intermediate files are handled.

## `stamp_data.sh`

Runs `stamp.sh` over every data product that ctddump's pipeline produces for the
Arctic, Baltic, and Mediterranean seas, one worker per product, all in parallel,
then summarises each stamped file with
[`report stats`](./commands/report.md#report-stats). The inputs are the
de-duplicated files `dedup_data.sh` writes, one per stem (`nrt_ar_ar`,
`nrt_ar_gl`, `cora_ar`, and the same for `bo` and `mo`). Each
`<in>/<stem>.parquet` is stamped to `<out>/<stem>.parquet`, and its report is
written to `<report>/stamp/<stem>.tsv` (`-r, --report`, default `report`).

```bash
scripts/stamp_data.sh -i ../ctddump/output/dedup/dedup -o output/stamp \
  --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f --partition \
  --depth ./data/gebco/GEBCO_2024_sub_ice_topo.nc
```

- A leading command picks the steps, as in ctddump's scripts: `stamp`,
  `report`, or `all` (the default, stamp then report). `report` alone re-reads
  outputs that already exist, so it needs no module flags.
- The module flags and their options are the ones `stamp.sh` takes, forwarded
  unchanged. They are checked once, before any worker starts, so a bad
  combination fails the run up front rather than once per product.
- Name regions (`arctic`, `baltic`, `mediterranean`) to stamp only those; the
  default is all three.
- A product whose input is missing is skipped with a note, so a product that is
  not published yet (the Baltic GL) does not fail the run.
- `-t, --threads` sets the threads of each seastamp call (default 2), so the
  total is that times the number of products running at once.
- Each worker is a separate seastamp process with its own copy of the reference
  data, so memory use grows with the number of workers. `--by-region` runs one
  worker per region and `--sequential` one product at a time. Stamping `depth`
  for several products at once is safe: the one-thread limit on the grid lookup
  applies within a process, not across processes.
- The report reads the units and the depth sign from the same flags the stamp
  step takes (`--coast-unit`, `--place-unit`, `--nearest-unit`,
  `--depth-positive`), so a `report` run on its own needs them repeated.
  `--dist-bands` and `--depth-bands` set the report's band edges.
- `-f, --format` picks the output format (`parquet`, `csv`, `tsv`, `csv.gz`,
  `tsv.gz`), `-n, --dry-run` prints the commands, and `-y, --yes` skips the
  confirmation prompt.

Run `scripts/stamp_data.sh --help` for the full interface.

## `summary_data.sh`

Builds one [`report summary`](./commands/report.md#report-summary) page per
product from the reports `stamp_data.sh` wrote: `<report>/stamp/<stem>.tsv` to
`summary/<stem>.md` (`-d, --dest`), Markdown by default and HTML with
`-f html`. A product with no report is skipped.

```bash
scripts/summary_data.sh -y                  # every region
scripts/summary_data.sh -y -f html baltic   # HTML pages, Baltic only
```

Each page gets a title and notes per product, defined in the script's "Page
text" section, which is the place to edit what a page says about a product. The
titles match ctddump's summary pages for the same products. `--top` sets how
many names a sea-area or country table lists before summing the rest.

## `summary_site.sh`

Renders the Markdown pages into a static web site with
[mdBook](https://rust-lang.github.io/mdBook/), the same way ctddump's script of
the same name does: one part per region, a chapter per product, read from
`summary/` (`-s, --src`) and written to `site/` (`-d, --dest`). Open
`site/index.html` in a browser, or publish the directory as is: it carries a
`.nojekyll`, a `LICENSE`, and a `README.md`, so it can be pushed straight to a
GitHub Pages repository.

```bash
scripts/summary_site.sh -y                          # site/ from summary/
scripts/summary_site.sh -y -t "Arctic stamping" arctic
```

`-t, --title` sets the book title, and `-c, --config` swaps in your own
`book.toml`. Each chapter is named after its page's title, so titles live in
`summary_data.sh` alone.

The whole chain, after ctddump has run:

```bash
scripts/stamp_data.sh -y -i ../ctddump/output/dedup/dedup \
  --coast ./data/gshhg/gshhg-shp-2.3.7/GSHHS_shp/f --partition \
  --depth ./data/gebco/GEBCO_2024_sub_ice_topo.nc
scripts/summary_data.sh -y
scripts/summary_site.sh -y
```

## The old name `enrich.sh`

`stamp.sh` was called `enrich.sh` up to 0.15.0. That name still works: it is a
wrapper that forwards every argument to `stamp.sh` and prints a notice to stderr.
It will be removed in a later release, so move to `stamp.sh` when convenient.

## Requirements

`download_data.sh` needs `curl` and `unzip` on `PATH`. `stamp.sh`,
`stamp_data.sh`, and `summary_data.sh` need a built `seastamp` binary: they use
`$SEASTAMP_BIN` if set, else the one on `PATH`, else a `./target/release` or
`./target/debug` build in the repository (pass `--bin` to point elsewhere).
`stamp_data.sh` calls the `stamp.sh` next to it, so keep the two in the same
directory. `summary_site.sh` needs `mdbook` on `PATH`
(`cargo install mdbook`).
