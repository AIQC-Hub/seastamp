//! Nearest country and municipality for an offshore point.
//!
//! Points at sea sit inside no land polygon, so both lookups resolve a point by
//! containment first (points on land) and nearest boundary otherwise, through
//! the shared [`PolygonIndex`]:
//!   - Country: Natural Earth country polygons. Appends `country` and
//!     `country_code` (ISO 3166-1 alpha-3 where Natural Earth provides one).
//!   - Municipality: GISCO LAU polygons. Appends `municipality`. The set is
//!     optional; without `--municipalities` the column stays empty.
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

use crate::cli::PlaceArgs;
use crate::config::{resolve, BBox, Settings};
use crate::geo::vector::{PolygonIndex, Rings, CROP_MARGIN_DEG};
use crate::geo::Laea;
use crate::pipeline::{run_module, Enricher, OutputKind, OutputSpec, Value};

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
    ) -> Self {
        PlaceEnricher {
            countries: PolygonIndex::build(countries, region, CROP_MARGIN_DEG, proj),
            municipalities: municipalities
                .map(|m| PolygonIndex::build(m, region, CROP_MARGIN_DEG, proj)),
        }
    }

    /// Open the Natural Earth countries shapefile and, when given, the GISCO
    /// LAU municipalities shapefile, cropped to `region`.
    pub fn open(
        countries: &Path,
        municipalities: Option<&Path>,
        region: BBox,
        proj: Laea,
    ) -> Result<Self, Box<dyn Error>> {
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

        Ok(Self::from_features(cfeats, municipalities, region, proj))
    }
}

impl Enricher for PlaceEnricher {
    fn outputs(&self) -> Vec<OutputSpec> {
        Vec::from([
            OutputSpec { name: "country".into(), kind: OutputKind::Text },
            OutputSpec { name: "country_code".into(), kind: OutputKind::Text },
            OutputSpec { name: "municipality".into(), kind: OutputKind::Text },
        ])
    }

    fn enrich(&self, lon: f64, lat: f64) -> Vec<Value> {
        let (country, code) = match self.countries.locate(lon, lat) {
            Some((name, code)) => (Some(name.clone()), code.clone()),
            None => (None, None),
        };
        let municipality = self
            .municipalities
            .as_ref()
            .and_then(|m| m.locate(lon, lat))
            .cloned();
        Vec::from([
            Value::Text(country),
            Value::Text(code),
            Value::Text(municipality),
        ])
    }
}

pub fn run(args: PlaceArgs) -> Result<(), Box<dyn Error>> {
    let s: Settings = resolve(&args.common, Some(&args.region))?;
    let countries = args
        .countries
        .ok_or("place requires --countries <Natural Earth countries shapefile>")?;
    if args.municipalities.is_none() {
        eprintln!("[seastamp] place: no --municipalities given, the municipality column will be empty");
    }
    let df = crate::io::read_frame(&args.common.input, args.common.in_format)?;
    let out_path = args
        .common
        .output
        .clone()
        .unwrap_or_else(|| super::default_output(&args.common.input, "place", args.common.in_format));

    let proj = Laea::new(s.proj_lon0, s.proj_lat0);
    let enr = PlaceEnricher::open(&countries, args.municipalities.as_deref(), s.bbox, proj)?;
    run_module(&enr, df, &s, &out_path, args.common.out_format)
}
