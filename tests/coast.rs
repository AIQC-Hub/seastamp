//! Coast module geometry against in-memory shoreline rings (no shapefile on
//! disk): verifies that a point on the coast reads ~0, that an offshore point's
//! projected distance tracks the great-circle distance, the km/m unit switch,
//! and that segments outside the region-plus-margin box are cropped away.

use seastamp::cli::{DistUnit, Format};
use seastamp::config::{Settings, BALTIC};
use seastamp::geo::{haversine_m, Laea};
use seastamp::modules::coast::CoastEnricher;
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

fn run_lookup(enr: &CoastEnricher, df: DataFrame) -> DataFrame {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    run_module(enr, df, &settings(), &out, Format::Parquet).unwrap();
    ParquetReader::new(std::fs::File::open(&out).unwrap())
        .finish()
        .unwrap()
}

/// A single north-south coastline segment along the 20 deg meridian.
fn meridian_coast() -> Vec<Vec<(f64, f64)>> {
    vec![vec![(20.0, 58.0), (20.0, 60.0)]]
}

#[test]
fn on_coast_is_zero_and_offshore_matches_great_circle() {
    let proj = Laea::new(19.5, 59.5);
    let enr = CoastEnricher::from_rings(
        meridian_coast(),
        BALTIC,
        proj,
        DistUnit::Km,
        "dist_to_coast".into(),
    );

    let df = df! {
        "longitude" => [20.0f64, 21.0],
        "latitude"  => [59.0f64, 59.0],
    }
    .unwrap();

    let back = run_lookup(&enr, df);
    let d = back.column("dist_to_coast").unwrap().f64().unwrap();

    // On the segment: essentially zero.
    assert!(d.get(0).unwrap() < 0.01, "on-coast dist = {:?} km", d.get(0));

    // 1 deg east at lat 59: nearest point is (20, 59), so the planar distance
    // should track the great-circle distance to within the LAEA's regional error.
    let expected_km = haversine_m(21.0, 59.0, 20.0, 59.0) / 1000.0;
    let got = d.get(1).unwrap();
    let rel = (got - expected_km).abs() / expected_km;
    assert!(rel < 0.02, "offshore {got} km vs great-circle {expected_km} km (rel {rel})");
}

#[test]
fn unit_meters_is_km_times_1000() {
    let proj = Laea::new(19.5, 59.5);
    let df = df! { "longitude" => [21.0f64], "latitude" => [59.0f64] }.unwrap();

    let km = CoastEnricher::from_rings(meridian_coast(), BALTIC, proj, DistUnit::Km, "d".into());
    let m = CoastEnricher::from_rings(meridian_coast(), BALTIC, proj, DistUnit::M, "d".into());

    let dk = run_lookup(&km, df.clone());
    let dm = run_lookup(&m, df);
    let vk = dk.column("d").unwrap().f64().unwrap().get(0).unwrap();
    let vm = dm.column("d").unwrap().f64().unwrap().get(0).unwrap();
    assert!((vm - vk * 1000.0).abs() < 1.0, "m={vm} km={vk}");
}

#[test]
fn segments_outside_region_are_cropped() {
    let proj = Laea::new(19.5, 59.5);
    // Only coastline is far outside the Baltic box plus margin, so it is dropped
    // and the R-tree is empty: every lookup is NaN.
    let far = vec![vec![(100.0, 0.0), (100.0, 5.0)]];
    let enr = CoastEnricher::from_rings(far, BALTIC, proj, DistUnit::Km, "dist_to_coast".into());

    let df = df! { "longitude" => [20.0f64], "latitude" => [59.0f64] }.unwrap();
    let back = run_lookup(&enr, df);
    let d = back.column("dist_to_coast").unwrap().f64().unwrap();
    assert!(d.get(0).map(|v| v.is_nan()).unwrap_or(true), "expected NaN, got {:?}", d.get(0));
}
