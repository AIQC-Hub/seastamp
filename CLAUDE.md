# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project overview

`seastamp` is a Rust CLI that stamps a table of `longitude`/`latitude` points
with sea attributes, one per top-level command:

| Command | Adds | Reference data |
|---------|------|----------------|
| `coast` | distance to the nearest shoreline | GSHHG |
| `depth` | bathymetric depth at the point | GEBCO grid |
| `sea` | sea / ocean name | IHO Sea Areas |
| `place` | nearest country and municipality | Natural Earth + GISCO |
| `nearest` | nearest location in a second table the caller supplies | none |
| `regions` | nothing: lists sea and ocean bounding boxes | none (baked-in table) |

Input and output can be Parquet (default), CSV, TSV, and the gzip variants
`csv.gz` / `tsv.gz`. Every enrichment module reads the input, reduces it to
unique locations with rounded coordinates (3 decimals by default), enriches those
in parallel (`depth` excepted), then joins the results back onto every input row.
`regions` takes no input table and runs no pipeline.

It is a sibling to `ctddump` and follows the same house style, but is a separate
package on purpose: it must stay light and reusable across several downstream
projects, so it does not depend on `ctddump` and adds spatial dependencies only
as each algorithm needs them.

## Hard rules

These two cost real debugging to establish. Do not re-litigate either.

1. **No em dashes in any human-facing text**: `README.md`, `CHANGELOG.md`, docs,
   generated output, help text, and log lines. Use a colon, comma, parentheses,
   a semicolon, or a reworded sentence. (Carried over from `ctddump`.)
2. **Never enrich `depth` from more than one thread.** A serial HDF5 build cannot
   be entered from several threads at all, even under a mutex, and it crashes
   hard. `DepthEnricher` returns `parallel() -> false` for this reason. Read
   [docs/dev/depth-hdf5.md](docs/dev/depth-hdf5.md) before touching that module.

Commit and push only when the user asks. Commit messages end with
`Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.

## Commands

```bash
cargo build
cargo test                         # unit + integration tests
cargo run -- <command> --help      # discover any command's interface
cargo run -- coast input.parquet --data ./data/gshhg/.../GSHHS_shp/f
```

The full CLI is defined with `clap` in `src/cli.rs` and is self-documenting via
`--help` at every level.

## Where the detail lives

`docs/dev/` holds the working notes. They are not part of the published
documentation site (`docs/src/`, listed in `docs/src/SUMMARY.md`) and `docs/` is
excluded from the crate, so nothing there ships.

| Read this | When |
|-----------|------|
| [docs/dev/architecture.md](docs/dev/architecture.md) | Touching dispatch, `Settings`, `pipeline::run_module`, I/O, or `src/geo/` |
| [docs/dev/modules.md](docs/dev/modules.md) | Changing what a module computes, or adding one |
| [docs/dev/regions.md](docs/dev/regions.md) | Anything about `--region`, projections, the antimeridian, or the IHO name table |
| [docs/dev/depth-hdf5.md](docs/dev/depth-hdf5.md) | Any change near `depth`, NetCDF, HDF5, or thread counts |
| [docs/dev/data-and-scripts.md](docs/dev/data-and-scripts.md) | The reference datasets, or `scripts/*` |
| [docs/dev/release.md](docs/dev/release.md) | Branching, tagging, CI, publishing |

The user-facing documentation is the mdBook under `docs/src/`. When behavior
changes, `docs/src/reference/coverage.md` (what works where) and
`docs/src/reference/regions.md` (the `--region` flag) are the two pages most
likely to go stale, along with `README.md` and `CHANGELOG.md`.

Each module file's header comment states its own algorithm and caveats; keep
those in step with the code as well.
