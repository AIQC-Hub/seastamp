//! Shell completion scripts, generated from the clap command tree.
//!
//! There is nothing to compute here: `clap_complete` walks the same [`Cli`]
//! definition the parser uses, so a script can never list a flag the binary
//! does not have. What it emits is a static snapshot of one version's CLI,
//! which is why the command's help tells the user to regenerate after an
//! upgrade.
//!
//! The one hand-fed part is `--region`. Its values are a plain `String`, so
//! clap would have nothing to offer; [`crate::cli::region_names`] supplies the
//! 109 accepted names through a value parser that still accepts anything else.

use std::error::Error;

use clap::CommandFactory;

use crate::cli::{Cli, CompletionsArgs};

pub fn run(args: CompletionsArgs) -> Result<(), Box<dyn Error>> {
    let mut cmd = Cli::command();
    clap_complete::generate(args.shell, &mut cmd, "seastamp", &mut std::io::stdout());
    Ok(())
}
