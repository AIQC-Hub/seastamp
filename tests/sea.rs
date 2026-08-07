//! Sea module against in-memory IHO-style polygons and a small GeoJSON file:
//! containment, the nearest-boundary fallback for points inside no polygon,
//! cropping of features outside the region, and the GeoJSON open path.

use seastamp::cli::Format;
use seastamp::config::{Settings, BALTIC};
use seastamp::geo::vector::Rings;
use seastamp::geo::Laea;
use seastamp::modules::sea::SeaEnricher;
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
        auto: None,
        partition: false,
    }
}

fn run_lookup(enr: &SeaEnricher, df: DataFrame) -> DataFrame {
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

#[test]
fn containment_and_nearest_fallback() {
    let proj = Laea::new(19.5, 59.5);
    let feats = vec![
        (boxed(16.0, 57.0, 20.0, 61.0), "West Basin".to_string()),
        (boxed(20.0, 57.0, 24.0, 61.0), "East Basin".to_string()),
    ];
    let enr = SeaEnricher::from_features(feats, BALTIC, proj, "sea_name".into());

    let df = df! {
        // in West Basin, in East Basin, in neither (fallback to nearest boundary)
        "longitude" => [18.0f64, 22.0, 26.0],
        "latitude"  => [59.0f64, 59.0, 59.0],
    }
    .unwrap();

    let back = run_lookup(&enr, df);
    let s = back.column("sea_name").unwrap().str().unwrap();
    assert_eq!(s.get(0), Some("West Basin"));
    assert_eq!(s.get(1), Some("East Basin"));
    assert_eq!(s.get(2), Some("East Basin")); // 26 E: nearest boundary is East's
}

#[test]
fn features_outside_region_are_cropped() {
    let proj = Laea::new(19.5, 59.5);
    // The only feature is far outside the Baltic box plus margin, so it is
    // dropped and every lookup comes back null.
    let feats = vec![(boxed(100.0, 0.0, 105.0, 5.0), "Far Sea".to_string())];
    let enr = SeaEnricher::from_features(feats, BALTIC, proj, "sea_name".into());

    let df = df! { "longitude" => [20.0f64], "latitude" => [59.0f64] }.unwrap();
    let back = run_lookup(&enr, df);
    let s = back.column("sea_name").unwrap().str().unwrap();
    assert_eq!(s.get(0), None);
}

#[test]
fn open_geojson_reads_named_features() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seas.geojson");
    std::fs::write(
        &path,
        r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"NAME":"Test Sea"},
             "geometry":{"type":"Polygon",
               "coordinates":[[[16,57],[24,57],[24,61],[16,61],[16,57]]]}},
            {"type":"Feature","properties":{"NAME":"Split Sea"},
             "geometry":{"type":"MultiPolygon",
               "coordinates":[[[[10,53],[11,53],[11,54],[10,54],[10,53]]],
                              [[[12,53],[13,53],[13,54],[12,54],[12,53]]]]}}
        ]}"#,
    )
    .unwrap();

    let proj = Laea::new(19.5, 59.5);
    let enr = SeaEnricher::open(&path, "NAME", BALTIC, proj, "sea_name".into()).unwrap();

    let df = df! {
        "longitude" => [20.0f64, 12.5],
        "latitude"  => [59.0f64, 53.5],
    }
    .unwrap();
    let back = run_lookup(&enr, df);
    let s = back.column("sea_name").unwrap().str().unwrap();
    assert_eq!(s.get(0), Some("Test Sea"));
    assert_eq!(s.get(1), Some("Split Sea")); // second MultiPolygon part
}

#[test]
fn open_rejects_wrong_name_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seas.geojson");
    std::fs::write(
        &path,
        r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"NAME":"Test Sea"},
             "geometry":{"type":"Polygon",
               "coordinates":[[[16,57],[24,57],[24,61],[16,61],[16,57]]]}}
        ]}"#,
    )
    .unwrap();

    let proj = Laea::new(19.5, 59.5);
    let err = SeaEnricher::open(&path, "NOPE", BALTIC, proj, "sea_name".into())
        .err()
        .expect("wrong field must fail");
    assert!(err.to_string().contains("NOPE"), "unexpected error: {err}");
}
