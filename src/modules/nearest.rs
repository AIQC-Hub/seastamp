//! Nearest location from a second set of points, with its distance.
//!
//! Unlike the other modules, the reference data is not a bundled dataset but a
//! second table the caller supplies (`--to`): any set of named locations (fish
//! farms, ports, stations, ...). For every input point this appends the name of
//! the nearest reference location and the great-circle distance to it.
//!
//! Algorithm (pure Rust, no PROJ):
//!   1. Read the reference table, taking each row's lon/lat and a name column.
//!   2. Map every reference point to a unit-sphere `(x, y, z)` vector and index
//!      those in a 3D `rstar` R-tree.
//!   3. Per input point: project it the same way and take the R-tree's nearest
//!      neighbor. Euclidean chord distance on the unit sphere is monotone in the
//!      great-circle distance, so the nearest by chord is the nearest on the
//!      globe; the squared chord is then converted to meters (`chord2_to_m`).
//!
//! The unit-sphere index is used instead of the region LAEA on purpose: the two
//! sets can be anywhere and arbitrarily far apart, and a single planar
//! projection distorts distances away from its center. The sphere is exact
//! everywhere, so this command takes no region box or projection center.
//!
//! Reference rows with a null or non-numeric coordinate are skipped. A reference
//! name that is null stays null in the output. If the reference table has no
//! usable location, every input row gets a null name and NaN distance.

use std::error::Error;

use polars::prelude::*;
use rstar::{PointDistance, RTree, RTreeObject, AABB};

use crate::cli::{DistUnit, NearestArgs};
use crate::config::{resolve, Settings};
use crate::geo::{chord2_to_m, unit_sphere};
use crate::pipeline::{run_module, Enricher, OutputKind, OutputSpec, Value};

/// One reference location as a unit-sphere point, tagged with its row index so
/// the name can be looked up after the nearest-neighbor query.
struct RefPoint {
    xyz: [f64; 3],
    tag: usize,
}

impl RTreeObject for RefPoint {
    type Envelope = AABB<[f64; 3]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.xyz)
    }
}

impl PointDistance for RefPoint {
    fn distance_2(&self, p: &[f64; 3]) -> f64 {
        let (dx, dy, dz) = (self.xyz[0] - p[0], self.xyz[1] - p[1], self.xyz[2] - p[2]);
        dx * dx + dy * dy + dz * dz
    }
}

pub struct NearestEnricher {
    tree: RTree<RefPoint>,
    names: Vec<Option<String>>,
    to_km: bool,
    name_column: String,
    dist_column: String,
}

impl NearestEnricher {
    /// Build the enricher from reference locations already in memory: each item
    /// is `(lon, lat, name)`. Rows with a non-finite coordinate are skipped.
    /// Used by [`NearestEnricher::open`] and by tests, so the geometry can be
    /// exercised without a file on disk.
    pub fn from_points<I>(points: I, unit: DistUnit, name_column: String, dist_column: String) -> Self
    where
        I: IntoIterator<Item = (f64, f64, Option<String>)>,
    {
        let mut refs = Vec::new();
        let mut names = Vec::new();
        for (lon, lat, name) in points {
            if !lon.is_finite() || !lat.is_finite() {
                continue;
            }
            refs.push(RefPoint { xyz: unit_sphere(lon, lat), tag: names.len() });
            names.push(name);
        }
        NearestEnricher {
            tree: RTree::bulk_load(refs),
            names,
            to_km: matches!(unit, DistUnit::Km),
            name_column,
            dist_column,
        }
    }

    /// Read the reference table and build the index. `lon_col`/`lat_col` are its
    /// coordinate columns and `name_field` the column holding each location's
    /// name (cast to text, so numeric ids work too).
    pub fn open(
        df: &DataFrame,
        lon_col: &str,
        lat_col: &str,
        name_field: &str,
        unit: DistUnit,
        name_column: String,
        dist_column: String,
    ) -> Result<Self, Box<dyn Error>> {
        let lon = column_f64(df, lon_col)?;
        let lat = column_f64(df, lat_col)?;
        let names = column_text(df, name_field)?;
        let points = (0..df.height()).map(|i| (lon[i], lat[i], names[i].clone()));
        let enr = Self::from_points(points, unit, name_column, dist_column);
        if enr.names.is_empty() {
            eprintln!(
                "[seastamp] warning: reference table '{}' has no usable location; \
                 nearest name/distance will be null",
                name_field
            );
        }
        Ok(enr)
    }
}

impl Enricher for NearestEnricher {
    fn outputs(&self) -> Vec<OutputSpec> {
        Vec::from([
            OutputSpec { name: self.name_column.clone(), kind: OutputKind::Text },
            OutputSpec { name: self.dist_column.clone(), kind: OutputKind::Float },
        ])
    }

    fn enrich(&self, lon: f64, lat: f64) -> Vec<Value> {
        let p = unit_sphere(lon, lat);
        match self.tree.nearest_neighbor(p) {
            Some(rp) => {
                let meters = chord2_to_m(rp.distance_2(&p));
                let dist = if self.to_km { meters / 1000.0 } else { meters };
                Vec::from([Value::Text(self.names[rp.tag].clone()), Value::Float(dist)])
            }
            // Empty reference set: no nearest location.
            None => Vec::from([Value::Text(None), Value::Float(f64::NAN)]),
        }
    }
}

/// Extract a column as `f64`, mapping nulls to NaN. Casts from any numeric dtype.
fn column_f64(df: &DataFrame, name: &str) -> Result<Vec<f64>, Box<dyn Error>> {
    let s = df
        .column(name)
        .map_err(|_| format!("reference table has no column '{name}'"))?;
    let ca = s.cast(&DataType::Float64)?;
    Ok(ca.f64()?.into_iter().map(|o| o.unwrap_or(f64::NAN)).collect())
}

/// Extract a column as optional text, casting numeric ids to their string form.
fn column_text(df: &DataFrame, name: &str) -> Result<Vec<Option<String>>, Box<dyn Error>> {
    let s = df
        .column(name)
        .map_err(|_| format!("reference table has no name column '{name}'"))?;
    let ca = s.cast(&DataType::String)?;
    Ok(ca.str()?.into_iter().map(|o| o.map(|v| v.to_string())).collect())
}

pub fn run(args: NearestArgs) -> Result<(), Box<dyn Error>> {
    // Nearest neighbor on the unit sphere needs no region box or projection.
    let s: Settings = resolve(&args.common, None)?;
    let df = crate::io::read_frame(&args.common.input, args.common.in_format)?;
    let reference = crate::io::read_frame(&args.to, args.to_format)?;
    let out_path = args
        .common
        .output
        .clone()
        .unwrap_or_else(|| super::default_output(&args.common.input, "nearest", args.common.in_format));

    let enr = NearestEnricher::open(
        &reference,
        &args.to_lon_col,
        &args.to_lat_col,
        &args.name_field,
        args.unit,
        args.name_column,
        args.dist_column,
    )?;
    run_module(&enr, df, &s, &out_path, args.common.out_format)
}
