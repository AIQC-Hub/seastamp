//! `report stats` and `report summary` end to end, through the CLI parser: the
//! location reduction matches the stamping pipeline's, the bands and the land
//! split come out right under both depth sign conventions, `--by` breaks the
//! counts down, and the summary page reads the TSV back.

use std::fs;
use std::path::Path;

use clap::Parser;
use polars::prelude::*;
use seastamp::cli::Cli;

fn run(args: &[&str]) {
    let mut v = vec!["seastamp"];
    v.extend_from_slice(args);
    seastamp::run(Cli::parse_from(v)).unwrap();
}

/// A small stamped table: three rows share one rounded location, one row has
/// no position, one location has no stamped values at all.
fn stamped(dir: &Path, positive: bool) -> String {
    let sign = if positive { -1.0 } else { 1.0 };
    let mut df = df! {
        "platform_code" => ["A", "A", "A", "A", "B", "B", "B", "B", "C"],
        "longitude" => [Some(10.0001), Some(10.0002), Some(10.0), Some(11.0), Some(11.0), Some(12.0), Some(13.0), None, Some(14.0)],
        "latitude"  => [60.0, 60.0, 60.0, 61.0, 61.0, 62.0, 63.0, 63.0, 64.0],
        "dist_to_coast" => [Some(0.5), Some(0.5), Some(0.5), Some(12.0), Some(12.0), Some(150.0), Some(3.0), None, None],
        "bathymetry" => [-5.0, -5.0, -5.0, -150.0, -150.0, -2500.0, 20.0, f64::NAN, f64::NAN]
            .iter().map(|v| v * sign).collect::<Vec<f64>>(),
        "sea_name" => [Some("North Sea"), Some("North Sea"), Some("North Sea"), Some("North Sea"), Some("North Sea"), Some("Norwegian Sea"), None, None, None],
    }
    .unwrap();
    let p = dir.join("t.parquet");
    ParquetWriter::new(fs::File::create(&p).unwrap()).finish(&mut df).unwrap();
    p.to_string_lossy().into_owned()
}

/// The report as (column, item) -> (rows, locations, min, max, mean) cells.
fn rows_of(path: &Path) -> Vec<Vec<String>> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

fn find<'a>(rows: &'a [Vec<String>], col: &str, item: &str) -> &'a Vec<String> {
    rows.iter()
        .find(|r| r[0] == col && r[2] == item)
        .unwrap_or_else(|| panic!("no row {col} / {item}"))
}

#[test]
fn stats_counts_rows_and_locations() {
    let dir = tempfile::tempdir().unwrap();
    let input = stamped(dir.path(), false);
    let out = dir.path().join("r.tsv");
    run(&["report", "stats", &input, out.to_str().unwrap()]);
    let r = rows_of(&out);

    // 8 located rows on 5 locations (10.0001 / 10.0002 / 10.0 round together).
    assert_eq!(find(&r, "(all)", "(located)")[4..6], ["8", "5"]);
    assert_eq!(find(&r, "(all)", "(no position)")[4..6], ["1", "0"]);

    // Statistics over locations, not rows: mean of 0.5, 12, 150, 3.
    let v = find(&r, "dist_to_coast", "(value)");
    assert_eq!(v[4..9], ["7", "4", "0.500", "150.000", "41.375"]);
    assert_eq!(find(&r, "dist_to_coast", "(null)")[4..6], ["1", "1"]);
    assert_eq!(find(&r, "dist_to_coast", "< 1")[4..6], ["3", "1"]);
    assert_eq!(find(&r, "dist_to_coast", "20-100")[4..6], ["0", "0"]);
    assert_eq!(find(&r, "dist_to_coast", ">= 100")[4..6], ["1", "1"]);

    // Elevation stored as is: +20 m is land, -5 m is the < 10 m band.
    assert_eq!(find(&r, "bathymetry", "land")[4..6], ["1", "1"]);
    assert_eq!(find(&r, "bathymetry", "< 10")[4..6], ["3", "1"]);
    assert_eq!(find(&r, "bathymetry", ">= 1000")[4..6], ["1", "1"]);

    // Names by locations, most first, nulls last.
    let names: Vec<&str> = r.iter().filter(|x| x[0] == "sea_name").map(|x| x[2].as_str()).collect();
    assert_eq!(names, ["North Sea", "Norwegian Sea", "(null)"]);
    assert_eq!(find(&r, "sea_name", "North Sea")[4..6], ["5", "2"]);
}

#[test]
fn depth_positive_flips_the_land_test() {
    let dir = tempfile::tempdir().unwrap();
    let input = stamped(dir.path(), true);
    let out = dir.path().join("r.tsv");
    run(&["report", "stats", "--depth-positive", &input, out.to_str().unwrap()]);
    let r = rows_of(&out);
    assert_eq!(find(&r, "bathymetry", "land")[4..6], ["1", "1"]);
    assert_eq!(find(&r, "bathymetry", "< 10")[4..6], ["3", "1"]);
}

#[test]
fn metre_distances_scale_the_km_band_edges() {
    let dir = tempfile::tempdir().unwrap();
    let input = stamped(dir.path(), false);
    let out = dir.path().join("r.tsv");
    run(&["report", "stats", "--coast-unit", "m", &input, out.to_str().unwrap()]);
    let r = rows_of(&out);
    // Edges 1, 5, 20, 100 km become 1000 ... 100000 m; every value is now < 1000.
    assert_eq!(find(&r, "dist_to_coast", "< 1000")[4..6], ["7", "4"]);
    assert_eq!(find(&r, "dist_to_coast", "< 1000")[3], "m");
}

#[test]
fn by_breaks_the_report_down_per_group() {
    let dir = tempfile::tempdir().unwrap();
    let input = stamped(dir.path(), false);
    let out = dir.path().join("r.tsv");
    run(&["report", "stats", "--by", "platform_code", &input, out.to_str().unwrap()]);
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("platform_code\tcolumn\t"));
    let r: Vec<Vec<String>> = rows_of(&out);
    let located = |g: &str| {
        r.iter()
            .find(|x| x[0] == g && x[1] == "(all)" && x[3] == "(located)")
            .map(|x| (x[5].clone(), x[6].clone()))
            .unwrap()
    };
    // (11, 61) is in both A and B, so it is a location of each.
    assert_eq!(located("A"), ("4".into(), "2".into()));
    assert_eq!(located("B"), ("3".into(), "3".into()));
    assert_eq!(located("C"), ("1".into(), "1".into()));
    // Groups in value order.
    let order: Vec<&str> = r.iter().filter(|x| x[3] == "(located)").map(|x| x[0].as_str()).collect();
    assert_eq!(order, ["A", "B", "C"]);
}

#[test]
fn summary_renders_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let input = stamped(dir.path(), false);
    let rdir = dir.path().join("report");
    fs::create_dir_all(rdir.join("stamp")).unwrap();
    let tsv = rdir.join("stamp").join("t.tsv");
    run(&["report", "stats", &input, tsv.to_str().unwrap()]);

    let md = dir.path().join("t.md");
    run(&[
        "report", "summary", "t", "--report-dir", rdir.to_str().unwrap(),
        "--title", "Test page", "--note", "First note.", "-o", md.to_str().unwrap(),
    ]);
    let page = fs::read_to_string(&md).unwrap();
    assert!(page.starts_with("# Test page\n\n> First note.\n"));
    for heading in ["## Overview", "## Distance to coast", "## Depth", "## Sea area"] {
        assert!(page.contains(heading), "missing {heading}");
    }
    assert!(!page.contains("## Place"), "no place columns, so no Place section");
    assert!(page.contains("| With a position | 8 | 5 |"));
    assert!(page.contains("| Land (at or above sea level) | 1 | 20.00% | 1 | 12.50% |"));
    assert!(!page.contains('\u{2014}'), "no em dashes in human-facing text");

    let html = dir.path().join("t.html");
    run(&[
        "report", "summary", "t", "--report-dir", rdir.to_str().unwrap(),
        "-f", "html", "-o", html.to_str().unwrap(),
    ]);
    let page = fs::read_to_string(&html).unwrap();
    assert!(page.contains("<h1>Summary: t</h1>"));
    assert!(page.contains("<td>&lt; 1</td>"), "band labels are escaped");
}

#[test]
fn summary_refuses_a_by_report() {
    let dir = tempfile::tempdir().unwrap();
    let input = stamped(dir.path(), false);
    let rdir = dir.path().join("report");
    fs::create_dir_all(rdir.join("stamp")).unwrap();
    let tsv = rdir.join("stamp").join("t.tsv");
    run(&["report", "stats", "--by", "platform_code", &input, tsv.to_str().unwrap()]);
    let cli = Cli::parse_from(["seastamp", "report", "summary", "t", "--report-dir", rdir.to_str().unwrap()]);
    let err = seastamp::run(cli).unwrap_err().to_string();
    assert!(err.contains("--by platform_code"), "{err}");
}
