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
use crate::config::{preset_bbox, IHO_AREAS, PRESET_NAMES};
use crate::geo::arc::{covering_arc, push_interval};

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
    /// Where the box came from: a built-in `preset`, the baked-in `iho` table,
    /// or `data` when it was derived from a file passed with `--data`.
    pub source: String,
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

    /// Record the longitude span of one edge.
    fn add_edge(&mut self, lon_a: f64, lon_b: f64) {
        push_interval(&mut self.intervals, lon_a, lon_b);
    }
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
            let (min_lon, max_lon, crosses) = covering_arc(&mut a.intervals)?;
            Some(Area {
                name: name.to_string(),
                min_lon,
                max_lon,
                min_lat: a.min_lat,
                max_lat: a.max_lat,
                crosses_antimeridian: crosses,
                source: "data".to_string(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Every name `--region` accepts: the built-in presets first, then the baked-in
/// IHO Sea Areas table. Listing both is the point, since the two are
/// interchangeable at the `--region` flag and neither needs a data file.
fn builtin_areas() -> Vec<Area> {
    let presets = PRESET_NAMES.iter().filter_map(|name| {
        let b = preset_bbox(name)?;
        Some(Area {
            name: name.to_string(),
            min_lon: b.min_lon,
            max_lon: b.max_lon,
            min_lat: b.min_lat,
            max_lat: b.max_lat,
            crosses_antimeridian: false,
            source: "preset".to_string(),
        })
    });
    let iho = IHO_AREAS.iter().map(|a| Area {
        name: a.name.to_string(),
        min_lon: a.bbox.min_lon,
        max_lon: a.bbox.max_lon,
        min_lat: a.bbox.min_lat,
        max_lat: a.bbox.max_lat,
        crosses_antimeridian: a.crosses,
        source: "iho".to_string(),
    });
    presets.chain(iho).collect()
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
        "source" => areas.iter().map(|a| a.source.clone()).collect::<Vec<_>>(),
    }?;
    Ok(df)
}

/// Print the list as an aligned table on stdout. The table is the command's
/// product, not progress reporting, so it goes to stdout and stays pipeable.
fn print_table(areas: &[Area], builtin: bool) {
    let w = areas
        .iter()
        .map(|a| a.name.chars().count())
        .chain(std::iter::once(4)) // the "name" header itself
        .max()
        .unwrap_or(4);

    println!(
        "{:<w$}  {:>8}  {:>8}  {:>8}  {:>8}  {:<6}  antimeridian",
        "name", "min_lon", "max_lon", "min_lat", "max_lat", "source"
    );
    for a in areas {
        println!(
            "{:<w$}  {:>8.2}  {:>8.2}  {:>8.2}  {:>8.2}  {:<6}  {}",
            a.name,
            a.min_lon,
            a.max_lon,
            a.min_lat,
            a.max_lat,
            a.source,
            if a.crosses_antimeridian { "yes" } else { "" }
        );
    }

    let crossing = areas.iter().filter(|a| a.crosses_antimeridian).count();
    eprintln!(
        "[seastamp] {} area{}",
        areas.len(),
        if areas.len() == 1 { "" } else { "s" }
    );
    if builtin {
        eprintln!(
            "[seastamp] every name here works as --region <NAME>, no data file needed. \
             --region auto derives the region from your points instead."
        );
    } else {
        eprintln!(
            "[seastamp] derived from --data. The built-in list (run without --data) is what \
             --region accepts by name; use --min-lon / --max-lon / --min-lat / --max-lat for \
             anything else."
        );
    }
    if crossing > 0 {
        eprintln!(
            "[seastamp] {crossing} of them {} the antimeridian, so min_lon is greater than \
             max_lon there. --region rejects those by name: use --region auto, or split the run \
             into an eastern and a western half.",
            if crossing == 1 { "crosses" } else { "cross" }
        );
    }
}

pub fn run(args: RegionsArgs) -> Result<(), Box<dyn Error>> {
    let mut areas = match &args.data {
        Some(p) => areas_from_file(p, &args.name_field)?,
        None => builtin_areas(),
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

