# Installation

## Prebuilt binary

The quickest option: every release attaches prebuilt archives for Linux and
macOS (x86_64 and arm64) to its
[GitHub release](https://github.com/AIQC-Hub/seastamp/releases/latest). They
bundle HDF5 and netCDF, so they need no system libraries at all: download the
archive for your platform, unpack it, and run the `seastamp` binary inside. The
[helper scripts](./helper-scripts.md) ship in the archive alongside it.

## From crates.io

```bash
cargo install seastamp
```

This compiles from source, so the [`depth`](./commands/depth.md) command needs
the HDF5 / NetCDF development headers (see [System dependencies](#system-dependencies)).

## Build from source

seastamp is a Rust project, so a recent stable toolchain is all you need:

```bash
git clone https://github.com/AIQC-Hub/seastamp
cd seastamp
cargo build --release
# binary at target/release/seastamp
```

To build a self-contained binary that vendors HDF5 and netCDF (as the release
archives do, needing no system libraries), add `--features static-netcdf`. This
compiles the C libraries from source, so it needs `cmake` and takes longer.

## System dependencies

Only the [`depth`](./commands/depth.md) command needs anything beyond the Rust
toolchain: it reads GEBCO NetCDF and links the HDF5 / NetCDF C libraries, so a
source or `cargo install` build needs their development headers (the same system
dependency as `ctddump`):

```bash
# Ubuntu / Debian
sudo apt-get install libhdf5-dev libnetcdf-dev

# macOS
brew install hdf5
```

The other four commands (`coast`, `sea`, `place`, `nearest`) use only pure-Rust
geometry and have no system dependencies. The prebuilt binary and a
`--features static-netcdf` build vendor the C libraries, so neither needs these.

## Reference data

The datasets each command enriches from (shorelines, bathymetry, sea polygons,
country and municipality boundaries) are large and are not bundled. Download the
ones you need with the helper script, described under
[Reference datasets](./data.md).

## Check it works

```bash
seastamp --help
seastamp coast --help
```

Every command is self-documenting through `--help` at each level.
