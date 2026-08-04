//! Nearest module against an in-memory reference set (no file on disk): verifies
//! that each input point picks the closest reference location, that the reported
//! distance matches the great-circle distance globally (even across the world),
//! the km/m unit switch, and that an empty reference set yields null/NaN.

use seastamp::cli::{DistUnit, Format};
use seastamp::config::{Settings, GLOBAL};
use seastamp::geo::haversine_m;
use seastamp::modules::nearest::NearestEnricher;
use seastamp::pipeline::run_module;
use polars::prelude::*;

fn settings() -> Settings {
    Settings {
        lon_col: "longitude".into(),
        lat_col: "latitude".into(),
        decimals: 3,
        threads: None,
        overwrite: false,
        bbox: GLOBAL,
        proj_lon0: 0.0,
        proj_lat0: 0.0,
        auto: None,
    }
}

fn run_lookup(enr: &NearestEnricher, df: DataFrame) -> DataFrame {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    run_module(enr, df, &settings(), &out, Format::Parquet).unwrap();
    ParquetReader::new(std::fs::File::open(&out).unwrap())
        .finish()
        .unwrap()
}

/// Three named reference locations spread across the globe.
fn farms() -> Vec<(f64, f64, Option<String>)> {
    vec![
        (18.0686, 59.3293, Some("Stockholm".into())),
        (151.2093, -33.8688, Some("Sydney".into())),
        (-74.0060, 40.7128, Some("New York".into())),
    ]
}

#[test]
fn picks_nearest_reference_and_reports_great_circle_distance() {
    let enr = NearestEnricher::from_points(
        farms(),
        DistUnit::Km,
        "nearest_name".into(),
        "nearest_dist".into(),
    );

    // Helsinki is nearest Stockholm; Melbourne nearest Sydney. Coordinates are
    // given at 3 decimals, the pipeline's rounding precision, so the enriched
    // point equals what we compute the expected distance from below.
    let df = df! {
        "longitude" => [24.938f64, 144.963],
        "latitude"  => [60.170f64, -37.814],
    }
    .unwrap();

    let back = run_lookup(&enr, df);
    let name = back.column("nearest_name").unwrap().str().unwrap();
    let dist = back.column("nearest_dist").unwrap().f64().unwrap();

    assert_eq!(name.get(0), Some("Stockholm"));
    assert_eq!(name.get(1), Some("Sydney"));

    // The distance is the exact great-circle distance to that reference point.
    let hel = haversine_m(24.938, 60.170, 18.0686, 59.3293) / 1000.0;
    let mel = haversine_m(144.963, -37.814, 151.2093, -33.8688) / 1000.0;
    assert!((dist.get(0).unwrap() - hel).abs() < 1e-6, "{:?} vs {hel}", dist.get(0));
    assert!((dist.get(1).unwrap() - mel).abs() < 1e-6, "{:?} vs {mel}", dist.get(1));
}

#[test]
fn unit_meters_is_km_times_1000() {
    let df = df! { "longitude" => [24.9384f64], "latitude" => [60.1699f64] }.unwrap();
    let km = NearestEnricher::from_points(farms(), DistUnit::Km, "n".into(), "d".into());
    let m = NearestEnricher::from_points(farms(), DistUnit::M, "n".into(), "d".into());

    let dk = run_lookup(&km, df.clone());
    let dm = run_lookup(&m, df);
    let vk = dk.column("d").unwrap().f64().unwrap().get(0).unwrap();
    let vm = dm.column("d").unwrap().f64().unwrap().get(0).unwrap();
    assert!((vm - vk * 1000.0).abs() < 1e-3, "m={vm} km={vk}");
}

#[test]
fn empty_reference_set_yields_null_and_nan() {
    let enr = NearestEnricher::from_points(
        Vec::<(f64, f64, Option<String>)>::new(),
        DistUnit::Km,
        "nearest_name".into(),
        "nearest_dist".into(),
    );
    let df = df! { "longitude" => [10.0f64], "latitude" => [50.0f64] }.unwrap();
    let back = run_lookup(&enr, df);
    assert!(back.column("nearest_name").unwrap().str().unwrap().get(0).is_none());
    let d = back.column("nearest_dist").unwrap().f64().unwrap().get(0);
    assert!(d.map(|v| v.is_nan()).unwrap_or(true), "expected NaN, got {d:?}");
}
