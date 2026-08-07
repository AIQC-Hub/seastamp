//! Nearest country and municipality for an offshore point.
//!
//! Points at sea sit inside no land polygon, so both lookups resolve a point by
//! containment first (points on land) and nearest boundary otherwise, through
//! the shared [`PolygonIndex`]:
//!   - Country: Natural Earth country polygons. Appends `country` and
//!     `country_code` (ISO 3166-1 alpha-3 where Natural Earth provides one).
//!   - Municipality: GISCO LAU polygons. Appends `municipality`, and alongside it
//!     `municipality_dist` (0 inside the polygon, else the distance to it) when a
//!     set is given. The set is optional; without `--municipalities` the column
//!     stays empty and no distance column is added.
//!
//! The nearest-municipality match is unbounded by default, which matters because
//! GISCO LAU covers Europe only: a site outside that coverage still resolves to
//! whatever municipality is closest, however far. `municipality_dist` exposes
//! that, and `--max-municipality-dist` drops the match past a limit, clearing the
//! name and the distance together. Country does not need this, since Natural
//! Earth is global.
//!
//! Both polygon sets are cropped to the region box plus margin at load time, so
//! the large LAU file costs one parse and a small index; features are kept
//! whole, never clipped. Attribute fields are auto-detected per record from a
//! candidate list (Natural Earth name: NAME / ADMIN / NAME_EN / NAME_LONG, code:
//! ISO_A3 / ADM0_A3 / ISO_A3_EH with the "-99" placeholder treated as missing;
//! LAU name: LAU_NAME / LAU_LABEL / NAME), so minor schema drift between dataset
//! versions does not need flags.

use std::error::Error;
use std::path::Path;

use crate::cli::{DistUnit, PlaceArgs};
use crate::config::{resolve, BBox, Settings};
use crate::geo::vector::{PolygonIndex, Rings, CROP_MARGIN_DEG};
use crate::geo::Laea;
use crate::pipeline::{run_module, run_partitioned, Enricher, OutputKind, OutputSpec, Value};

/// Candidate DBF fields for the country name, tried in order per record.
const COUNTRY_NAME_FIELDS: &[&str] = &["NAME", "ADMIN", "NAME_EN", "NAME_LONG"];
/// Candidate DBF fields for the ISO alpha-3 code. Natural Earth stores "-99"
/// where a country has no agreed code; that placeholder is skipped.
const COUNTRY_CODE_FIELDS: &[&str] = &["ISO_A3", "ADM0_A3", "ISO_A3_EH"];
/// Candidate DBF fields for the municipality name (GISCO LAU).
const LAU_NAME_FIELDS: &[&str] = &["LAU_NAME", "LAU_LABEL", "NAME"];

/// First candidate field present in the record with a non-empty character
/// value, skipping the Natural Earth "-99" missing-code placeholder.
fn field_string(record: &shapefile::dbase::Record, candidates: &[&str]) -> Option<String> {
    for c in candidates {
        if let Some(shapefile::dbase::FieldValue::Character(Some(s))) = record.get(c) {
            let s = s.trim();
            if !s.is_empty() && s != "-99" {
                return Some(s.to_string());
            }
        }
    }
    None
}

pub struct PlaceEnricher {
    countries: PolygonIndex<(String, Option<String>)>,
    municipalities: Option<PolygonIndex<String>>,
    /// Metres per output unit: 1000 for km, 1 for m.
    dist_divisor: f64,
    /// Drop a municipality match further than this many metres. `None` leaves the
    /// nearest match unbounded, which is the historical behavior.
    max_municipality_dist_m: Option<f64>,
}

impl PlaceEnricher {
    /// Build the enricher from features already in memory (country attribute:
    /// name and optional ISO code). Used by [`PlaceEnricher::open`] and by
    /// tests, so the geometry can be exercised without shapefiles on disk.
    pub fn from_features(
        countries: Vec<(Rings, (String, Option<String>))>,
        municipalities: Option<Vec<(Rings, String)>>,
        region: BBox,
        proj: Laea,
        dist_divisor: f64,
        max_municipality_dist_m: Option<f64>,
    ) -> Self {
        PlaceEnricher {
            countries: PolygonIndex::build(countries, region, CROP_MARGIN_DEG, proj),
            municipalities: municipalities
                .map(|m| PolygonIndex::build(m, region, CROP_MARGIN_DEG, proj)),
            dist_divisor,
            max_municipality_dist_m,
        }
    }

    /// Wrap indexes built elsewhere, which is how the `--partition` path and its
    /// tests get one enricher per partition out of [`PolygonIndex::build_many`].
    pub fn from_indexes(
        countries: PolygonIndex<(String, Option<String>)>,
        municipalities: Option<PolygonIndex<String>>,
        dist_divisor: f64,
        max_municipality_dist_m: Option<f64>,
    ) -> Self {
        PlaceEnricher {
            countries,
            municipalities,
            dist_divisor,
            max_municipality_dist_m,
        }
    }

    /// Build one enricher per `(crop box, projection)` from a single read of
    /// each shapefile, for `--partition`. See [`PolygonIndex::build_many`] for
    /// why the read is shared and the geometry is not.
    pub fn open_many(
        countries: &Path,
        municipalities: Option<&Path>,
        regions: &[(BBox, Laea)],
        dist_divisor: f64,
        max_municipality_dist_m: Option<f64>,
    ) -> Result<Vec<Self>, Box<dyn Error>> {
        let (cfeats, mfeats) = Self::read_features(countries, municipalities)?;

        let cidx = PolygonIndex::build_many(&cfeats, regions, CROP_MARGIN_DEG);
        let midx = mfeats.map(|m| PolygonIndex::build_many(&m, regions, CROP_MARGIN_DEG));
        let built: Vec<Self> = match midx {
            Some(midx) => cidx
                .into_iter()
                .zip(midx)
                .map(|(c, m)| Self::from_indexes(c, Some(m), dist_divisor, max_municipality_dist_m))
                .collect(),
            None => cidx
                .into_iter()
                .map(|c| Self::from_indexes(c, None, dist_divisor, max_municipality_dist_m))
                .collect(),
        };

        // Only a run that matched nothing at all is worth saying here; a single
        // empty partition gets its crop widened and retried, so warning now
        // would report an empty column the finished run does not have.
        if built.iter().all(|e| e.countries.is_empty()) {
            eprintln!(
                "[seastamp] warning: no country polygons overlap any partition, so country will \
                 be empty for every row. Check --countries against your points."
            );
        }
        if built
            .iter()
            .all(|e| e.municipalities.as_ref().is_some_and(|m| m.is_empty()))
            && built.iter().any(|e| e.municipalities.is_some())
        {
            eprintln!(
                "[seastamp] warning: no municipality polygons overlap any partition. GISCO LAU \
                 covers Europe only, so this is expected outside it."
            );
        }
        Ok(built)
    }

    /// Read both shapefiles into memory, with their attributes resolved.
    #[allow(clippy::type_complexity)]
    fn read_features(
        countries: &Path,
        municipalities: Option<&Path>,
    ) -> Result<
        (
            Vec<(Rings, (String, Option<String>))>,
            Option<Vec<(Rings, String)>>,
        ),
        Box<dyn Error>,
    > {
        let mut cfeats = Vec::new();
        for (rings, record) in super::shp_polygons(countries)? {
            let Some(name) = field_string(&record, COUNTRY_NAME_FIELDS) else {
                continue;
            };
            let code = field_string(&record, COUNTRY_CODE_FIELDS);
            cfeats.push((rings, (name, code)));
        }
        if cfeats.is_empty() {
            return Err(format!("no named country polygons in {}", countries.display()).into());
        }

        let municipalities = match municipalities {
            Some(path) => {
                let mut mfeats = Vec::new();
                for (rings, record) in super::shp_polygons(path)? {
                    let Some(name) = field_string(&record, LAU_NAME_FIELDS) else {
                        continue;
                    };
                    mfeats.push((rings, name));
                }
                if mfeats.is_empty() {
                    return Err(
                        format!("no named municipality polygons in {}", path.display()).into()
                    );
                }
                Some(mfeats)
            }
            None => None,
        };
        Ok((cfeats, municipalities))
    }

    /// Open the Natural Earth countries shapefile and, when given, the GISCO
    /// LAU municipalities shapefile, cropped to `region`.
    pub fn open(
        countries: &Path,
        municipalities: Option<&Path>,
        region: BBox,
        proj: Laea,
        dist_divisor: f64,
        max_municipality_dist_m: Option<f64>,
    ) -> Result<Self, Box<dyn Error>> {
        let (cfeats, municipalities) = Self::read_features(countries, municipalities)?;
        let enr = Self::from_features(
            cfeats,
            municipalities,
            region,
            proj,
            dist_divisor,
            max_municipality_dist_m,
        );
        if enr.countries.is_empty() {
            eprintln!(
                "[seastamp] warning: no country polygons overlap the region, so country will be \
                 empty for every row. Check --region against your data."
            );
        }
        if enr.municipalities.as_ref().is_some_and(|m| m.is_empty()) {
            eprintln!(
                "[seastamp] warning: no municipality polygons overlap the region. GISCO LAU \
                 covers Europe only, so this is expected outside it; drop --municipalities, or \
                 widen --region."
            );
        }
        Ok(enr)
    }
}

impl Enricher for PlaceEnricher {
    /// `municipality_dist` is planar, and both lookups fall back to the nearest
    /// boundary for points inside no polygon, which is a planar comparison too.
    fn projection_center(&self) -> Option<(f64, f64)> {
        Some(self.countries.center())
    }

    /// Either lookup reaching past its crop makes the whole row provisional:
    /// the country and the municipality are cropped to the same box, and a row
    /// with one sound column and one suspect one is not worth keeping apart.
    ///
    /// A municipality match beyond `--max-municipality-dist` is deliberately
    /// exempt. It has already been discarded on purpose, so widening the crop to
    /// chase a nearer one that would also be discarded is wasted work.
    fn crop_shortfall(&self, lon: f64, lat: f64) -> f64 {
        let country = self.countries.crop_shortfall(lon, lat);
        let muni = match (&self.municipalities, self.max_municipality_dist_m) {
            (None, _) => 0.0,
            // A match already past the cutoff has been discarded on purpose, so
            // widening the crop to chase a nearer one that would also be
            // discarded is wasted work.
            (Some(m), Some(max)) => match m.locate_with_dist(lon, lat) {
                Some((_, d)) if d > max => 0.0,
                _ => m.crop_shortfall(lon, lat),
            },
            (Some(m), None) => m.crop_shortfall(lon, lat),
        };
        country.max(muni)
    }

    fn outputs(&self) -> Vec<OutputSpec> {
        let mut v = Vec::from([
            OutputSpec { name: "country".into(), kind: OutputKind::Text },
            OutputSpec { name: "country_code".into(), kind: OutputKind::Text },
            OutputSpec { name: "municipality".into(), kind: OutputKind::Text },
        ]);
        // Only meaningful when there is a municipality set to measure against, so
        // a run without --municipalities keeps exactly the columns it always had.
        if self.municipalities.is_some() {
            v.push(OutputSpec { name: "municipality_dist".into(), kind: OutputKind::Float });
        }
        v
    }

    fn enrich(&self, lon: f64, lat: f64) -> Vec<Value> {
        let (country, code) = match self.countries.locate(lon, lat) {
            Some((name, code)) => (Some(name.clone()), code.clone()),
            None => (None, None),
        };

        let mut out = Vec::from([Value::Text(country), Value::Text(code)]);

        match self.municipalities.as_ref() {
            None => out.push(Value::Text(None)),
            Some(index) => {
                // Zero when the point sits inside the polygon, otherwise how far
                // the nearest-boundary fallback had to reach. Past the cutoff the
                // match counts as no match, so the name and the distance drop out
                // together rather than leaving a distance with no name.
                let hit = index
                    .locate_with_dist(lon, lat)
                    .filter(|(_, d)| match self.max_municipality_dist_m {
                        Some(max) => *d <= max,
                        None => true,
                    });
                match hit {
                    Some((name, d)) => {
                        out.push(Value::Text(Some(name.clone())));
                        out.push(Value::Float(d / self.dist_divisor));
                    }
                    None => {
                        out.push(Value::Text(None));
                        out.push(Value::Float(f64::NAN));
                    }
                }
            }
        }
        out
    }
}

pub fn run(args: PlaceArgs) -> Result<(), Box<dyn Error>> {
    let mut s: Settings = resolve(&args.common, Some(&args.region))?;
    let countries = args
        .countries
        .ok_or("place requires --countries <Natural Earth countries shapefile>")?;
    if args.municipalities.is_none() {
        eprintln!("[seastamp] place: no --municipalities given, the municipality column will be empty");
        if args.max_municipality_dist.is_some() {
            eprintln!("[seastamp] place: --max-municipality-dist has no effect without --municipalities");
        }
    }
    let df = crate::io::read_frame(&args.common.input, args.common.in_format)?;
    let out_path = args
        .common
        .output
        .clone()
        .unwrap_or_else(|| super::default_output(&args.common.input, "place", args.common.in_format));

    // The cutoff is given in the output unit; the index measures in meters.
    let divisor = match args.unit {
        DistUnit::Km => 1000.0,
        DistUnit::M => 1.0,
    };
    let max_m = args.max_municipality_dist.map(|d| d * divisor);

    // --partition derives a region per piece of the input, so there is no single
    // region to settle and `apply_auto_region` has nothing to say. The pipeline
    // splits the locations and calls back here once per batch of partitions,
    // each call reading both shapefiles once for the whole batch.
    if s.partition {
        let municipalities = args.municipalities.clone();
        let build = move |regions: &[(BBox, Laea)]| {
            let built = PlaceEnricher::open_many(
                &countries,
                municipalities.as_deref(),
                regions,
                divisor,
                max_m,
            )?;
            Ok(built
                .into_iter()
                .map(|e| Box::new(e) as Box<dyn Enricher>)
                .collect())
        };
        // The column set must be known before any enricher exists, and it turns
        // only on whether a municipality set was given, so it is decided here
        // rather than read off a built enricher.
        let mut outputs = Vec::from([
            OutputSpec { name: "country".into(), kind: OutputKind::Text },
            OutputSpec { name: "country_code".into(), kind: OutputKind::Text },
            OutputSpec { name: "municipality".into(), kind: OutputKind::Text },
        ]);
        if args.municipalities.is_some() {
            outputs.push(OutputSpec { name: "municipality_dist".into(), kind: OutputKind::Float });
        }
        return run_partitioned(&build, &outputs, df, &s, &out_path, args.common.out_format);
    }

    // --region auto needs the points, so the region settles here, after the
    // table is read and before any reference data is cropped to it.
    let pts = crate::pipeline::locations(&df, &s)?;
    crate::config::apply_auto_region(&mut s, &pts)?;

    let proj = Laea::new(s.proj_lon0, s.proj_lat0);
    let enr = PlaceEnricher::open(
        &countries,
        args.municipalities.as_deref(),
        s.bbox,
        proj,
        divisor,
        max_m,
    )?;
    run_module(&enr, df, &s, &out_path, args.common.out_format)
}
