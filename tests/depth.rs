//! Depth module against a small synthetic GEBCO-style NetCDF grid: verifies
//! nearest-cell indexing, the negative-elevation sign convention and the
//! `--positive` flip, and out-of-grid points yielding NaN. Building a tiny grid
//! in the test avoids needing the multi-gigabyte real GEBCO file.

use seastamp::cli::Format;
use seastamp::config::{Settings, BALTIC};
use seastamp::modules::depth::DepthEnricher;
use seastamp::pipeline::run_module;
use polars::prelude::*;

/// Write a 3x4 grid: lat = [58, 59, 60], lon = [18, 19, 20, 21], elevation
/// (lat, lon) row-major set to -(i*100 + j*10 + 5) so every cell is a distinct
/// negative value with no zeros to confuse the sign assertions.
fn make_grid(path: &std::path::Path) {
    // Silence HDF5's error-stack printing before writing: netcdf-c probes for
    // optional attributes here too, and this write happens before any open().
    seastamp::modules::depth::silence_hdf5_diagnostics();

    let lats = [58.0f64, 59.0, 60.0];
    let lons = [18.0f64, 19.0, 20.0, 21.0];

    let mut file = netcdf::create(path).unwrap();
    file.add_dimension("lat", lats.len()).unwrap();
    file.add_dimension("lon", lons.len()).unwrap();
    file.add_variable::<f64>("lat", &["lat"]).unwrap().put_values(&lats, ..).unwrap();
    file.add_variable::<f64>("lon", &["lon"]).unwrap().put_values(&lons, ..).unwrap();

    let mut data = Vec::with_capacity(lats.len() * lons.len());
    for i in 0..lats.len() {
        for j in 0..lons.len() {
            data.push(-((i * 100 + j * 10 + 5) as f64));
        }
    }
    file.add_variable::<f64>("elevation", &["lat", "lon"])
        .unwrap()
        .put_values(&data, ..)
        .unwrap();
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
    }
}

fn run_lookup(nc: &std::path::Path, positive: bool, on_land: bool, df: DataFrame) -> DataFrame {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let enr = DepthEnricher::open(nc, "bathymetry".into(), positive, on_land).unwrap();
    run_module(&enr, df, &settings(), &out, Format::Parquet).unwrap();
    ParquetReader::new(std::fs::File::open(&out).unwrap())
        .finish()
        .unwrap()
}

/// Every case runs from this one test on purpose. The harness gives each `#[test]`
/// its own thread, and a serial HDF5 build aborts when entered from more than one
/// thread, so separate test functions made `cargo test --features static-netcdf`
/// crash at random even though the library code was sound. One test means one
/// thread touches HDF5 in this binary. A mutex would not do: the calls have to be
/// on the same thread, not merely non-overlapping.
#[test]
fn depth_grid_lookup() {
    nearest_cell_and_out_of_grid();
    many_points_do_not_crash_the_grid_reader();
    positive_flips_sign();
    on_land_flags_positive_elevation();
}

/// A 2x2 grid mixing sea and land: lat = [58, 59], lon = [18, 19], with the
/// (58, 19) cell above sea level so `on_land` has something true to find.
fn make_grid_with_land(path: &std::path::Path) {
    seastamp::modules::depth::silence_hdf5_diagnostics();
    let lats = [58.0f64, 59.0];
    let lons = [18.0f64, 19.0];
    let mut file = netcdf::create(path).unwrap();
    file.add_dimension("lat", lats.len()).unwrap();
    file.add_dimension("lon", lons.len()).unwrap();
    file.add_variable::<f64>("lat", &["lat"]).unwrap().put_values(&lats, ..).unwrap();
    file.add_variable::<f64>("lon", &["lon"]).unwrap().put_values(&lons, ..).unwrap();
    // (lat, lon) row-major: sea, land / sea, sea
    let data = [-100.0f64, 250.0, -300.0, -400.0];
    file.add_variable::<f64>("elevation", &["lat", "lon"])
        .unwrap()
        .put_values(&data, ..)
        .unwrap();
}

/// `--on-land` reads the raw GEBCO elevation, so it means the same thing with and
/// without `--positive`, which inverts the reported number. Off-grid points get a
/// null rather than `false`, since there is no reading to judge them by, and the
/// depth value is still reported on land rather than nulled.
fn on_land_flags_positive_elevation() {
    let dir = tempfile::tempdir().unwrap();
    let nc = dir.path().join("land.nc");
    make_grid_with_land(&nc);

    let df = df! {
        // sea cell, land cell, off-grid
        "longitude" => [18.0f64, 19.0, 100.0],
        "latitude"  => [58.0f64, 58.0, 58.0],
    }
    .unwrap();

    for positive in [false, true] {
        let back = run_lookup(&nc, positive, true, df.clone());
        let land = back.column("on_land").unwrap().bool().unwrap();
        assert_eq!(land.get(0), Some(false), "negative elevation is sea");
        assert_eq!(land.get(1), Some(true), "positive elevation is land");
        assert_eq!(land.get(2), None, "off-grid has no reading");

        let b = back.column("bathymetry").unwrap().f64().unwrap();
        let expect = if positive { -250.0 } else { 250.0 };
        assert_eq!(b.get(1), Some(expect), "land elevation is still reported");
    }

    // Without the flag the column does not exist at all.
    let plain = run_lookup(&nc, false, false, df);
    assert!(plain.column("on_land").is_err(), "no on_land column by default");
}

fn nearest_cell_and_out_of_grid() {
    let dir = tempfile::tempdir().unwrap();
    let nc = dir.path().join("grid.nc");
    make_grid(&nc);

    let df = df! {
        // exact center, exact center, off-center (rounds to nearest cell), off-grid
        "longitude" => [19.0f64, 21.0, 18.6, 100.0],
        "latitude"  => [59.0f64, 60.0, 58.0, 59.0],
    }
    .unwrap();

    let back = run_lookup(&nc, false, false, df);
    let b = back.column("bathymetry").unwrap().f64().unwrap();
    assert_eq!(b.get(0), Some(-115.0)); // i=1, j=1 -> -(100+10+5)
    assert_eq!(b.get(1), Some(-235.0)); // i=2, j=3 -> -(200+30+5)
    assert_eq!(b.get(2), Some(-15.0)); // 18.6 rounds to j=1, i=0 -> -(0+10+5)
    assert!(b.get(3).map(|v| v.is_nan()).unwrap_or(true)); // lon 100 off grid
}

/// Regression test for a segfault: enriching a few hundred points crashed, while
/// a handful got through. The pipeline used to spread locations across rayon
/// workers, and a serial HDF5 build cannot be entered from more than one thread
/// even with every call under a mutex, so `depth` now enriches single-threaded.
///
/// This only reproduces against a serial HDF5, so it passes either way on a
/// system HDF5 built thread-safe (Ubuntu's is). To exercise the configuration
/// that actually crashed, and that the release binaries ship, run it with the
/// vendored library: `cargo test --features static-netcdf --test depth`.
fn many_points_do_not_crash_the_grid_reader() {
    let dir = tempfile::tempdir().unwrap();
    let nc = dir.path().join("grid.nc");
    make_grid(&nc);

    // Distinct to 3 decimals so the pipeline keeps them as separate locations,
    // and inside the grid so each one performs a real HDF5 read.
    let n = 2000;
    let lons: Vec<f64> = (0..n).map(|k| 18.0 + (k as f64) * 0.001).collect();
    let lats: Vec<f64> = (0..n).map(|k| 58.0 + ((k % 2000) as f64) * 0.001).collect();
    let df = df! { "longitude" => lons, "latitude" => lats }.unwrap();

    let back = run_lookup(&nc, false, false, df);
    let b = back.column("bathymetry").unwrap().f64().unwrap();
    assert_eq!(b.len(), n);
    assert!(b.into_no_null_iter().all(|v| v < 0.0), "every in-grid cell is negative");
}

fn positive_flips_sign() {
    let dir = tempfile::tempdir().unwrap();
    let nc = dir.path().join("grid.nc");
    make_grid(&nc);

    let df = df! {
        "longitude" => [19.0f64],
        "latitude"  => [59.0f64],
    }
    .unwrap();

    let back = run_lookup(&nc, true, false, df);
    let b = back.column("bathymetry").unwrap().f64().unwrap();
    assert_eq!(b.get(0), Some(115.0)); // -(-115) with --positive
}
