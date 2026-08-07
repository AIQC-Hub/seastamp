//! The `completions` generator: that every shell produces a script, that the
//! script carries the parts a user actually tabs for, and that offering the
//! `--region` names to completion did not start rejecting anything.

use clap::CommandFactory;
use clap::Parser;
use clap_complete::Shell;
use seastamp::cli::{region_names, Cli, Commands};

/// Generate a script the way `modules::completions::run` does, into a buffer.
fn script(shell: Shell) -> String {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, "seastamp", &mut buf);
    String::from_utf8(buf).expect("completion scripts are UTF-8")
}

#[test]
fn every_shell_generates_a_script() {
    for shell in [
        Shell::Bash,
        Shell::Zsh,
        Shell::Fish,
        Shell::Elvish,
        Shell::PowerShell,
    ] {
        let s = script(shell);
        assert!(!s.is_empty(), "{shell} produced nothing");
        assert!(
            s.contains("seastamp"),
            "{shell} script never names the binary"
        );
    }
}

#[test]
fn bash_script_lists_every_subcommand() {
    let s = script(Shell::Bash);
    for sub in [
        "coast",
        "depth",
        "sea",
        "place",
        "nearest",
        "regions",
        "completions",
    ] {
        assert!(s.contains(sub), "the bash script never mentions '{sub}'");
    }
}

/// `--region` is a `String`, so clap has nothing to offer on its own. The
/// candidate list is fed in by `RegionNameParser`, and it must survive
/// `hide_possible_values`, which keeps the same 109 names out of `--help`.
#[test]
fn bash_script_offers_region_names() {
    let s = script(Shell::Bash);
    for name in ["auto", "baltic", "Adriatic Sea", "Barentsz Sea"] {
        assert!(
            s.contains(name),
            "the bash script never offers region '{name}'"
        );
    }
}

/// The names offered by completion must be names the program accepts. This is
/// what catches an example in the help text that no longer resolves, which is
/// how "Barents Sea" (IHO spells it "Barentsz Sea") went unnoticed.
#[test]
fn every_offered_region_name_resolves() {
    for name in region_names() {
        if name == seastamp::config::AUTO_REGION {
            continue; // auto names no box; it is resolved from the points
        }
        match seastamp::config::region_bbox(name) {
            Ok(_) => {}
            // The four antimeridian crossers are offered on purpose: the error
            // names the workaround. Anything else is a broken candidate.
            Err(e) => assert!(
                e.to_string().contains("antimeridian"),
                "offered region '{name}' does not resolve: {e}"
            ),
        }
    }
}

/// Advertising possible values must not turn `--region` into a closed set: an
/// unknown name has to reach `config::region_bbox` and its word-level hint.
#[test]
fn an_unknown_region_still_parses_and_fails_later() {
    let cli = Cli::try_parse_from(["seastamp", "coast", "in.csv", "--region", "Barent Sea"])
        .expect("clap must accept any --region string, not just the known names");
    let Commands::Coast(args) = cli.command else {
        panic!("expected the coast subcommand")
    };
    assert_eq!(args.region.region.as_deref(), Some("Barent Sea"));

    let err = seastamp::config::region_bbox("Barent Sea")
        .expect_err("an unknown region must be an error");
    assert!(
        err.to_string().contains("Barentsz Sea"),
        "the word-level suggestion was lost: {err}"
    );
}

/// A known limitation, pinned so it is noticed if clap_complete ever fixes it.
///
/// The bash generator emits candidates as one `compgen -W` word list and zsh as
/// one parenthesized list, both split on spaces, so a name like "Adriatic Sea"
/// arrives as two candidates. fish, which puts one candidate per line, is fine.
/// The damage is limited: completing to "Adriatic" and running it produces
/// "unknown region 'Adriatic'. Did you mean: Adriatic Sea?", so the shell's half
/// answer is finished by the error message.
#[test]
fn multi_word_region_names_split_in_bash_and_zsh() {
    assert!(
        script(Shell::Bash).contains("mediterranean Adriatic Sea Aegean"),
        "the bash word list changed shape; re-check whether spaces now survive"
    );
    assert!(
        script(Shell::Fish).contains("Adriatic Sea\\t"),
        "fish should keep a multi-word name on one line"
    );

    let err = seastamp::config::region_bbox("Adriatic").expect_err("half a name is not a region");
    assert!(
        err.to_string().contains("Adriatic Sea"),
        "a truncated completion must still point at the full name: {err}"
    );
}

/// The 109 region names belong in the completion script, not in `--help`.
#[test]
fn region_names_stay_out_of_help() {
    let help = Cli::command()
        .find_subcommand_mut("coast")
        .expect("coast subcommand")
        .render_long_help()
        .to_string();
    assert!(
        help.contains("--region"),
        "the flag itself must be documented"
    );
    assert!(
        !help.contains("Adriatic Sea"),
        "the IHO names leaked into --help, which buries every other flag"
    );
}
