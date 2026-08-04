//! The `regions` listing against a small synthetic IHO-style GeoJSON: one box
//! per name, MultiPolygon parts merged, the antimeridian case, the circumpolar
//! case, and the written table.

use seastamp::cli::Format;
use seastamp::modules::regions::{areas_from_file, to_frame, Area};

/// A GeoJSON polygon ring for an axis-aligned box.
fn ring(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> String {
    format!(
        "[[[{min_lon},{min_lat}],[{max_lon},{min_lat}],[{max_lon},{max_lat}],\
         [{min_lon},{max_lat}],[{min_lon},{min_lat}]]]"
    )
}

/// Write a FeatureCollection holding the four cases and return its path.
fn fixture(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("seas.geojson");
    let plain = ring(12.0, 40.0, 20.0, 46.0);
    // Split at the antimeridian the way Marine Regions distributes it: one part
    // each side of 180, sharing a name.
    let east = ring(120.0, -10.0, 180.0, 60.0);
    let west = ring(-180.0, -10.0, -100.0, 60.0);
    // Circumpolar: four contiguous parts with no vertices in between, so only
    // an edge-based extent sees them as covering the globe.
    let ring_parts = [
        ring(-180.0, -70.0, -90.0, -60.0),
        ring(-90.0, -70.0, 0.0, -60.0),
        ring(0.0, -70.0, 90.0, -60.0),
        ring(90.0, -70.0, 180.0, -60.0),
    ]
    .join(",");
    std::fs::write(
        &path,
        format!(
            r#"{{"type":"FeatureCollection","features":[
                {{"type":"Feature","properties":{{"NAME":"Adriatic Sea"}},
                 "geometry":{{"type":"Polygon","coordinates":{plain}}}}},
                {{"type":"Feature","properties":{{"NAME":"Pacific Ocean"}},
                 "geometry":{{"type":"MultiPolygon","coordinates":[{east},{west}]}}}},
                {{"type":"Feature","properties":{{"NAME":"Southern Ocean"}},
                 "geometry":{{"type":"MultiPolygon","coordinates":[{ring_parts}]}}}}
            ]}}"#
        ),
    )
    .unwrap();
    path
}

fn find<'a>(areas: &'a [Area], name: &str) -> &'a Area {
    areas
        .iter()
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("no area named '{name}' in {areas:?}"))
}

#[test]
fn one_box_per_name_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let areas = areas_from_file(&fixture(dir.path()), "NAME").unwrap();

    assert_eq!(areas.len(), 3, "one row per name, not per polygon part");
    let names: Vec<&str> = areas.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Adriatic Sea", "Pacific Ocean", "Southern Ocean"]);
}

#[test]
fn plain_box_keeps_its_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let areas = areas_from_file(&fixture(dir.path()), "NAME").unwrap();

    let a = find(&areas, "Adriatic Sea");
    assert_eq!((a.min_lon, a.max_lon), (12.0, 20.0));
    assert_eq!((a.min_lat, a.max_lat), (40.0, 46.0));
    assert!(!a.crosses_antimeridian);
}

/// The whole point of the edge-based extent: a feature split at 180 has
/// vertices at both -180 and 180, and a naive min/max would call it global.
#[test]
fn antimeridian_area_reports_the_short_arc() {
    let dir = tempfile::tempdir().unwrap();
    let areas = areas_from_file(&fixture(dir.path()), "NAME").unwrap();

    let a = find(&areas, "Pacific Ocean");
    assert!(a.crosses_antimeridian);
    assert_eq!((a.min_lon, a.max_lon), (120.0, -100.0));
    assert!(a.min_lon > a.max_lon, "the flag and the ordering agree");
    assert_eq!((a.min_lat, a.max_lat), (-10.0, 60.0));
}

/// Parts that tile the globe with no intermediate vertices still read as
/// circumpolar, which vertex-only gap detection would get wrong.
#[test]
fn circumpolar_area_spans_every_longitude() {
    let dir = tempfile::tempdir().unwrap();
    let areas = areas_from_file(&fixture(dir.path()), "NAME").unwrap();

    let a = find(&areas, "Southern Ocean");
    assert_eq!((a.min_lon, a.max_lon), (-180.0, 180.0));
    assert!(!a.crosses_antimeridian);
    assert_eq!((a.min_lat, a.max_lat), (-70.0, -60.0));
}

#[test]
fn written_table_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let areas = areas_from_file(&fixture(dir.path()), "NAME").unwrap();
    let out = dir.path().join("regions.csv");
    seastamp::io::write_frame(to_frame(&areas).unwrap(), &out, Format::Auto).unwrap();

    let df = seastamp::io::read_frame(&out, Format::Auto).unwrap();
    assert_eq!(df.height(), 3);
    assert_eq!(
        df.get_column_names()
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
        ["name", "min_lon", "max_lon", "min_lat", "max_lat", "crosses_antimeridian"]
    );
    let crossing = df.column("crosses_antimeridian").unwrap().bool().unwrap();
    assert_eq!(crossing.get(1), Some(true), "the Pacific row is flagged");
}

#[test]
fn wrong_name_field_fails_loudly() {
    let dir = tempfile::tempdir().unwrap();
    let err = areas_from_file(&fixture(dir.path()), "NOPE")
        .expect_err("a missing name field must be an error");
    assert!(err.to_string().contains("NOPE"), "unexpected error: {err}");
}
