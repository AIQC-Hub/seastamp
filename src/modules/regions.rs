//! Sea and ocean bounding boxes: the built-in region presets, or one box per
//! named area in an IHO Sea Areas file.
//!
//! Unlike the five enrichment modules this one takes no table of points and
//! runs no pipeline. It answers "which seas exist, and where are they", so a
//! user whose data sits outside the presets (which are mostly European) can
//! find a starting box for their own region.
//!
//! The latitude extent is the plain minimum and maximum of the vertices. The
//! longitude extent is not: it is the smallest arc of the globe that covers
//! every polygon edge, computed as the complement of the widest uncovered gap.
//! Marine Regions splits its polygons at the antimeridian, so a Pacific area
//! has vertices at both -180 and 180, and a naive min/max would report the
//! whole globe for it. Working from edges rather than vertices also keeps a
//! long edge with no intermediate vertex from reading as a gap.
//!
//! An area whose smallest arc crosses the antimeridian is reported with
//! `min_lon` greater than `max_lon` and `crosses_antimeridian` true. Such a box
//! cannot be handed back to `--min-lon` / `--max-lon`, which `config::resolve`
//! rejects, so the flag marks exactly the areas that have to be run as an
//! eastern and a western box.
//!
//! Nothing here is specific to IHO beyond the default `--name-field`: any
//! polygon layer with a name attribute lists the same way.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

use polars::prelude::*;

use crate::cli::RegionsArgs;
use crate::config::{preset_bbox, PRESET_NAMES};

/// A gap this narrow counts as no gap at all: the area wraps the globe, and its
/// longitude extent is the whole range rather than an arc that happens to stop
/// a fraction of a degree short of where it started.
const CIRCUMPOLAR_GAP_DEG: f64 = 1.0;

/// One named area's bounding box in degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    pub name: String,
    pub min_lon: f64,
    pub max_lon: f64,
    pub min_lat: f64,
    pub max_lat: f64,
    /// True when the box runs east past 180 and continues from -180, which is
    /// also why `min_lon` is then greater than `max_lon`.
    pub crosses_antimeridian: bool,
}

/// Longitude normalized to `0..360`, the frame the arc arithmetic works in.
fn norm360(lon: f64) -> f64 {
    let x = lon % 360.0;
    if x < 0.0 {
        x + 360.0
    } else {
        x
    }
}

/// Back from `0..360` to the `-180..180` the rest of seastamp uses.
fn to180(lon360: f64) -> f64 {
    if lon360 > 180.0 {
        lon360 - 360.0
    } else {
        lon360
    }
}

/// The longitude interval one polygon edge covers, as `(start, length)` in the
/// `0..360` frame. An edge takes the shorter way around: a polygon edge
/// spanning more than half the globe would be a data error, and reading it the
/// long way would swallow the very gap we are looking for.
fn edge_interval(lon_a: f64, lon_b: f64) -> (f64, f64) {
    let (a, b) = (norm360(lon_a), norm360(lon_b));
    let east = norm360(b - a); // degrees travelled going east from a to b
    if east <= 180.0 {
        (a, east)
    } else {
        (b, 360.0 - east)
    }
}

/// What one name's polygon parts contribute, accumulated as they are read.
/// Deliberately not `Default`: the latitude bounds start at infinity, not zero.
struct Accum {
    /// Longitude intervals covered by the edges, in the `0..360` frame.
    intervals: Vec<(f64, f64)>,
    min_lat: f64,
    max_lat: f64,
}

impl Accum {
    fn new() -> Self {
        Accum {
            intervals: Vec::new(),
            min_lat: f64::INFINITY,
            max_lat: f64::NEG_INFINITY,
        }
    }

    fn add_lat(&mut self, lat: f64) {
        if lat.is_finite() {
            self.min_lat = self.min_lat.min(lat);
            self.max_lat = self.max_lat.max(lat);
        }
    }

    /// Record the longitude span of one edge, splitting it if it runs past the
    /// seam so the sweep can stay on a plain sorted list.
    fn add_edge(&mut self, lon_a: f64, lon_b: f64) {
        let (start, len) = edge_interval(lon_a, lon_b);
        if start + len > 360.0 {
            self.intervals.push((start, 360.0));
            self.intervals.push((0.0, start + len - 360.0));
        } else {
            self.intervals.push((start, start + len));
        }
    }
}

/// Smallest longitude arc covering every edge recorded in `intervals`, as
/// `(min_lon, max_lon, crosses_antimeridian)` in degrees.
///
/// Every edge contributes a covered interval; the arc is the complement of the
/// widest interval left uncovered. A fully covered circle (or one whose widest
/// gap is under [`CIRCUMPOLAR_GAP_DEG`]) reports the whole range.
fn lon_extent(intervals: &mut [(f64, f64)]) -> Option<(f64, f64, bool)> {
    if intervals.is_empty() {
        return None;
    }
    intervals.sort_by(|x, y| x.0.total_cmp(&y.0));

    // Sweep for the widest gap between the end of the covered run so far and
    // the start of the next interval.
    let (mut gap_from, mut gap_width) = (0.0_f64, -1.0_f64);
    let mut covered_to = intervals[0].1;
    for &(start, end) in &intervals[1..] {
        if start > covered_to {
            let w = start - covered_to;
            if w > gap_width {
                (gap_from, gap_width) = (covered_to, w);
            }
        }
        covered_to = covered_to.max(end);
    }
    // The gap that wraps the seam, from the end of the last interval round to
    // the start of the first.
    let wrap = intervals[0].0 + 360.0 - covered_to;
    if wrap > gap_width {
        (gap_from, gap_width) = (covered_to, wrap);
    }

    if gap_width < CIRCUMPOLAR_GAP_DEG {
        return Some((-180.0, 180.0, false));
    }
    // The arc is everything the gap is not: it starts where the gap ends and
    // runs east to where the gap starts.
    let west = to180(norm360(gap_from + gap_width));
    let east = to180(norm360(gap_from));
    Some((west, east, west > east))
}

/// Collapse named polygon parts into one bounding box per name. A MultiPolygon
/// arrives as several parts sharing a name (see `sea::read_features`), and its
/// parts are exactly what has to be merged for the extent to be right.
pub fn areas_from_features(feats: &[(crate::geo::vector::Rings, String)]) -> Vec<Area> {
    let mut acc: HashMap<&str, Accum> = HashMap::new();

    for (rings, name) in feats {
        let entry = acc.entry(name.as_str()).or_insert_with(Accum::new);
        for ring in rings {
            for pair in ring.windows(2) {
                let ((lon_a, lat_a), (lon_b, _)) = (pair[0], pair[1]);
                if !lon_a.is_finite() || !lon_b.is_finite() {
                    continue;
                }
                entry.add_edge(lon_a, lon_b);
                entry.add_lat(lat_a);
            }
            // `windows(2)` never yields the last vertex as its own `a`, and an
            // unclosed ring would otherwise lose that vertex's latitude.
            if let Some(&(_, lat)) = ring.last() {
                entry.add_lat(lat);
            }
        }
    }

    let mut out: Vec<Area> = acc
        .into_iter()
        .filter_map(|(name, mut a)| {
            let (min_lon, max_lon, crosses) = lon_extent(&mut a.intervals)?;
            Some(Area {
                name: name.to_string(),
                min_lon,
                max_lon,
                min_lat: a.min_lat,
                max_lat: a.max_lat,
                crosses_antimeridian: crosses,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The built-in `--region` presets as areas, so both listings share a shape.
fn preset_areas() -> Vec<Area> {
    PRESET_NAMES
        .iter()
        .filter_map(|name| {
            let b = preset_bbox(name)?;
            Some(Area {
                name: name.to_string(),
                min_lon: b.min_lon,
                max_lon: b.max_lon,
                min_lat: b.min_lat,
                max_lat: b.max_lat,
                crosses_antimeridian: false,
            })
        })
        .collect()
}

/// Read an IHO Sea Areas file and reduce it to one box per name.
pub fn areas_from_file(data: &Path, name_field: &str) -> Result<Vec<Area>, Box<dyn Error>> {
    let feats = crate::modules::sea::read_features(data, name_field)?;
    Ok(areas_from_features(&feats))
}

/// The list as a table, for `--output`.
pub fn to_frame(areas: &[Area]) -> Result<DataFrame, Box<dyn Error>> {
    let df = df! {
        "name" => areas.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
        "min_lon" => areas.iter().map(|a| a.min_lon).collect::<Vec<_>>(),
        "max_lon" => areas.iter().map(|a| a.max_lon).collect::<Vec<_>>(),
        "min_lat" => areas.iter().map(|a| a.min_lat).collect::<Vec<_>>(),
        "max_lat" => areas.iter().map(|a| a.max_lat).collect::<Vec<_>>(),
        "crosses_antimeridian" => areas.iter().map(|a| a.crosses_antimeridian).collect::<Vec<_>>(),
    }?;
    Ok(df)
}

/// Print the list as an aligned table on stdout. The table is the command's
/// product, not progress reporting, so it goes to stdout and stays pipeable.
fn print_table(areas: &[Area], presets: bool) {
    const HEAD: [&str; 5] = ["min_lon", "max_lon", "min_lat", "max_lat", "antimeridian"];
    let w = areas
        .iter()
        .map(|a| a.name.chars().count())
        .chain(std::iter::once(4)) // the "name" header itself
        .max()
        .unwrap_or(4);

    println!(
        "{:<w$}  {:>8}  {:>8}  {:>8}  {:>8}  {}",
        "name", HEAD[0], HEAD[1], HEAD[2], HEAD[3], HEAD[4]
    );
    for a in areas {
        println!(
            "{:<w$}  {:>8.2}  {:>8.2}  {:>8.2}  {:>8.2}  {}",
            a.name,
            a.min_lon,
            a.max_lon,
            a.min_lat,
            a.max_lat,
            if a.crosses_antimeridian { "yes" } else { "" }
        );
    }

    let crossing = areas.iter().filter(|a| a.crosses_antimeridian).count();
    eprintln!(
        "[seastamp] {} area{}",
        areas.len(),
        if areas.len() == 1 { "" } else { "s" }
    );
    if presets {
        eprintln!("[seastamp] these are the names --region accepts. Pass --data <IHO Sea Areas> to list every sea and ocean instead.");
    } else {
        eprintln!("[seastamp] use a box with --min-lon / --max-lon / --min-lat / --max-lat, not --region, which only takes the built-in preset names.");
    }
    if crossing > 0 {
        eprintln!(
            "[seastamp] {crossing} of them {} the antimeridian, so min_lon is greater than \
             max_lon there. seastamp cannot take such a box: split the run into an eastern and a \
             western half.",
            if crossing == 1 { "crosses" } else { "cross" }
        );
    }
}

pub fn run(args: RegionsArgs) -> Result<(), Box<dyn Error>> {
    let mut areas = match &args.data {
        Some(p) => areas_from_file(p, &args.name_field)?,
        None => preset_areas(),
    };

    if let Some(filter) = &args.name {
        let needle = filter.to_lowercase();
        areas.retain(|a| a.name.to_lowercase().contains(&needle));
        if areas.is_empty() {
            return Err(format!("no area name contains '{filter}'").into());
        }
    }

    if !args.quiet {
        print_table(&areas, args.data.is_none());
    }
    if let Some(out) = &args.output {
        crate::io::write_frame(to_frame(&areas)?, out, args.out_format)?;
        eprintln!("[seastamp] {} areas -> {}", areas.len(), out.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An edge always covers the shorter way round, whichever way it is drawn.
    #[test]
    fn edge_takes_the_short_arc() {
        assert_eq!(edge_interval(10.0, 40.0), (10.0, 30.0));
        assert_eq!(edge_interval(40.0, 10.0), (10.0, 30.0));
        // across the seam: 170 E to 170 W is 20 degrees, not 340
        let (start, len) = edge_interval(170.0, -170.0);
        assert_eq!((start, len), (170.0, 20.0));
    }

    #[test]
    fn norm360_and_back() {
        assert_eq!(norm360(-180.0), 180.0);
        assert_eq!(norm360(-90.0), 270.0);
        assert_eq!(to180(270.0), -90.0);
        assert_eq!(to180(180.0), 180.0);
    }

    /// A plain box in one hemisphere keeps its own bounds.
    #[test]
    fn simple_box_extent() {
        let mut iv = vec![(10.0, 20.0), (20.0, 30.0)];
        assert_eq!(lon_extent(&mut iv), Some((10.0, 30.0, false)));
    }

    /// A gap narrower than the circumpolar threshold reports the whole range
    /// rather than an arc that stops just short of closing.
    #[test]
    fn near_full_circle_is_circumpolar() {
        let mut iv = vec![(0.0, 359.5)];
        assert_eq!(lon_extent(&mut iv), Some((-180.0, 180.0, false)));
    }
}
