//! Distance to the nearest coast, from GSHHG shoreline polygons.
//!
//! Algorithm (pure Rust, no PROJ):
//!   1. Read the GSHHG L1 shoreline shapefile (resolution `f` recommended).
//!   2. Project every boundary segment through the region LAEA into planar
//!      meters and index the segments in an `rstar` R-tree.
//!   3. Per location: project the point and take the minimum planar distance to
//!      the nearest segment (Snyder LAEA meters), converted to the chosen unit.
//!
//! Distances are therefore planar in that projection, which is accurate at
//! regional scale and degrades with distance from the projection center.
//!
//! Cropping: only segments whose lon/lat bounding box intersects the region box
//! (expanded by a margin) are indexed. Whole polygons are never clipped, so no
//! artificial shoreline is introduced; distant segments are simply dropped,
//! which cannot create false coast. Points whose true nearest coast lies beyond
//! the region-plus-margin box therefore get an over-estimate; widen the region
//! for such cases.
//!
//! The whole L1 shapefile is parsed to filter it (there is no random-access skip
//! here yet); for very large inputs that one-time read dominates. Reads use an
//! immutable R-tree, so locations enrich fully in parallel with no locking.

use std::error::Error;
use std::path::{Path, PathBuf};

use rstar::{PointDistance, RTree, RTreeObject, AABB};

use crate::cli::{CoastArgs, DistUnit};
use crate::config::{resolve, BBox, Settings};
use crate::geo::vector::{expand, point_seg_dist2, CROP_MARGIN_DEG};
use crate::geo::Laea;
use crate::pipeline::{run_module, run_partitioned, Enricher, OutputKind, OutputSpec, Value};

/// One shoreline segment in projected (LAEA) meters.
struct Segment {
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
}

impl RTreeObject for Segment {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(
            [self.ax.min(self.bx), self.ay.min(self.by)],
            [self.ax.max(self.bx), self.ay.max(self.by)],
        )
    }
}

impl PointDistance for Segment {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        point_seg_dist2(point[0], point[1], self.ax, self.ay, self.bx, self.by)
    }
}

/// Whether the lon/lat bounding box of segment `a`-`b` intersects `crop`.
fn seg_intersects(alon: f64, alat: f64, blon: f64, blat: f64, crop: &BBox) -> bool {
    let (min_lon, max_lon) = (alon.min(blon), alon.max(blon));
    let (min_lat, max_lat) = (alat.min(blat), alat.max(blat));
    max_lon >= crop.min_lon
        && min_lon <= crop.max_lon
        && max_lat >= crop.min_lat
        && min_lat <= crop.max_lat
}

/// Crop, project, and push one boundary segment if it survives the crop.
fn add_segment(
    segs: &mut Vec<Segment>,
    alon: f64,
    alat: f64,
    blon: f64,
    blat: f64,
    crop: &BBox,
    proj: &Laea,
) {
    if !seg_intersects(alon, alat, blon, blat, crop) {
        return;
    }
    let (ax, ay) = proj.forward(alon, alat);
    let (bx, by) = proj.forward(blon, blat);
    segs.push(Segment { ax, ay, bx, by });
}

pub struct CoastEnricher {
    tree: RTree<Segment>,
    proj: Laea,
    to_km: bool,
    column: String,
}

impl CoastEnricher {
    /// Build the enricher from shoreline rings already in memory (lon/lat vertex
    /// lists). Used by [`CoastEnricher::open`] and by tests, so the geometry can
    /// be exercised without a shapefile on disk.
    pub fn from_rings<I>(
        rings: I,
        region: BBox,
        proj: Laea,
        unit: DistUnit,
        column: String,
    ) -> Self
    where
        I: IntoIterator<Item = Vec<(f64, f64)>>,
    {
        let crop = expand(&region, CROP_MARGIN_DEG);
        let mut segs = Vec::new();
        for ring in rings {
            for w in ring.windows(2) {
                add_segment(&mut segs, w[0].0, w[0].1, w[1].0, w[1].1, &crop, &proj);
            }
        }
        CoastEnricher {
            tree: RTree::bulk_load(segs),
            proj,
            to_km: matches!(unit, DistUnit::Km),
            column,
        }
    }

    /// Open a GSHHG shoreline shapefile (or a resolution directory holding one)
    /// and index its boundary segments cropped to `region`.
    pub fn open(
        data: &Path,
        region: BBox,
        proj: Laea,
        unit: DistUnit,
        column: String,
    ) -> Result<Self, Box<dyn Error>> {
        let mut built = Self::open_many(data, &[(region, proj)], unit, &column)?;
        Ok(built.remove(0))
    }

    /// Build one enricher per `(crop box, projection)` from a single pass over
    /// the shapefile, for `--partition`.
    ///
    /// The pass is shared on purpose. Parsing the 154 MB `f` shoreline dominates
    /// a `coast` run, so building each partition separately would multiply the
    /// dominant cost by the partition count. Here every segment is tested
    /// against every crop as it streams past and pushed, already projected, into
    /// whichever partitions want it. A segment near a partition boundary lands
    /// in both, which is what keeps each partition's coastline complete: the
    /// crops overlap by design, and `--partition` batches its calls here so the
    /// duplication stays within a memory budget.
    pub fn open_many(
        data: &Path,
        regions: &[(BBox, Laea)],
        unit: DistUnit,
        column: &str,
    ) -> Result<Vec<Self>, Box<dyn Error>> {
        let shp = resolve_shapefile(data)?;
        let mut reader = shapefile::Reader::from_path(&shp)
            .map_err(|e| format!("cannot read shapefile {}: {e}", shp.display()))?;

        let crops: Vec<BBox> = regions
            .iter()
            .map(|(r, _)| expand(r, CROP_MARGIN_DEG))
            .collect();
        let mut segs: Vec<Vec<Segment>> = regions.iter().map(|_| Vec::new()).collect();
        for item in reader.iter_shapes_and_records() {
            let (shape, _record) = item.map_err(|e| format!("reading {}: {e}", shp.display()))?;
            if let shapefile::Shape::Polygon(poly) = shape {
                for ring in poly.rings() {
                    for w in ring.points().windows(2) {
                        for (i, crop) in crops.iter().enumerate() {
                            let proj = &regions[i].1;
                            add_segment(
                                &mut segs[i], w[0].x, w[0].y, w[1].x, w[1].y, crop, proj,
                            );
                        }
                    }
                }
            }
        }

        // With one region this is the whole run; with many it is a handful of
        // partitions in open water, and saying "every distance will be null"
        // would be false. Counting them is also the only warning a caller gets
        // that a tighter crop has cost them values a global crop would have
        // found, however distorted those were.
        let empty = segs.iter().filter(|s| s.is_empty()).count();
        if empty == segs.len() {
            eprintln!(
                "[seastamp] warning: no shoreline segments overlap the region, so every distance \
                 will be null. Check --region and --data against your points."
            );
        } else if empty > 0 {
            eprintln!(
                "[seastamp] warning: {empty} partition(s) have no shoreline within reach, so \
                 dist_to_coast is null for points there. GSHHG L1 has no Antarctic coastline \
                 (it stops at 69S), which is the usual cause."
            );
        }
        Ok(segs
            .into_iter()
            .zip(regions)
            .map(|(s, &(_, proj))| CoastEnricher {
                tree: RTree::bulk_load(s),
                proj,
                to_km: matches!(unit, DistUnit::Km),
                column: column.to_string(),
            })
            .collect())
    }
}

impl Enricher for CoastEnricher {
    /// Distances here are planar in the region LAEA, so the pipeline warns when
    /// the input sits far from its center.
    fn projection_center(&self) -> Option<(f64, f64)> {
        Some(self.proj.center())
    }

    fn outputs(&self) -> Vec<OutputSpec> {
        Vec::from([OutputSpec {
            name: self.column.clone(),
            kind: OutputKind::Float,
        }])
    }

    fn enrich(&self, lon: f64, lat: f64) -> Vec<Value> {
        let (x, y) = self.proj.forward(lon, lat);
        let p = [x, y];
        let d = match self.tree.nearest_neighbor(p) {
            Some(seg) => {
                let meters = seg.distance_2(&p).sqrt();
                if self.to_km {
                    meters / 1000.0
                } else {
                    meters
                }
            }
            None => f64::NAN, // no coastline in range
        };
        Vec::from([Value::Float(d)])
    }
}

/// Resolve `--data` to an L1 shoreline `.shp`: a file is used as-is; a directory
/// is searched for a `GSHHS_*_L1.shp` (the land/ocean boundary).
fn resolve_shapefile(data: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if data.is_file() {
        return Ok(data.to_path_buf());
    }
    if data.is_dir() {
        for entry in std::fs::read_dir(data)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("GSHHS_") && name.ends_with("_L1.shp") {
                return Ok(entry.path());
            }
        }
        return Err(format!(
            "no GSHHS_*_L1.shp shoreline file found in {}",
            data.display()
        )
        .into());
    }
    Err(format!("coast data path not found: {}", data.display()).into())
}

pub fn run(args: CoastArgs) -> Result<(), Box<dyn Error>> {
    let mut s: Settings = resolve(&args.common, Some(&args.region))?;
    let data = args.data.ok_or(
        "coast requires --data <GSHHG shapefile directory or a GSHHS_*_L1.shp file>",
    )?;
    let df = crate::io::read_frame(&args.common.input, args.common.in_format)?;
    let out_path = args
        .common
        .output
        .clone()
        .unwrap_or_else(|| super::default_output(&args.common.input, "coast", args.common.in_format));

    // --partition derives a region per piece of the input, so there is no single
    // region to settle and `apply_auto_region` has nothing to say. The pipeline
    // splits the locations and calls back here once per batch of partitions,
    // each call reading the shapefile once for the whole batch.
    if s.partition {
        let unit = args.unit;
        let column = args.column.clone();
        let build = move |regions: &[(BBox, Laea)]| {
            let built = CoastEnricher::open_many(&data, regions, unit, &column)?;
            Ok(built
                .into_iter()
                .map(|e| Box::new(e) as Box<dyn Enricher>)
                .collect())
        };
        let outputs = [OutputSpec {
            name: args.column.clone(),
            kind: OutputKind::Float,
        }];
        return run_partitioned(&build, &outputs, df, &s, &out_path, args.common.out_format);
    }

    // --region auto needs the points, so the region settles here, after the
    // table is read and before any reference data is cropped to it.
    let pts = crate::pipeline::locations(&df, &s)?;
    crate::config::apply_auto_region(&mut s, &pts)?;

    let proj = Laea::new(s.proj_lon0, s.proj_lat0);
    let enr = CoastEnricher::open(&data, s.bbox, proj, args.unit, args.column)?;
    run_module(&enr, df, &s, &out_path, args.common.out_format)
}
