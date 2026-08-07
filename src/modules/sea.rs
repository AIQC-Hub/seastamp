//! Sea / ocean name from IHO Sea Areas polygons (point in polygon).
//!
//! Algorithm (pure Rust):
//!   1. Read the IHO Sea Areas (Marine Regions GeoJSON or shapefile) and keep
//!      every feature whose bounding box intersects the region box expanded by
//!      a margin. Features are kept whole, never clipped, so containment stays
//!      exact.
//!   2. Index feature bounding boxes in an `rstar` R-tree; containment is an
//!      even-odd ray cast over the candidate features only.
//!   3. A point inside no feature (just inland, common in narrow fjords) falls
//!      back to the feature with the nearest boundary segment by planar
//!      (region-LAEA) distance.
//!
//! Point-in-polygon runs in plain lon/lat: sea boundaries are coarse relative
//! to the coordinate rounding, so the containment test needs no projection.
//! Features without the name property are skipped; an input whose features all
//! lack it is rejected so a wrong `--name-field` fails loudly.

use std::error::Error;
use std::path::Path;

use crate::cli::SeaArgs;
use crate::config::{resolve, BBox, Settings};
use crate::geo::vector::{PolygonIndex, Rings, CROP_MARGIN_DEG};
use crate::geo::Laea;
use crate::pipeline::{run_module, run_partitioned, Enricher, OutputKind, OutputSpec, Value};

pub struct SeaEnricher {
    index: PolygonIndex<String>,
    column: String,
}

impl SeaEnricher {
    /// Build the enricher from named features already in memory. Used by
    /// [`SeaEnricher::open`] and by tests, so the geometry can be exercised
    /// without a data file on disk.
    pub fn from_features(
        feats: Vec<(Rings, String)>,
        region: BBox,
        proj: Laea,
        column: String,
    ) -> Self {
        SeaEnricher {
            index: PolygonIndex::build(feats, region, CROP_MARGIN_DEG, proj),
            column,
        }
    }

    /// Wrap an index built elsewhere, which is how the `--partition` path and
    /// its tests get one per partition out of [`PolygonIndex::build_many`].
    pub fn from_index(index: PolygonIndex<String>, column: String) -> Self {
        SeaEnricher { index, column }
    }

    /// Build one enricher per `(crop box, projection)` from a single read of the
    /// data file, for `--partition`. See [`PolygonIndex::build_many`] for why the
    /// read is shared and the geometry is not.
    pub fn open_many(
        data: &Path,
        name_field: &str,
        regions: &[(BBox, Laea)],
        column: &str,
    ) -> Result<Vec<Self>, Box<dyn Error>> {
        let feats = read_features(data, name_field)?;
        let built: Vec<Self> = PolygonIndex::build_many(&feats, regions, CROP_MARGIN_DEG)
            .into_iter()
            .map(|index| Self::from_index(index, column.to_string()))
            .collect();

        // One line for the run, not one per partition: with dozens of them the
        // per-partition version would bury everything else. Only a partition
        // that kept nothing is worth mentioning, and only in aggregate.
        let empty = built.iter().filter(|e| e.index.is_empty()).count();
        if empty == built.len() {
            eprintln!(
                "[seastamp] warning: no sea polygons overlap any partition, so every row will be \
                 empty. Check --data against your points."
            );
        } else if empty > 0 {
            eprintln!(
                "[seastamp] warning: {empty} of {} partitions matched no sea polygon; rows in \
                 those areas will be empty.",
                built.len()
            );
        }
        Ok(built)
    }

    /// Open an IHO Sea Areas file (GeoJSON `.geojson` / `.json`, or a
    /// shapefile `.shp`) and index its named polygons cropped to `region`.
    pub fn open(
        data: &Path,
        name_field: &str,
        region: BBox,
        proj: Laea,
        column: String,
    ) -> Result<Self, Box<dyn Error>> {
        let feats = read_features(data, name_field)?;
        let enr = Self::from_features(feats, region, proj, column);
        if enr.index.is_empty() {
            eprintln!(
                "[seastamp] warning: no sea polygons overlap the region, so every row will be \
                 empty. Check --region against your data."
            );
        }
        Ok(enr)
    }
}

impl Enricher for SeaEnricher {
    /// Containment needs no projection, but the nearest-boundary fallback for
    /// points inside no polygon is planar, so a far-away center can pick the
    /// wrong feature.
    fn projection_center(&self) -> Option<(f64, f64)> {
        Some(self.index.center())
    }

    fn outputs(&self) -> Vec<OutputSpec> {
        Vec::from([OutputSpec {
            name: self.column.clone(),
            kind: OutputKind::Text,
        }])
    }

    fn enrich(&self, lon: f64, lat: f64) -> Vec<Value> {
        Vec::from([Value::Text(self.index.locate(lon, lat).cloned())])
    }
}

/// Read every named polygon from an IHO Sea Areas file, GeoJSON or shapefile,
/// uncropped. Shared with the `regions` module, which needs the same features
/// to derive one bounding box per name.
pub(crate) fn read_features(
    data: &Path,
    name_field: &str,
) -> Result<Vec<(Rings, String)>, Box<dyn Error>> {
    let ext = data
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let feats = match ext.as_str() {
        "geojson" | "json" => geojson_features(data, name_field)?,
        "shp" => shp_features(data, name_field)?,
        _ => {
            return Err(format!(
                "sea data must be .geojson, .json, or .shp: {}",
                data.display()
            )
            .into())
        }
    };
    if feats.is_empty() {
        return Err(format!(
            "no polygon features with a '{name_field}' name in {}; check --name-field",
            data.display()
        )
        .into());
    }
    Ok(feats)
}

/// Read named polygon features from a Marine Regions style GeoJSON
/// FeatureCollection. MultiPolygons become one feature per part, all sharing
/// the name, which changes neither containment nor the nearest fallback.
fn geojson_features(path: &Path, name_field: &str) -> Result<Vec<(Rings, String)>, Box<dyn Error>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let gj: geojson::GeoJson = text
        .parse()
        .map_err(|e| format!("invalid GeoJSON {}: {e}", path.display()))?;
    let geojson::GeoJson::FeatureCollection(fc) = gj else {
        return Err(format!("{} is not a GeoJSON FeatureCollection", path.display()).into());
    };

    let mut feats = Vec::new();
    for f in fc.features {
        let name = f
            .properties
            .as_ref()
            .and_then(|m| m.get(name_field))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let (Some(name), Some(geom)) = (name, f.geometry) else {
            continue;
        };
        match geom.value {
            geojson::Value::Polygon(p) => feats.push((poly_rings(&p), name)),
            geojson::Value::MultiPolygon(mp) => {
                for p in &mp {
                    feats.push((poly_rings(p), name.clone()));
                }
            }
            _ => {}
        }
    }
    Ok(feats)
}

/// GeoJSON polygon coordinates to lon/lat ring tuples.
fn poly_rings(p: &[Vec<Vec<f64>>]) -> Rings {
    p.iter()
        .map(|ring| {
            ring.iter()
                .filter(|pos| pos.len() >= 2)
                .map(|pos| (pos[0], pos[1]))
                .collect()
        })
        .collect()
}

/// Read named polygon features from a shapefile's shapes and DBF records.
fn shp_features(path: &Path, name_field: &str) -> Result<Vec<(Rings, String)>, Box<dyn Error>> {
    let mut feats = Vec::new();
    for (rings, record) in super::shp_polygons(path)? {
        if let Some(shapefile::dbase::FieldValue::Character(Some(name))) = record.get(name_field) {
            let name = name.trim().to_string();
            if !name.is_empty() {
                feats.push((rings, name));
            }
        }
    }
    Ok(feats)
}

pub fn run(args: SeaArgs) -> Result<(), Box<dyn Error>> {
    let mut s: Settings = resolve(&args.common, Some(&args.region))?;
    let data = args
        .data
        .ok_or("sea requires --data <IHO Sea Areas GeoJSON or shapefile>")?;
    let df = crate::io::read_frame(&args.common.input, args.common.in_format)?;
    let out_path = args
        .common
        .output
        .clone()
        .unwrap_or_else(|| super::default_output(&args.common.input, "sea", args.common.in_format));

    // --partition derives a region per piece of the input, so there is no single
    // region to settle and `apply_auto_region` has nothing to say. The pipeline
    // splits the locations and calls back here once per batch of partitions,
    // each call reading the data file once for the whole batch.
    if s.partition {
        let name_field = args.name_field.clone();
        let column = args.column.clone();
        let build = move |regions: &[(BBox, Laea)]| {
            let built = SeaEnricher::open_many(&data, &name_field, regions, &column)?;
            Ok(built
                .into_iter()
                .map(|e| Box::new(e) as Box<dyn Enricher>)
                .collect())
        };
        let outputs = [OutputSpec {
            name: args.column.clone(),
            kind: OutputKind::Text,
        }];
        return run_partitioned(&build, &outputs, df, &s, &out_path, args.common.out_format);
    }

    // --region auto needs the points, so the region settles here, after the
    // table is read and before any reference data is cropped to it.
    let pts = crate::pipeline::locations(&df, &s)?;
    crate::config::apply_auto_region(&mut s, &pts)?;

    let proj = Laea::new(s.proj_lon0, s.proj_lat0);
    let enr = SeaEnricher::open(&data, &args.name_field, s.bbox, proj, args.column)?;
    run_module(&enr, df, &s, &out_path, args.common.out_format)
}
