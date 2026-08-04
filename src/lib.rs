//! seastamp: stamp longitude/latitude points with sea attributes.
//!
//! Five modules add columns to a table of points, keyed only on `longitude` and
//! `latitude`:
//!   - `coast`    distance to the nearest shoreline (GSHHG)
//!   - `depth`    bathymetric depth (GEBCO grid)
//!   - `sea`      sea / ocean name (IHO Sea Areas)
//!   - `place`    nearest country and municipality (Natural Earth + GISCO)
//!   - `nearest`  nearest location in a second table, with its distance
//!
//! Every module shares one pipeline (`pipeline::run_module`): read the input,
//! reduce it to unique rounded locations, enrich those in parallel, then join the
//! results back onto the full table and write it out.
//!
//! A sixth command, `regions`, stands outside that pipeline: it takes no points
//! and lists sea and ocean bounding boxes, which is where a region for the other
//! commands comes from.

use std::error::Error;

pub mod cli;
pub mod config;
pub mod geo;
pub mod io;
pub mod modules;
pub mod pipeline;

use cli::{Cli, Commands};

/// Dispatch a parsed [`Cli`] to the requested module.
pub fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    match cli.command {
        Commands::Coast(args) => modules::coast::run(args),
        Commands::Depth(args) => modules::depth::run(args),
        Commands::Sea(args) => modules::sea::run(args),
        Commands::Place(args) => modules::place::run(args),
        Commands::Nearest(args) => modules::nearest::run(args),
        Commands::Regions(args) => modules::regions::run(args),
    }
}
