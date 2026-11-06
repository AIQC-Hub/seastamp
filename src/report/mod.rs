//! `report`: summarise what the stamping commands wrote, without stamping.
//!
//! - [`stats`] reads one stamped file and writes a long-form table (TSV, text,
//!   or JSON) of counts, statistics, and bands for each stamped column it finds.
//! - [`summary`] reads that TSV back and renders a Markdown or HTML page, the
//!   form `scripts/summary_site.sh` assembles into a static web site.
//!
//! The two meet only through the TSV, whose layout is fixed by the constants in
//! [`stats`]: `summary` never opens a data file, so it is instant, and a report
//! can be regenerated without re-stamping.

use std::error::Error;

use crate::cli::{ReportArgs, ReportCommand};

pub mod stats;
pub mod summary;
mod table;

pub fn run(args: ReportArgs) -> Result<(), Box<dyn Error>> {
    match args.command {
        ReportCommand::Stats(a) => stats::run(a),
        ReportCommand::Summary(a) => summary::run(a),
    }
}
