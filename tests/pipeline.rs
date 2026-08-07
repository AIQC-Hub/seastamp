//! End-to-end check of the shared pipeline with a deterministic dummy enricher:
//! it verifies de-duplication of rounded locations, parallel enrichment, the
//! join back to every row, and round-tripping through Parquet.

use seastamp::cli::Format;
use seastamp::config::{Settings, BALTIC};
use seastamp::pipeline::{run_module, Enricher, OutputKind, OutputSpec, Value};
use polars::prelude::*;

/// Appends a float (lon + lat) and a text label, so both column kinds are tested.
struct Dummy;

impl Enricher for Dummy {
    fn outputs(&self) -> Vec<OutputSpec> {
        vec![
            OutputSpec { name: "val".into(), kind: OutputKind::Float },
            OutputSpec { name: "lbl".into(), kind: OutputKind::Text },
        ]
    }
    fn enrich(&self, lon: f64, lat: f64) -> Vec<Value> {
        vec![
            Value::Float(lon + lat),
            Value::Text(Some(format!("{lon:.1},{lat:.1}"))),
        ]
    }
}

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

#[test]
fn appends_columns_and_dedups_rows() {
    // Two identical rows plus one distinct: 3 rows, 2 unique locations.
    let df = df! {
        "longitude" => [18.0f64, 18.0, 24.0],
        "latitude"  => [59.0f64, 59.0, 60.0],
    }
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    run_module(&Dummy, df, &settings(), &out, Format::Parquet).unwrap();

    let back = ParquetReader::new(std::fs::File::open(&out).unwrap())
        .finish()
        .unwrap();
    assert_eq!(back.height(), 3);
    assert_eq!(back.width(), 4); // longitude, latitude, val, lbl

    let val = back.column("val").unwrap().f64().unwrap();
    assert_eq!(val.get(0), Some(77.0));
    assert_eq!(val.get(1), Some(77.0));
    assert_eq!(val.get(2), Some(84.0));

    let lbl = back.column("lbl").unwrap().str().unwrap();
    assert_eq!(lbl.get(0), Some("18.0,59.0"));
    assert_eq!(lbl.get(2), Some("24.0,60.0"));
}

#[test]
fn existing_output_column_errors_without_overwrite() {
    let df = df! {
        "longitude" => [18.0f64],
        "latitude"  => [59.0f64],
        "val"       => [1.0f64],
    }
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let err = run_module(&Dummy, df, &settings(), &out, Format::Parquet)
        .expect_err("a clashing column must fail without --overwrite");
    assert!(err.to_string().contains("--overwrite"), "unexpected error: {err}");
    assert!(err.to_string().contains("'val'"), "unexpected error: {err}");
}

#[test]
fn overwrite_replaces_existing_columns_in_place() {
    // "val" sits between the coordinate columns and holds text, so the test
    // covers both the in-place replacement (position kept) and a dtype change.
    let df = df! {
        "longitude" => [18.0f64, 24.0],
        "val"       => ["old", "old"],
        "latitude"  => [59.0f64, 60.0],
    }
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let mut s = settings();
    s.overwrite = true;
    run_module(&Dummy, df, &s, &out, Format::Parquet).unwrap();

    let back = ParquetReader::new(std::fs::File::open(&out).unwrap())
        .finish()
        .unwrap();
    assert_eq!(back.width(), 4); // val replaced, lbl appended
    assert_eq!(back.get_column_names()[1].as_str(), "val"); // position kept

    let val = back.column("val").unwrap().f64().unwrap();
    assert_eq!(val.get(0), Some(77.0));
    assert_eq!(val.get(1), Some(84.0));
}

#[test]
fn nan_coordinates_get_null_outputs() {
    let df = df! {
        "longitude" => [18.0f64, f64::NAN],
        "latitude"  => [59.0f64, 59.0],
    }
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    run_module(&Dummy, df, &settings(), &out, Format::Parquet).unwrap();

    let back = ParquetReader::new(std::fs::File::open(&out).unwrap())
        .finish()
        .unwrap();
    let val = back.column("val").unwrap().f64().unwrap();
    assert_eq!(val.get(0), Some(77.0));
    // The NaN-coordinate row has no key, so its enrichment is null/NaN.
    assert!(val.get(1).map(|v| v.is_nan()).unwrap_or(true));
}

/// The planar modules warn when the input sits far from the projection center,
/// because a wrong region is otherwise silent: the distances look plausible and
/// are simply wrong. The check is the same radial-scale formula the projection
/// implies, so assert it against known separations rather than on log output.
#[test]
fn laea_radial_error_grows_with_distance_from_center() {
    use seastamp::geo::{haversine_m, projection::MEAN_RADIUS_M};

    // Error of a radial length measured in a LAEA centered at (0, 0):
    // sqrt((1 + cos c) / 2) - 1, where c is the angular distance.
    let err_at = |lon: f64, lat: f64| {
        let c = haversine_m(0.0, 0.0, lon, lat) / MEAN_RADIUS_M;
        ((1.0 + c.cos()) / 2.0).sqrt() - 1.0
    };

    // At the center there is no distortion at all.
    assert!(err_at(0.0, 0.0).abs() < 1e-12);

    // The North Sea against a whole-globe default is off by about 12%, which is
    // exactly the case the warning exists to catch.
    let north_sea = err_at(3.0, 56.0);
    assert!(
        (-0.13..-0.10).contains(&north_sea),
        "expected about -12%, got {north_sea}"
    );

    // Error grows monotonically with distance from the center.
    assert!(err_at(3.0, 20.0).abs() < err_at(3.0, 56.0).abs());
    assert!(err_at(3.0, 56.0).abs() < err_at(160.0, -40.0).abs());
}
