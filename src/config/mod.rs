//! Resolved settings and the optional TOML config file.
//!
//! Precedence for the region box and projection center is
//! `preset/built-in default < config file < CLI flag`. The coordinate columns,
//! rounding, and thread count come straight from the CLI (they always carry a
//! default, so there is nothing to layer). Per-field override flags in the
//! ctddump style can be added later if config-driven column names are wanted.

use serde::Deserialize;
use std::error::Error;

use crate::cli::{CommonArgs, RegionArgs};

mod iho_areas;
pub use iho_areas::{IhoArea, IHO_AREAS};

/// A geographic bounding box in degrees.
#[derive(Debug, Clone, Copy)]
pub struct BBox {
    pub min_lon: f64,
    pub max_lon: f64,
    pub min_lat: f64,
    pub max_lat: f64,
}

impl BBox {
    pub fn center(&self) -> (f64, f64) {
        (
            (self.min_lon + self.max_lon) / 2.0,
            (self.min_lat + self.max_lat) / 2.0,
        )
    }

    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        lon >= self.min_lon && lon <= self.max_lon && lat >= self.min_lat && lat <= self.max_lat
    }
}

/// The Baltic Sea box, the `baltic` region preset.
pub const BALTIC: BBox = BBox {
    min_lon: 8.0,
    max_lon: 31.0,
    min_lat: 53.0,
    max_lat: 66.0,
};

/// The whole globe, used as the default when no region is set.
pub const GLOBAL: BBox = BBox {
    min_lon: -180.0,
    max_lon: 180.0,
    min_lat: -90.0,
    max_lat: 90.0,
};

/// Everything a module needs after CLI + config are merged.
#[derive(Debug, Clone)]
pub struct Settings {
    pub lon_col: String,
    pub lat_col: String,
    pub decimals: u32,
    pub threads: Option<usize>,
    pub overwrite: bool,
    pub bbox: BBox,
    pub proj_lon0: f64,
    pub proj_lat0: f64,
    /// Set when the region is to be derived from the input points, holding the
    /// bounds the caller gave explicitly so they can be re-applied on top of the
    /// derived box. `None` once the region is settled, and for modules that
    /// have no region at all.
    pub auto: Option<RegionOverrides>,
    /// `--partition`: derive a region per sub-region of the input rather than
    /// one for all of it. The single `bbox` and projection center above are then
    /// unused, which is why the flag conflicts with every way of setting them.
    pub partition: bool,
}

/// The region fields the caller set explicitly, kept so `auto` can derive the
/// rest without overruling them.
#[derive(Debug, Clone, Default)]
pub struct RegionOverrides {
    pub min_lon: Option<f64>,
    pub max_lon: Option<f64>,
    pub min_lat: Option<f64>,
    pub max_lat: Option<f64>,
    pub proj_lon0: Option<f64>,
    pub proj_lat0: Option<f64>,
}

impl RegionOverrides {
    /// Overwrite whichever bounds were given explicitly.
    fn apply(&self, b: &mut BBox) {
        if let Some(v) = self.min_lon {
            b.min_lon = v;
        }
        if let Some(v) = self.max_lon {
            b.max_lon = v;
        }
        if let Some(v) = self.min_lat {
            b.min_lat = v;
        }
        if let Some(v) = self.max_lat {
            b.max_lat = v;
        }
    }
}

/// The `--region` value that derives the box and center from the input points.
pub const AUTO_REGION: &str = "auto";

/// Every preset name, in the order they are documented. Kept beside
/// [`preset_bbox`] so a new preset shows up in `seastamp regions` and in the
/// test below without a second edit. `auto` is not among them: it names no box.
pub const PRESET_NAMES: [&str; 7] = [
    "global",
    "baltic",
    "norway",
    "arctic",
    "atlantic",
    "europe",
    "mediterranean",
];

/// Named region presets. Extend this as new regions are needed, and add the
/// name to [`PRESET_NAMES`] as well.
pub fn preset_bbox(name: &str) -> Option<BBox> {
    match name.to_ascii_lowercase().as_str() {
        "baltic" => Some(BALTIC),
        "norway" => Some(BBox { min_lon: -10.0, max_lon: 45.0, min_lat: 55.0, max_lat: 85.0 }),
        "arctic" => Some(BBox { min_lon: -180.0, max_lon: 180.0, min_lat: 60.0, max_lat: 90.0 }),
        "atlantic" => Some(BBox { min_lon: -83.0, max_lon: 20.0, min_lat: -60.0, max_lat: 70.0 }),
        "europe" => Some(BBox { min_lon: -25.0, max_lon: 45.0, min_lat: 34.0, max_lat: 72.0 }),
        "mediterranean" => Some(BBox { min_lon: -6.0, max_lon: 37.0, min_lat: 30.0, max_lat: 46.0 }),
        "global" => Some(GLOBAL),
        _ => None,
    }
}

/// Look up an IHO Sea Areas v3 area by name, case-insensitively.
pub fn iho_area(name: &str) -> Option<&'static IhoArea> {
    IHO_AREAS.iter().find(|a| a.name.eq_ignore_ascii_case(name))
}

/// Resolve a `--region` name to a box: a preset first, then an IHO Sea Areas
/// name. An unknown name is an error rather than a silent fall back to the
/// whole globe, which used to turn a typo into quietly wrong distances.
pub fn region_bbox(name: &str) -> Result<BBox, Box<dyn Error>> {
    if let Some(b) = preset_bbox(name) {
        return Ok(b);
    }
    if let Some(a) = iho_area(name) {
        if a.crosses {
            return Err(format!(
                "the IHO area '{}' crosses the antimeridian ({} to {}), which cannot be \
                 expressed as a region box. Use --region auto, or split the run into an \
                 eastern and a western box",
                a.name, a.bbox.min_lon, a.bbox.max_lon
            )
            .into());
        }
        return Ok(a.bbox);
    }
    Err(format!("unknown region '{name}'.{}", did_you_mean(name)).into())
}

/// Words too common among sea names to narrow anything down: half the IHO list
/// ends in one of them, so matching on them would suggest half the list.
const GENERIC_WORDS: [&str; 8] = [
    "sea", "ocean", "gulf", "bay", "strait", "channel", "the", "of",
];

/// A short hint listing names that share a distinctive word with what was
/// typed, so a near miss among 108 names is easy to fix. Matching per word
/// rather than on the whole string is what catches "Barent Sea" for
/// "Barentsz Sea", where neither name contains the other.
fn did_you_mean(name: &str) -> String {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !GENERIC_WORDS.contains(w))
        .collect();
    let near: Vec<&str> = PRESET_NAMES
        .iter()
        .copied()
        .chain(IHO_AREAS.iter().map(|a| a.name))
        .filter(|cand| {
            let c = cand.to_lowercase();
            c.contains(&lower) || words.iter().any(|w| c.contains(w))
        })
        .take(5)
        .collect();
    if near.is_empty() {
        format!(
            " Use a preset ({}), an IHO Sea Areas name, or 'auto'. \
             Run 'seastamp regions' to list them.",
            PRESET_NAMES.join(", ")
        )
    } else {
        format!(" Did you mean: {}?", near.join(", "))
    }
}

/// Optional TOML config. Every field is optional and, when present, sits between
/// the built-in default and the CLI flag.
#[derive(Debug, Default, Deserialize)]
pub struct FileConfig {
    pub region: Option<String>,
    pub min_lon: Option<f64>,
    pub max_lon: Option<f64>,
    pub min_lat: Option<f64>,
    pub max_lat: Option<f64>,
    pub proj_lon0: Option<f64>,
    pub proj_lat0: Option<f64>,
}

/// A rectangle is a rectangle: cropping compares longitudes without wrapping, so
/// a box crossing the antimeridian would match no reference feature and return
/// nulls everywhere. Refuse it instead.
pub fn validate_bbox(b: &BBox) -> Result<(), Box<dyn Error>> {
    if b.min_lon > b.max_lon {
        return Err(format!(
            "region min-lon ({}) is greater than max-lon ({}). A region crossing the \
             antimeridian is not supported; split the run into an eastern and a western \
             box, or use --region auto, which keeps the projection centered correctly \
             and only widens the crop",
            b.min_lon, b.max_lon
        )
        .into());
    }
    if b.min_lat > b.max_lat {
        return Err(format!(
            "region min-lat ({}) is greater than max-lat ({})",
            b.min_lat, b.max_lat
        )
        .into());
    }
    Ok(())
}

/// Merge CLI arguments and the optional config file into [`Settings`].
/// Modules without a region (e.g. `depth`) pass `region = None`.
///
/// When no region name is given, or the name is `auto`, the box and center are
/// left to be derived from the input points by [`apply_auto_region`], which the
/// module calls once it has read the table. Any bound or center given
/// explicitly still wins, so the documented precedence holds either way.
pub fn resolve(common: &CommonArgs, region: Option<&RegionArgs>) -> Result<Settings, Box<dyn Error>> {
    let fc: FileConfig = match &common.config {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("cannot read config {}: {e}", p.display()))?;
            toml::from_str(&text).map_err(|e| format!("invalid config {}: {e}", p.display()))?
        }
        None => FileConfig::default(),
    };

    let overrides = RegionOverrides {
        min_lon: region.and_then(|r| r.min_lon).or(fc.min_lon),
        max_lon: region.and_then(|r| r.max_lon).or(fc.max_lon),
        min_lat: region.and_then(|r| r.min_lat).or(fc.min_lat),
        max_lat: region.and_then(|r| r.max_lat).or(fc.max_lat),
        proj_lon0: region.and_then(|r| r.proj_lon0).or(fc.proj_lon0),
        proj_lat0: region.and_then(|r| r.proj_lat0).or(fc.proj_lat0),
    };

    let region_name = region.and_then(|r| r.region.clone()).or_else(|| fc.region.clone());
    // No name at all means auto: the region is derived from the data, which is
    // right far more often than the whole globe ever was.
    let auto = match region_name.as_deref() {
        None => true,
        Some(n) => n.eq_ignore_ascii_case(AUTO_REGION),
        };
    let mut bbox = match (&region_name, auto) {
        (_, true) => GLOBAL, // a placeholder until the points are read
        (Some(n), false) => region_bbox(n)?,
        (None, false) => unreachable!("a nameless region is always auto"),
    };
    overrides.apply(&mut bbox);
    validate_bbox(&bbox)?;

    let (clon, clat) = bbox.center();
    Ok(Settings {
        lon_col: common.lon_col.clone(),
        lat_col: common.lat_col.clone(),
        decimals: common.decimals,
        threads: common.threads,
        overwrite: common.overwrite,
        bbox,
        proj_lon0: overrides.proj_lon0.unwrap_or(clon),
        proj_lat0: overrides.proj_lat0.unwrap_or(clat),
        auto: auto.then_some(overrides),
        partition: region.is_some_and(|r| r.partition),
    })
}

/// Extra degrees added around the data's own extent when `auto` builds a crop
/// box. The modules add their own [`crate::geo::vector::CROP_MARGIN_DEG`] on
/// top, so the reference data actually kept reaches about 10 degrees, roughly
/// 1100 km, beyond the outermost point. That is deliberately generous: an
/// offshore point's nearest coast can be hundreds of km away, and a box cropped
/// to the points alone would cut the coastline out and overstate the distance.
pub const AUTO_PAD_DEG: f64 = 5.0;

/// Below this resultant length (see [`crate::geo::arc::spherical_center`]) the
/// points are spread so widely that no single projection center serves them,
/// and `auto` says so rather than pretending otherwise.
const NO_CENTER_RESULTANT: f64 = 0.5;

/// The crop box a point set earns for itself: its own extent padded by
/// [`AUTO_PAD_DEG`] and clamped to the globe. The `bool` is true when the set
/// spans the antimeridian, in which case every longitude is kept, because a
/// rectangle cannot say "170 E to 170 W". Only cropping widens; nothing here
/// touches the projection center, which has no seam.
///
/// Shared by `--region auto`, which calls it once for the whole input, and
/// `--partition`, which calls it once per partition.
pub fn auto_bbox(pts: &[(f64, f64)]) -> Option<(BBox, bool)> {
    let (west, east, crosses) = crate::geo::arc::points_arc(pts.iter().map(|&(lo, _)| lo))?;
    let (mut min_lat, mut max_lat) = (f64::INFINITY, f64::NEG_INFINITY);
    for &(_, la) in pts {
        min_lat = min_lat.min(la);
        max_lat = max_lat.max(la);
    }
    let bbox = BBox {
        min_lon: if crosses { -180.0 } else { (west - AUTO_PAD_DEG).max(-180.0) },
        max_lon: if crosses { 180.0 } else { (east + AUTO_PAD_DEG).min(180.0) },
        min_lat: (min_lat - AUTO_PAD_DEG).max(-90.0),
        max_lat: (max_lat + AUTO_PAD_DEG).min(90.0),
    };
    Some((bbox, crosses))
}

/// Derive the region from the input points, for `--region auto`. A no-op when
/// the region was named or given as an explicit box.
///
/// The center is the mean direction of the points in three dimensions, which is
/// correct across the antimeridian and at the poles: a ring of stations around
/// the North Pole centers on the pole, which no rectangle's center can express.
/// The crop box is the points' own extent padded by [`AUTO_PAD_DEG`], except
/// that a set spanning the antimeridian falls back to a full longitude range,
/// since a rectangle cannot say "170 E to 170 W". Only cropping is widened by
/// that: the projection has no seam, so distances stay accurate.
pub fn apply_auto_region(s: &mut Settings, pts: &[(f64, f64)]) -> Result<(), Box<dyn Error>> {
    let Some(ov) = s.auto.clone() else {
        return Ok(());
    };

    let finite: Vec<(f64, f64)> = pts
        .iter()
        .copied()
        .filter(|(lo, la)| lo.is_finite() && la.is_finite())
        .collect();
    if finite.is_empty() {
        eprintln!(
            "[seastamp] warning: --region auto found no usable coordinates, falling back to \
             the whole globe centered on (0, 0)."
        );
        return Ok(());
    }
    // A `None` center means the points cancel exactly, which only a set spread
    // right around the globe can do. There is no direction to center on, so say
    // so and leave the global default rather than inventing one.
    let (Some((clon, clat, resultant)), Some((bbox, crosses))) = (
        crate::geo::arc::spherical_center(&finite),
        auto_bbox(&finite),
    ) else {
        eprintln!(
            "[seastamp] warning: the points are spread right around the globe, so they have no \
             mean direction and --region auto cannot center on them."
        );
        eprintln!(
            "[seastamp] warning: falling back to the whole globe centered on (0, 0). Pass \
             --partition if the distances matter."
        );
        return Ok(());
    };

    let spread_too_wide = resultant < NO_CENTER_RESULTANT;
    if crosses {
        // `auto_bbox` has already kept every longitude: a rectangle cannot
        // express the short way round. The projection center is unaffected,
        // which is the part that matters.
        //
        // Not worth saying when the points are spread so far that the warning
        // below is the real story.
        if !spread_too_wide {
            eprintln!(
                "[seastamp] --region auto: the points span the antimeridian, so the crop keeps \
                 every longitude. Distances are unaffected, the projection is centered on the data."
            );
        }
    }

    s.bbox = bbox;
    s.proj_lon0 = clon;
    s.proj_lat0 = clat;
    ov.apply(&mut s.bbox);
    if let Some(v) = ov.proj_lon0 {
        s.proj_lon0 = v;
    }
    if let Some(v) = ov.proj_lat0 {
        s.proj_lat0 = v;
    }
    validate_bbox(&s.bbox)?;

    eprintln!(
        "[seastamp] --region auto: box ({:.2}, {:.2}, {:.2}, {:.2}), projection centered on \
         ({:.2}, {:.2})",
        s.bbox.min_lon, s.bbox.max_lon, s.bbox.min_lat, s.bbox.max_lat, s.proj_lon0, s.proj_lat0
    );
    if spread_too_wide {
        eprintln!(
            "[seastamp] warning: the points are spread over too much of the globe for any one \
             projection to serve them (clustering {:.2} of a possible 1.00).",
            resultant
        );
        eprintln!(
            "[seastamp] warning: --region auto cannot help here. Pass --partition to measure \
             each area in its own projection."
        );
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A box crossing the antimeridian must be refused, not accepted and then
    /// silently matched against nothing.
    #[test]
    fn antimeridian_box_is_rejected() {
        let b = BBox { min_lon: 170.0, max_lon: -170.0, min_lat: -10.0, max_lat: 10.0 };
        assert!(validate_bbox(&b).is_err());
        let flipped = BBox { min_lon: -10.0, max_lon: 10.0, min_lat: 20.0, max_lat: -20.0 };
        assert!(validate_bbox(&flipped).is_err());
        assert!(validate_bbox(&BALTIC).is_ok());
    }

    #[test]
    fn named_presets_resolve() {
        for name in PRESET_NAMES {
            assert!(preset_bbox(name).is_some(), "missing preset '{name}'");
        }
        assert!(preset_bbox("EUROPE").is_some()); // case-insensitive
        assert!(preset_bbox("atlantis").is_none());
    }

    /// The IHO names resolve through the same flag as the presets, without the
    /// user holding a copy of the shapefile.
    #[test]
    fn iho_names_resolve_by_region() {
        let b = region_bbox("Barentsz Sea").expect("a known IHO area");
        assert!(b.min_lon > 16.0 && b.max_lon < 69.0, "unexpected box: {b:?}");
        assert!(region_bbox("BARENTSZ SEA").is_ok(), "case-insensitive");
        // presets still win, and there are 101 IHO areas behind them
        assert_eq!(IHO_AREAS.len(), 101);
        assert_eq!(region_bbox("baltic").unwrap().max_lon, BALTIC.max_lon);
    }

    /// The four crossing areas name a box seastamp cannot use, so they are
    /// refused with the reason rather than handed back inverted.
    #[test]
    fn crossing_iho_areas_are_refused_by_name() {
        for name in ["Bering Sea", "Chukchi Sea", "North Pacific Ocean", "South Pacific Ocean"] {
            let err = region_bbox(name).expect_err("a crossing area is not a usable region");
            let msg = err.to_string();
            assert!(msg.contains("antimeridian"), "unexpected error for {name}: {msg}");
            assert!(msg.contains("auto"), "the error should point at the way out: {msg}");
        }
    }

    /// A typo used to fall back to the whole globe and quietly mismeasure. Now
    /// it is an error, and one that names the likely intent.
    #[test]
    fn unknown_region_errors_and_suggests() {
        let msg = region_bbox("Barent Sea").unwrap_err().to_string();
        assert!(msg.contains("Barentsz Sea"), "no suggestion in: {msg}");
        let msg = region_bbox("okhotsk").unwrap_err().to_string();
        assert!(msg.contains("Sea of Okhotsk"), "no suggestion in: {msg}");
        // nothing close: fall back to telling the user where to look
        let msg = region_bbox("atlantis").unwrap_err().to_string();
        assert!(msg.contains("seastamp regions"), "no pointer in: {msg}");
    }

    fn auto_settings() -> Settings {
        Settings {
            lon_col: "longitude".into(),
            lat_col: "latitude".into(),
            decimals: 3,
            threads: None,
            overwrite: false,
            bbox: GLOBAL,
            proj_lon0: 0.0,
            proj_lat0: 0.0,
            auto: Some(RegionOverrides::default()),
            partition: false,
        }
    }

    /// The ordinary case: a survey gets a box around itself and a center on it.
    #[test]
    fn auto_follows_a_local_cluster() {
        let mut s = auto_settings();
        let pts = [(4.1, 60.4), (4.9, 60.9), (3.7, 60.1)];
        apply_auto_region(&mut s, &pts).unwrap();

        assert!((s.proj_lon0 - 4.2).abs() < 0.2, "lon0 was {}", s.proj_lon0);
        assert!((s.proj_lat0 - 60.5).abs() < 0.2, "lat0 was {}", s.proj_lat0);
        // the data extent padded by AUTO_PAD_DEG, not the whole globe
        assert!((s.bbox.min_lon - (3.7 - AUTO_PAD_DEG)).abs() < 1e-9);
        assert!((s.bbox.max_lat - (60.9 + AUTO_PAD_DEG)).abs() < 1e-9);
    }

    /// Points either side of the antimeridian: the crop gives up on longitude,
    /// but the center, which is what distances depend on, stays on the data.
    #[test]
    fn auto_centers_correctly_across_the_dateline() {
        let mut s = auto_settings();
        let pts = [(178.5, -17.5), (-179.2, -16.8), (179.9, -18.1)];
        apply_auto_region(&mut s, &pts).unwrap();

        assert!(s.proj_lon0.abs() > 179.0, "lon0 was {}", s.proj_lon0);
        assert!((s.proj_lat0 + 17.5).abs() < 0.5, "lat0 was {}", s.proj_lat0);
        assert_eq!((s.bbox.min_lon, s.bbox.max_lon), (-180.0, 180.0));
        // latitude still crops
        assert!(s.bbox.min_lat > -25.0 && s.bbox.max_lat < -10.0);
    }

    /// A ring of stations around the pole centers on the pole. No box could.
    #[test]
    fn auto_centers_a_polar_ring_on_the_pole() {
        let mut s = auto_settings();
        let pts: Vec<(f64, f64)> = (-180..180).step_by(20).map(|l| (l as f64, 75.0)).collect();
        apply_auto_region(&mut s, &pts).unwrap();

        assert!((s.proj_lat0 - 90.0).abs() < 1e-6, "lat0 was {}", s.proj_lat0);
        assert_eq!((s.bbox.min_lon, s.bbox.max_lon), (-180.0, 180.0));
    }

    /// Explicit bounds still win over the derived ones, so the documented
    /// precedence holds in auto mode too.
    #[test]
    fn explicit_bounds_override_auto() {
        let mut s = auto_settings();
        s.auto = Some(RegionOverrides {
            min_lon: Some(-30.0),
            proj_lat0: Some(0.0),
            ..Default::default()
        });
        apply_auto_region(&mut s, &[(4.1, 60.4), (4.9, 60.9)]).unwrap();

        assert_eq!(s.bbox.min_lon, -30.0, "the explicit bound must survive");
        assert_eq!(s.proj_lat0, 0.0, "the explicit center must survive");
        assert!((s.proj_lon0 - 4.5).abs() < 0.5, "the rest is still derived");
    }

    /// A settled region is left alone: `apply_auto_region` is a no-op when the
    /// caller named a region.
    #[test]
    fn a_named_region_is_not_touched() {
        let mut s = auto_settings();
        s.auto = None;
        s.bbox = BALTIC;
        apply_auto_region(&mut s, &[(-150.0, 20.0)]).unwrap();
        assert_eq!(s.bbox.min_lon, BALTIC.min_lon);
        assert_eq!(s.proj_lon0, 0.0);
    }
}
