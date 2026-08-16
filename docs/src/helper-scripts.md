# Helper scripts

The `scripts/` directory holds two bash helpers that sit alongside the CLI: one
to fetch the reference datasets, one to run several modules over an input in a
single pass. Neither is required to use seastamp, but together they cover the
"get the data, then stamp" workflow end to end.

Both follow the same conventions:

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
  --depth ./data/gebco/GEBCO_2024_sub_ice.nc \
  --nearest farms.parquet --nearest-name-field farm_name
```

See [Stamp with several modules](./scripts.md) for the module flags, the
per-module and common options, and how the intermediate files are handled.

This script was called `enrich.sh` up to 0.15.0. That name still works: it is a
wrapper that forwards every argument to `stamp.sh` and prints a notice to stderr.
It will be removed in a later release, so move to `stamp.sh` when convenient.

## Requirements

`download_data.sh` needs `curl` and `unzip` on `PATH`. `stamp.sh` needs a built
`seastamp` binary: it uses `$SEASTAMP_BIN` if set, else the one on `PATH`, else
a `./target/release` or `./target/debug` build in the repository (pass `--bin` to
point elsewhere).
