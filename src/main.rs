use clap::Parser;
use seastamp::cli::Cli;

fn main() {
    // Rayon and Polars spawn worker threads with a small default stack that can
    // overflow inside Polars' parquet writer on large frames. Raise the default
    // for every thread std spawns, before any pool initializes, and only when the
    // user has not chosen their own value. (Same reasoning as ctddump.)
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        std::env::set_var("RUST_MIN_STACK", (16 * 1024 * 1024).to_string());
    }

    let cli = Cli::parse();
    if let Err(e) = seastamp::run(cli) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
