//! Every stamping command end to end through the CLI parser, against reference
//! files written on the fly: tiny GSHHG-, IHO-, Natural Earth- and GISCO-shaped
//! shapefiles, and GeoJSON. The geometry has its own tests; these cover what sits
//! between the flags and the geometry: argument checks, config files, reading
//! the reference data, `--region` and its overrides, `--partition`, default
//! output names, and the input and output formats.
//!
//! The fixture world is one square country on lon 10..12, lat 55..57, with a
//! municipality in its south-west quarter, and a sea east of it on lon 12..16.
//! The GSHHG shoreline is the country's outline, so the coast at lat 56 is the
//! meridian lon 12.

use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use polars::prelude::*;
use seastamp::cli::{Cli, Format};
use seastamp::geo::haversine_m;
use shapefile::dbase::{FieldValue, Record, TableWriterBuilder};
use shapefile::{Point, Polygon, PolygonRing};

type Box4 = (f64, f64, f64, f64);

const LAND: Box4 = (10.0, 55.0, 12.0, 57.0);
const MUNI: Box4 = (10.0, 55.0, 11.0, 56.0);
const SEA: Box4 = (12.0, 55.0, 16.0, 57.0);

fn run(args: &[&str]) -> Result<(), String> {
    let mut v = vec!["seastamp"];
    v.extend_from_slice(args);
    seastamp::run(Cli::parse_from(v)).map_err(|e| e.to_string())
}

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn read(path: &Path) -> DataFrame {
    seastamp::io::read_frame(path, Format::Auto).unwrap()
}

fn f64s(df: &DataFrame, col: &str) -> Vec<Option<f64>> {
    df.column(col).unwrap().f64().unwrap().into_iter().collect()
}

fn strs(df: &DataFrame, col: &str) -> Vec<Option<String>> {
    df.column(col)
        .unwrap()
        .str()
        .unwrap()
        .into_iter()
        .map(|s| s.map(str::to_string))
        .collect()
}

/// A polygon shapefile with one axis-aligned box per feature and character
/// attributes, `values` in the order of `fields`.
fn write_boxes(path: &Path, fields: &[&str], features: &[(Box4, &[&str])]) {
    let mut table = TableWriterBuilder::new();
    for f in fields {
        table = table.add_character_field((*f).try_into().unwrap(), 40);
    }
    let mut w = shapefile::Writer::from_path(path, table).unwrap();
    for &((x0, y0, x1, y1), values) in features {
        let ring = PolygonRing::Outer(vec![
            Point::new(x0, y0),
            Point::new(x0, y1),
            Point::new(x1, y1),
            Point::new(x1, y0),
            Point::new(x0, y0),
        ]);
        let mut rec = Record::default();
        for (f, v) in fields.iter().zip(values) {
            rec.insert(f.to_string(), FieldValue::Character(Some(v.to_string())));
        }
        w.write_shape_and_record(&Polygon::new(ring), &rec).unwrap();
    }
}

/// Four located rows (two sharing a position, so the reduction to unique
/// locations is exercised) and one with no position.
fn points(dir: &Path) -> PathBuf {
    let path = dir.join("points.csv");
    fs::write(
        &path,
        "id,longitude,latitude\n\
         1,10.5,55.5\n\
         2,11.5,56.5\n\
         3,13.0,56.0\n\
         4,13.0,56.0\n\
         5,,\n",
    )
    .unwrap();
    path
}

/// Great-circle km from the sea points (13, 56) to the coast at (12, 56).
fn sea_point_km() -> f64 {
    haversine_m(13.0, 56.0, 12.0, 56.0) / 1000.0
}

fn assert_close(got: f64, want: f64, rel: f64) {
    assert!(
        (got - want).abs() <= want.abs() * rel,
        "got {got}, want {want} (within {:.1}%)",
        rel * 100.0
    );
}

// ---------------------------------------------------------------- coast

fn gshhg_dir(dir: &Path) -> PathBuf {
    let d = dir.join("gshhg");
    fs::create_dir_all(&d).unwrap();
    // A stray file first, so the directory search has to skip it.
    fs::write(d.join("README.txt"), "not a shapefile").unwrap();
    write_boxes(&d.join("GSHHS_t_L1.shp"), &["id"], &[(LAND, &["1"])]);
    d
}

#[test]
fn coast_reads_a_gshhg_directory_and_names_the_output_after_the_input() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());
    let data = gshhg_dir(dir.path());

    // No --output: the result lands beside the input as points.coast.csv.
    run(&["coast", p(&input), "--data", p(&data)]).unwrap();
    let df = read(&dir.path().join("points.coast.csv"));
    assert_eq!(df.height(), 5, "every input row comes back");

    let d = f64s(&df, "dist_to_coast");
    assert_close(d[2].unwrap(), sea_point_km(), 0.01);
    assert_eq!(d[2], d[3], "rows sharing a location share a value");
    // Half a degree of longitude from the eastern shore.
    assert_close(d[1].unwrap(), haversine_m(11.5, 56.5, 12.0, 56.5) / 1000.0, 0.01);
    // A row with no position has no key: null or NaN, as the pipeline documents.
    assert!(d[4].map_or(true, f64::is_nan), "no position, no distance");
}

#[test]
fn coast_partitioned_in_metres_to_gzip_tsv() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());
    let data = gshhg_dir(dir.path()).join("GSHHS_t_L1.shp");
    let out = dir.path().join("out.tsv.gz");

    run(&[
        "coast", p(&input), "--data", p(&data), "--partition",
        "--unit", "m", "--column", "shore_m", "-o", p(&out),
    ])
    .unwrap();
    let d = f64s(&read(&out), "shore_m");
    assert_close(d[2].unwrap(), sea_point_km() * 1000.0, 0.01);
}

#[test]
fn coast_with_explicit_bounds_and_a_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());
    let data = gshhg_dir(dir.path());
    let cfg = dir.path().join("seastamp.toml");
    // The file names the region; the flags then narrow it. Flags win.
    fs::write(&cfg, "region = \"baltic\"\nproj_lon0 = 12.0\nproj_lat0 = 56.0\n").unwrap();
    let out = dir.path().join("out.parquet");

    run(&[
        "coast", p(&input), "--data", p(&data), "-c", p(&cfg),
        "--min-lon", "9", "--max-lon", "17", "-o", p(&out),
    ])
    .unwrap();
    let d = f64s(&read(&out), "dist_to_coast");
    assert_close(d[2].unwrap(), sea_point_km(), 0.01);

    // A named preset alone works too. Its projection is centered elsewhere, so
    // the planar distance differs slightly; within 1% of the great circle.
    let out2 = dir.path().join("out2.parquet");
    run(&["coast", p(&input), "--data", p(&data), "--region", "baltic", "-o", p(&out2)]).unwrap();
    assert_close(f64s(&read(&out2), "dist_to_coast")[2].unwrap(), sea_point_km(), 0.01);
}

#[test]
fn coast_refuses_bad_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());
    let data = gshhg_dir(dir.path());

    let err = run(&["coast", p(&input)]).unwrap_err();
    assert!(err.contains("--data"), "{err}");

    let empty = dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    let err = run(&["coast", p(&input), "--data", p(&empty)]).unwrap_err();
    assert!(err.contains("no GSHHS_*_L1.shp"), "{err}");

    let err = run(&["coast", p(&input), "--data", p(&dir.path().join("nope"))]).unwrap_err();
    assert!(err.contains("not found"), "{err}");

    let err = run(&[
        "coast", p(&input), "--data", p(&data), "--min-lon", "20", "--max-lon", "10",
    ])
    .unwrap_err();
    assert!(err.contains("antimeridian"), "{err}");

    let cfg = dir.path().join("bad.toml");
    fs::write(&cfg, "region = [1, 2]\n").unwrap();
    assert!(run(&["coast", p(&input), "--data", p(&data), "-c", p(&cfg)]).is_err());
    let err = run(&["coast", p(&input), "--data", p(&data), "-c", p(&dir.path().join("none.toml"))])
        .unwrap_err();
    assert!(err.contains("none.toml"), "{err}");
}

#[test]
fn an_existing_column_needs_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());
    let data = gshhg_dir(dir.path());
    let once = dir.path().join("once.parquet");
    let twice = dir.path().join("twice.parquet");

    run(&["coast", p(&input), "--data", p(&data), "-o", p(&once)]).unwrap();
    let err = run(&["coast", p(&once), "--data", p(&data), "-o", p(&twice)]).unwrap_err();
    assert!(err.contains("dist_to_coast"), "{err}");
    run(&["coast", p(&once), "--data", p(&data), "-o", p(&twice), "--overwrite"]).unwrap();
    assert_eq!(read(&twice).width(), read(&once).width(), "replaced, not appended");
}

// ------------------------------------------------------------------ sea

fn sea_shp(dir: &Path) -> PathBuf {
    let path = dir.join("seas.shp");
    write_boxes(&path, &["NAME"], &[(SEA, &["Test Sea"])]);
    path
}

fn sea_geojson(dir: &Path) -> PathBuf {
    let path = dir.join("seas.geojson");
    fs::write(
        &path,
        r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"NAME":"Test Sea"},
             "geometry":{"type":"Polygon",
               "coordinates":[[[12,55],[16,55],[16,57],[12,57],[12,55]]]}}
        ]}"#,
    )
    .unwrap();
    path
}

#[test]
fn sea_from_a_shapefile_and_from_geojson() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());

    for (data, extra) in [
        (sea_shp(dir.path()), &[][..]),
        (sea_geojson(dir.path()), &["--partition"][..]),
    ] {
        let out = dir.path().join("out.parquet");
        let mut args = vec!["sea", p(&input), "--data", p(&data), "-o", p(&out), "--overwrite"];
        args.extend_from_slice(extra);
        run(&args).unwrap();
        let names = strs(&read(&out), "sea_name");
        assert_eq!(names[2].as_deref(), Some("Test Sea"), "{}", data.display());
        assert_eq!(names[3].as_deref(), Some("Test Sea"));
        assert_eq!(names[4], None);
    }
}

#[test]
fn sea_refuses_bad_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());
    let shp = sea_shp(dir.path());

    let err = run(&["sea", p(&input)]).unwrap_err();
    assert!(err.contains("--data"), "{err}");
    let err = run(&["sea", p(&input), "--data", p(&shp), "--name-field", "NOPE"]).unwrap_err();
    assert!(err.contains("NOPE"), "{err}");
}

// ---------------------------------------------------------------- place

fn countries(dir: &Path) -> PathBuf {
    let path = dir.join("countries.shp");
    // The second feature has no name and must be skipped, not stamped.
    write_boxes(
        &path,
        &["NAME", "ISO_A3"],
        &[(LAND, &["Testland", "TST"]), ((30.0, 55.0, 31.0, 56.0), &["", "-99"])],
    );
    path
}

fn municipalities(dir: &Path) -> PathBuf {
    let path = dir.join("lau.shp");
    write_boxes(&path, &["LAU_NAME"], &[(MUNI, &["Munitown"])]);
    path
}

#[test]
fn place_with_and_without_municipalities() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());
    let c = countries(dir.path());
    let m = municipalities(dir.path());

    let out = dir.path().join("plain.parquet");
    run(&["place", p(&input), "--countries", p(&c), "-o", p(&out)]).unwrap();
    let df = read(&out);
    assert_eq!(strs(&df, "country")[0].as_deref(), Some("Testland"));
    assert_eq!(strs(&df, "country_code")[1].as_deref(), Some("TST"));
    assert!(df.column("municipality_dist").is_err(), "no set, no distance column");
    assert!(strs(&df, "municipality").iter().all(Option::is_none));

    for extra in [&[][..], &["--partition"][..]] {
        let out = dir.path().join("full.parquet");
        let mut args = vec![
            "place", p(&input), "--countries", p(&c), "--municipalities", p(&m),
            "--unit", "m", "-o", p(&out), "--overwrite",
        ];
        args.extend_from_slice(extra);
        run(&args).unwrap();
        let df = read(&out);
        assert_eq!(strs(&df, "municipality")[0].as_deref(), Some("Munitown"));
        assert_eq!(f64s(&df, "municipality_dist")[0], Some(0.0));
        // Nearest country offshore, by the fallback.
        assert_eq!(strs(&df, "country")[2].as_deref(), Some("Testland"));
    }

    // A cutoff of 1 km drops the municipality matched from 2 degrees away.
    let out = dir.path().join("cut.parquet");
    run(&[
        "place", p(&input), "--countries", p(&c), "--municipalities", p(&m),
        "--max-municipality-dist", "1", "-o", p(&out),
    ])
    .unwrap();
    let df = read(&out);
    assert_eq!(strs(&df, "municipality")[0].as_deref(), Some("Munitown"));
    assert_eq!(strs(&df, "municipality")[2], None);
}

#[test]
fn place_refuses_bad_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());

    let err = run(&["place", p(&input)]).unwrap_err();
    assert!(err.contains("--countries"), "{err}");

    // Shapefiles with no usable name field.
    let unnamed = dir.path().join("unnamed.shp");
    write_boxes(&unnamed, &["OTHER"], &[(LAND, &["x"])]);
    let err = run(&["place", p(&input), "--countries", p(&unnamed)]).unwrap_err();
    assert!(err.contains("no named country polygons"), "{err}");

    let c = countries(dir.path());
    let err = run(&[
        "place", p(&input), "--countries", p(&c), "--municipalities", p(&unnamed),
    ])
    .unwrap_err();
    assert!(err.contains("no named municipality polygons"), "{err}");

    let err = run(&["place", p(&input), "--countries", p(&dir.path().join("none.shp"))]).unwrap_err();
    assert!(err.contains("none.shp"), "{err}");
}

// -------------------------------------------------------------- nearest

#[test]
fn nearest_against_a_reference_table() {
    let dir = tempfile::tempdir().unwrap();
    let input = points(dir.path());
    let farms = dir.path().join("farms.tsv");
    // Numeric names, to exercise the cast to text; custom coordinate columns.
    fs::write(&farms, "farm_id\tx\ty\n101\t13.0\t56.1\n102\t10.0\t55.0\n").unwrap();
    let out = dir.path().join("out.csv");

    run(&[
        "nearest", p(&input), "--to", p(&farms), "--to-lon-col", "x", "--to-lat-col", "y",
        "--name-field", "farm_id", "--name-column", "farm", "--dist-column", "farm_km",
        "-o", p(&out),
    ])
    .unwrap();
    let df = read(&out);
    let names: Vec<Option<i64>> = df.column("farm").unwrap().i64().unwrap().into_iter().collect();
    // (11.5, 56.5) is about 100 km from farm 101 and 180 km from farm 102.
    assert_eq!(names[..4], [Some(102), Some(101), Some(101), Some(101)]);
    assert_close(
        f64s(&df, "farm_km")[2].unwrap(),
        haversine_m(13.0, 56.0, 13.0, 56.1) / 1000.0,
        0.01,
    );

    let err = run(&["nearest", p(&input), "--to", p(&farms)]).unwrap_err();
    assert!(err.contains("longitude") || err.contains("name"), "{err}");
}

// -------------------------------------------------------------- regions

#[test]
fn regions_lists_filters_and_writes() {
    let dir = tempfile::tempdir().unwrap();

    run(&["regions"]).unwrap();

    let out = dir.path().join("baltic.csv");
    run(&["regions", "--name", "BALTIC", "-q", "-o", p(&out)]).unwrap();
    let names = strs(&read(&out), "name");
    assert!(!names.is_empty());
    assert!(names.iter().all(|n| n.as_deref().unwrap().to_lowercase().contains("baltic")));

    let out = dir.path().join("file.parquet");
    run(&["regions", "--data", p(&sea_geojson(dir.path())), "-o", p(&out)]).unwrap();
    assert_eq!(strs(&read(&out), "name"), [Some("Test Sea".to_string())]);

    let err = run(&["regions", "--name", "no such sea anywhere"]).unwrap_err();
    assert!(err.contains("no such sea anywhere"), "{err}");
}
