//! Place module against in-memory country and municipality polygons:
//! containment on land, nearest country offshore, the ISO code passthrough,
//! and the empty municipality column when no municipality set is given.

use seastamp::cli::Format;
use seastamp::config::{Settings, BALTIC};
use seastamp::geo::vector::Rings;
use seastamp::geo::Laea;
use seastamp::modules::place::PlaceEnricher;
use seastamp::pipeline::run_module;
use polars::prelude::*;

fn settings() -> Settings {
    Settings {
        lon_col: "longitude".into(),
        lat_col: "latitude".into(),
        decimals: 3,
        threads: None,
        overwrite: false,
        bbox: BALTIC,
        proj_lon0: 19.5,
        proj_lat0: 59.5,
    }
}

fn run_lookup(enr: &PlaceEnricher, df: DataFrame) -> DataFrame {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    run_module(enr, df, &settings(), &out, Format::Parquet).unwrap();
    ParquetReader::new(std::fs::File::open(&out).unwrap())
        .finish()
        .unwrap()
}

/// An axis-aligned closed box as a single-ring polygon.
fn boxed(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Rings {
    vec![vec![
        (min_lon, min_lat),
        (max_lon, min_lat),
        (max_lon, max_lat),
        (min_lon, max_lat),
        (min_lon, min_lat),
    ]]
}

/// Two schematic countries flanking an open strip of sea between 18.5 and 21.5 E.
fn countries() -> Vec<(Rings, (String, Option<String>))> {
    vec![
        (boxed(10.0, 55.0, 18.5, 66.0), ("Sweden".to_string(), Some("SWE".to_string()))),
        (boxed(21.5, 55.0, 30.0, 66.0), ("Finland".to_string(), Some("FIN".to_string()))),
    ]
}

#[test]
fn containment_on_land_and_nearest_country_offshore() {
    let proj = Laea::new(19.5, 59.5);
    let enr = PlaceEnricher::from_features(countries(), None, BALTIC, proj);

    let df = df! {
        // inland Sweden, offshore nearer Sweden, offshore nearer Finland
        "longitude" => [15.0f64, 19.0, 21.0],
        "latitude"  => [59.0f64, 59.0, 59.0],
    }
    .unwrap();

    let back = run_lookup(&enr, df);
    let name = back.column("country").unwrap().str().unwrap();
    let code = back.column("country_code").unwrap().str().unwrap();
    let muni = back.column("municipality").unwrap().str().unwrap();

    assert_eq!(name.get(0), Some("Sweden"));
    assert_eq!(name.get(1), Some("Sweden"));
    assert_eq!(name.get(2), Some("Finland"));
    assert_eq!(code.get(0), Some("SWE"));
    assert_eq!(code.get(2), Some("FIN"));
    // No municipality set given: the column exists but stays empty.
    assert_eq!(muni.get(0), None);
    assert_eq!(muni.get(1), None);
}

#[test]
fn nearest_municipality_offshore() {
    let proj = Laea::new(19.5, 59.5);
    let munis = vec![
        (boxed(17.8, 59.2, 18.3, 59.6), "Stockholm".to_string()),
        (boxed(24.5, 60.0, 25.5, 60.4), "Helsinki".to_string()),
    ];
    let enr = PlaceEnricher::from_features(countries(), Some(munis), BALTIC, proj);

    let df = df! {
        "longitude" => [19.0f64, 23.0],
        "latitude"  => [59.0f64, 59.8],
    }
    .unwrap();

    let back = run_lookup(&enr, df);
    let muni = back.column("municipality").unwrap().str().unwrap();
    assert_eq!(muni.get(0), Some("Stockholm"));
    assert_eq!(muni.get(1), Some("Helsinki"));
}

#[test]
fn missing_country_code_stays_null() {
    let proj = Laea::new(19.5, 59.5);
    let feats = vec![(boxed(10.0, 55.0, 18.5, 66.0), ("Atlantis".to_string(), None))];
    let enr = PlaceEnricher::from_features(feats, None, BALTIC, proj);

    let df = df! { "longitude" => [15.0f64], "latitude" => [59.0f64] }.unwrap();
    let back = run_lookup(&enr, df);
    assert_eq!(
        back.column("country").unwrap().str().unwrap().get(0),
        Some("Atlantis")
    );
    assert_eq!(back.column("country_code").unwrap().str().unwrap().get(0), None);
}
