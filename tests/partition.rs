//! `--partition` end to end for all three commands that take it, against
//! in-memory geometry rather than reference data on disk.
//!
//! The unit tests in `geo::partition` cover the split itself. What matters here
//! is the property the flag exists for: a point set too spread for one
//! projection gets measured accurately anyway, and the join back onto input rows
//! survives being computed in pieces.
//!
//! The reference distances come from running each area on its own with a
//! projection centered on it, which is the manual workaround `--partition`
//! automates. Agreement with that is the whole claim.

use polars::prelude::*;
use seastamp::cli::{DistUnit, Format};
use seastamp::config::{Settings, BBox, GLOBAL};
use seastamp::geo::partition::{partition, worst_distortion, DEFAULT_TOLERANCE};
use seastamp::geo::vector::{PolygonIndex, Rings};
use seastamp::geo::Laea;
use seastamp::modules::coast::CoastEnricher;
use seastamp::modules::place::PlaceEnricher;
use seastamp::modules::sea::SeaEnricher;
use seastamp::pipeline::{run_module, run_partitioned, Enricher, OutputKind, OutputSpec};

fn settings(partition: bool) -> Settings {
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
        partition,
    }
}

/// Four short north-south coastlines, one per quadrant of the globe, far enough
/// apart that no single LAEA projection can serve them all.
fn scattered_coasts() -> Vec<Vec<(f64, f64)>> {
    vec![
        vec![(20.0, 58.0), (20.0, 62.0)],     // northern Europe
        vec![(-60.0, -30.0), (-60.0, -26.0)], // South America
        vec![(150.0, -30.0), (150.0, -26.0)], // Australia
        vec![(-150.0, 20.0), (-150.0, 24.0)], // mid Pacific
    ]
}

/// One test point a little east of each coastline above.
fn points() -> Vec<(f64, f64)> {
    vec![(22.0, 60.0), (-58.0, -28.0), (152.0, -28.0), (-148.0, 22.0)]
}

fn frame(pts: &[(f64, f64)]) -> DataFrame {
    df! {
        "longitude" => pts.iter().map(|p| p.0).collect::<Vec<_>>(),
        "latitude"  => pts.iter().map(|p| p.1).collect::<Vec<_>>(),
    }
    .unwrap()
}

fn dists(out: &DataFrame) -> Vec<f64> {
    out.column("dist_to_coast")
        .unwrap()
        .f64()
        .unwrap()
        .into_iter()
        .map(|v| v.unwrap())
        .collect()
}

fn read_back(path: &std::path::Path) -> DataFrame {
    ParquetReader::new(std::fs::File::open(path).unwrap())
        .finish()
        .unwrap()
}

/// Run the four points together, each measured in its own partition.
fn run_partitioned_coast(pts: &[(f64, f64)]) -> DataFrame {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let build = |regions: &[(BBox, Laea)]| {
        Ok(regions
            .iter()
            .map(|&(bbox, proj)| {
                Box::new(CoastEnricher::from_rings(
                    scattered_coasts(),
                    bbox,
                    proj,
                    DistUnit::Km,
                    "dist_to_coast".into(),
                )) as Box<dyn Enricher>
            })
            .collect())
    };
    let outputs = [OutputSpec {
        name: "dist_to_coast".into(),
        kind: OutputKind::Float,
    }];
    run_partitioned(&build, &outputs, frame(pts), &settings(true), &out, Format::Parquet).unwrap();
    read_back(&out)
}

/// The reference: one point measured alone, in a projection centered on it.
fn run_alone(pt: (f64, f64)) -> f64 {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let proj = Laea::new(pt.0, pt.1);
    let bbox = BBox {
        min_lon: pt.0 - 10.0,
        max_lon: pt.0 + 10.0,
        min_lat: pt.1 - 10.0,
        max_lat: pt.1 + 10.0,
    };
    let enr =
        CoastEnricher::from_rings(scattered_coasts(), bbox, proj, DistUnit::Km, "dist_to_coast".into());
    run_module(&enr, frame(&[pt]), &settings(false), &out, Format::Parquet).unwrap();
    dists(&read_back(&out))[0]
}

/// The claim the feature makes: globally spread points come out as accurate as
/// if each had been run on its own, where a single projection is badly wrong.
#[test]
fn scattered_points_match_running_each_area_alone() {
    let pts = points();
    let got = dists(&run_partitioned_coast(&pts));

    for (&pt, &d) in pts.iter().zip(&got) {
        let want = run_alone(pt);
        let err = (d - want).abs() / want;
        assert!(
            err <= DEFAULT_TOLERANCE,
            "point {pt:?}: partitioned {d:.3} km vs {want:.3} km measured alone, {:.2}% out",
            err * 100.0
        );
    }
}

/// The same points through one projection, which is what `--partition` replaces.
/// Without this the test above could pass on a problem that was never hard.
#[test]
fn one_projection_really_is_worse() {
    let pts = points();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let enr = CoastEnricher::from_rings(
        scattered_coasts(),
        GLOBAL,
        Laea::new(0.0, 0.0),
        DistUnit::Km,
        "dist_to_coast".into(),
    );
    run_module(&enr, frame(&pts), &settings(false), &out, Format::Parquet).unwrap();
    let single = dists(&read_back(&out));

    let worst = pts
        .iter()
        .zip(&single)
        .map(|(&pt, &d)| {
            let want = run_alone(pt);
            (d - want).abs() / want
        })
        .fold(0.0_f64, f64::max);
    assert!(
        worst > DEFAULT_TOLERANCE,
        "a single global projection was only {:.2}% out, so this input does not exercise \
         --partition at all",
        worst * 100.0
    );
}

/// Data one projection already serves must come out of `--partition` unchanged,
/// not merely close: the flag has to be safe to leave on.
#[test]
fn a_local_run_is_untouched_by_partitioning() {
    let pts = [(22.0, 60.0), (21.5, 59.0), (23.0, 61.5)];
    let parts = partition(&pts, DEFAULT_TOLERANCE);
    assert_eq!(parts.len(), 1, "a local cluster must not be split");

    let got = dists(&run_partitioned_coast(&pts));
    let center = parts[0].center;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let enr = CoastEnricher::from_rings(
        scattered_coasts(),
        parts[0].bbox,
        Laea::new(center.0, center.1),
        DistUnit::Km,
        "dist_to_coast".into(),
    );
    run_module(&enr, frame(&pts), &settings(false), &out, Format::Parquet).unwrap();
    for (a, b) in got.iter().zip(&dists(&read_back(&out))) {
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }
}

/// Rows the pipeline cannot place still line up. A null coordinate must produce
/// a null result in the right row rather than shifting every later row's answer,
/// which is the way a scatter-gather join fails.
#[test]
fn rows_without_a_location_keep_their_place() {
    let df = df! {
        "longitude" => [22.0, f64::NAN, -58.0, 152.0],
        "latitude"  => [60.0, 60.0, -28.0, f64::NAN],
    }
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let build = |regions: &[(BBox, Laea)]| {
        Ok(regions
            .iter()
            .map(|&(bbox, proj)| {
                Box::new(CoastEnricher::from_rings(
                    scattered_coasts(),
                    bbox,
                    proj,
                    DistUnit::Km,
                    "dist_to_coast".into(),
                )) as Box<dyn Enricher>
            })
            .collect())
    };
    let outputs = [OutputSpec {
        name: "dist_to_coast".into(),
        kind: OutputKind::Float,
    }];
    run_partitioned(&build, &outputs, df, &settings(true), &out, Format::Parquet).unwrap();

    let col = dists(&read_back(&out));
    assert_eq!(col.len(), 4);
    assert!(col[0].is_finite(), "row 0 has a location: {}", col[0]);
    assert!(col[1].is_nan(), "row 1 has no longitude: {}", col[1]);
    assert!(col[2].is_finite(), "row 2 has a location: {}", col[2]);
    assert!(col[3].is_nan(), "row 3 has no latitude: {}", col[3]);
}

/// Four named sea polygons, one per quadrant, each a square a few degrees wide.
/// Far enough apart that no single projection serves them all.
fn scattered_seas() -> Vec<(Rings, String)> {
    let square = |lon: f64, lat: f64| -> Rings {
        vec![vec![
            (lon - 3.0, lat - 3.0),
            (lon + 3.0, lat - 3.0),
            (lon + 3.0, lat + 3.0),
            (lon - 3.0, lat + 3.0),
            (lon - 3.0, lat - 3.0),
        ]]
    };
    vec![
        (square(22.0, 60.0), "Northern Sea".to_string()),
        (square(-58.0, -28.0), "Southwestern Sea".to_string()),
        (square(152.0, -28.0), "Southeastern Sea".to_string()),
        (square(-148.0, 22.0), "Pacific Sea".to_string()),
    ]
}

/// `sea` names each point through its own partition. Containment is exact in
/// lon/lat and so cannot go wrong, but cropping is per partition, so this pins
/// that a partition still keeps the polygon its own points sit in.
#[test]
fn sea_names_every_point_through_its_own_partition() {
    let pts = points();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let build = |regions: &[(BBox, Laea)]| {
        Ok(
            PolygonIndex::build_many(&scattered_seas(), regions, 5.0)
                .into_iter()
                .map(|index| {
                    Box::new(SeaEnricher::from_index(index, "sea_name".into())) as Box<dyn Enricher>
                })
                .collect(),
        )
    };
    let outputs = [OutputSpec {
        name: "sea_name".into(),
        kind: OutputKind::Text,
    }];
    run_partitioned(&build, &outputs, frame(&pts), &settings(true), &out, Format::Parquet).unwrap();

    let got = read_back(&out);
    let names: Vec<Option<&str>> = got
        .column("sea_name")
        .unwrap()
        .str()
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(
        names,
        vec![
            Some("Northern Sea"),
            Some("Southwestern Sea"),
            Some("Southeastern Sea"),
            Some("Pacific Sea"),
        ],
        "each point must be named by the sea it is actually in"
    );
}

/// `place`'s `municipality_dist` is planar, so it is the column partitioning has
/// to improve. Same claim as for `coast`: as accurate as running each area alone.
#[test]
fn place_distances_match_running_each_area_alone() {
    let pts = points();
    let feats: Vec<(Rings, String)> = scattered_seas();
    let countries: Vec<(Rings, (String, Option<String>))> = feats
        .iter()
        .map(|(r, n)| (r.clone(), (n.clone(), None)))
        .collect();

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let build = |regions: &[(BBox, Laea)]| {
        let c = PolygonIndex::build_many(&countries, regions, 5.0);
        let m = PolygonIndex::build_many(&feats, regions, 5.0);
        Ok(c.into_iter()
            .zip(m)
            .map(|(c, m)| {
                Box::new(PlaceEnricher::from_indexes(c, Some(m), 1000.0, None)) as Box<dyn Enricher>
            })
            .collect())
    };
    let outputs = [
        OutputSpec { name: "country".into(), kind: OutputKind::Text },
        OutputSpec { name: "country_code".into(), kind: OutputKind::Text },
        OutputSpec { name: "municipality".into(), kind: OutputKind::Text },
        OutputSpec { name: "municipality_dist".into(), kind: OutputKind::Float },
    ];
    run_partitioned(&build, &outputs, frame(&pts), &settings(true), &out, Format::Parquet).unwrap();
    let got = read_back(&out);

    // Every point sits inside its own square, so the distance is 0 and the name
    // is that square's. A partition that cropped the wrong polygon away would
    // show up as a null or a non-zero distance to a distant square.
    let dist: Vec<f64> = got
        .column("municipality_dist")
        .unwrap()
        .f64()
        .unwrap()
        .into_iter()
        .map(|v| v.unwrap())
        .collect();
    for (pt, d) in pts.iter().zip(&dist) {
        assert_eq!(*d, 0.0, "point {pt:?} should be inside its own polygon, got {d}");
    }
    let names: Vec<Option<&str>> = got
        .column("municipality")
        .unwrap()
        .str()
        .unwrap()
        .into_iter()
        .collect();
    assert!(names.iter().all(|n| n.is_some()), "every point has a municipality: {names:?}");
}

/// `--partition` derives every box and center from the data, so pairing it with
/// a region or a bound is a contradiction rather than a precedence question.
/// Clap has to say so at parse time; silently ignoring one or the other is how
/// a run ends up measured somewhere the caller did not ask for.
#[test]
fn partition_refuses_to_share_a_command_line_with_a_region() {
    use clap::Parser;
    let parse = |extra: &[&str]| {
        let mut argv = vec!["seastamp", "coast", "in.parquet", "--partition"];
        argv.extend_from_slice(extra);
        seastamp::cli::Cli::try_parse_from(argv)
    };
    assert!(parse(&[]).is_ok(), "--partition alone must parse");
    for extra in [
        vec!["--region", "baltic"],
        vec!["--region", "auto"],
        vec!["--min-lon", "0"],
        vec!["--max-lat", "70"],
        vec!["--proj-lon0", "5"],
    ] {
        let err = parse(&extra).expect_err(&format!("{extra:?} must conflict with --partition"));
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::ArgumentConflict,
            "{extra:?} failed for the wrong reason: {err}"
        );
    }
}

/// Every partition must be inside the tolerance it was asked for, and the run
/// must report that honestly: the summary line is the accuracy claim.
#[test]
fn the_reported_distortion_bounds_every_partition() {
    let pts: Vec<(f64, f64)> = (-180..180)
        .step_by(20)
        .flat_map(|lo| (-60..70).step_by(20).map(move |la| (lo as f64, la as f64)))
        .collect();
    let parts = partition(&pts, DEFAULT_TOLERANCE);
    let worst = worst_distortion(&parts);
    assert!(worst <= DEFAULT_TOLERANCE, "reported {worst}");
    for p in &parts {
        assert!(p.distortion <= worst, "a partition beat the reported worst case");
    }
}

/// A crop's reach must be an under-estimate. The widening loop trusts it to
/// decide an answer is final, so an over-estimate would ship a distance that a
/// wider crop would have corrected, which is the whole failure being fixed.
#[test]
fn crop_reach_never_overstates() {
    use seastamp::geo::haversine_m;
    use seastamp::geo::vector::crop_reach_m;

    let crop = BBox { min_lon: -20.0, max_lon: 20.0, min_lat: 30.0, max_lat: 70.0 };
    for &(lon, lat) in &[(0.0, 50.0), (-19.0, 31.0), (19.0, 69.0), (5.0, 68.0), (-15.0, 45.0)] {
        let reach = crop_reach_m(&crop, lon, lat);
        // the true distance to each edge, sampled densely along it
        let mut nearest = f64::INFINITY;
        for i in 0..=400 {
            let t = i as f64 / 400.0;
            let x = crop.min_lon + t * (crop.max_lon - crop.min_lon);
            let y = crop.min_lat + t * (crop.max_lat - crop.min_lat);
            for &(elon, elat) in &[
                (x, crop.min_lat),
                (x, crop.max_lat),
                (crop.min_lon, y),
                (crop.max_lon, y),
            ] {
                nearest = nearest.min(haversine_m(lon, lat, elon, elat));
            }
        }
        assert!(
            reach <= nearest,
            "reach {reach:.0} m overstates the true {nearest:.0} m at ({lon}, {lat})"
        );
    }
    // a point outside the box is not covered by it at all
    assert_eq!(crop_reach_m(&crop, 100.0, 50.0), 0.0);
}

/// The fix itself: a point whose nearest coast lies well outside its own
/// partition's crop must still get the right answer, because the partition is
/// rebuilt wider rather than left to report whatever happened to be in range.
#[test]
fn a_partition_cropped_too_tightly_is_widened() {
    // One cluster of points with no coastline anywhere near it: the only
    // shoreline sits far to the east, well beyond the usual crop.
    let pts = [(0.0, 0.0), (1.0, 1.0), (-1.0, -1.0)];
    let far_coast: Vec<Vec<(f64, f64)>> = vec![vec![(40.0, -5.0), (40.0, 5.0)]];

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.parquet");
    let rings = far_coast.clone();
    let build = move |regions: &[(BBox, Laea)]| {
        Ok(regions
            .iter()
            .map(|&(bbox, proj)| {
                Box::new(CoastEnricher::from_rings(
                    rings.clone(),
                    bbox,
                    proj,
                    DistUnit::Km,
                    "dist_to_coast".into(),
                )) as Box<dyn Enricher>
            })
            .collect())
    };
    let outputs = [OutputSpec {
        name: "dist_to_coast".into(),
        kind: OutputKind::Float,
    }];
    run_partitioned(&build, &outputs, frame(&pts), &settings(true), &out, Format::Parquet).unwrap();
    let got = dists(&read_back(&out));

    // Against the same coastline with nothing cropped away at all.
    for (i, &pt) in pts.iter().enumerate() {
        let d2 = dir.path().join("ref.parquet");
        let enr = CoastEnricher::from_rings(
            far_coast.clone(),
            GLOBAL,
            Laea::new(pt.0, pt.1),
            DistUnit::Km,
            "dist_to_coast".into(),
        );
        run_module(&enr, frame(&[pt]), &settings(false), &d2, Format::Parquet).unwrap();
        let want = dists(&read_back(&d2))[0];
        assert!(
            got[i].is_finite(),
            "point {pt:?} got no distance at all; the crop was never widened"
        );
        let err = (got[i] - want).abs() / want;
        assert!(
            err <= DEFAULT_TOLERANCE,
            "point {pt:?}: {:.1} km against {want:.1} km uncropped, {:.1}% out",
            got[i],
            err * 100.0
        );
    }
}

/// Widening must not fire when the crop was already sufficient, or every run
/// pays for the rare case. A coast running through the points is comfortably
/// inside the first crop.
#[test]
fn a_sufficient_crop_is_not_widened() {
    use seastamp::pipeline::Enricher as _;

    let coast: Vec<Vec<(f64, f64)>> = vec![vec![(20.0, 58.0), (20.0, 62.0)]];
    let region = BBox { min_lon: 15.0, max_lon: 25.0, min_lat: 55.0, max_lat: 65.0 };
    let enr = CoastEnricher::from_rings(
        coast,
        region,
        Laea::new(20.0, 60.0),
        DistUnit::Km,
        "dist_to_coast".into(),
    );
    // a point a few tens of km from the coast, far inside the crop
    assert_eq!(
        enr.crop_shortfall(20.5, 60.0),
        0.0,
        "a coast well inside the crop must read as final"
    );
}
