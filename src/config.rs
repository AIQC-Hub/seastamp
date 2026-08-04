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
}

/// Every preset name, in the order they are documented. Kept beside
/// [`preset_bbox`] so a new preset shows up in `seastamp regions` and in the
/// test below without a second edit.
pub const PRESET_NAMES: [&str; 7] = [
    "global",
    "baltic",
    "norway",
    "arctic",
    "atlantic",
    "europe",
    "mediterranean",
];

/// Named region presets. Extend this as new regions are needed.
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

/// Merge CLI arguments and the optional config file into [`Settings`].
/// Modules without a region (e.g. `depth`) pass `region = None`.
pub fn resolve(common: &CommonArgs, region: Option<&RegionArgs>) -> Result<Settings, Box<dyn Error>> {
    let fc: FileConfig = match &common.config {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("cannot read config {}: {e}", p.display()))?;
            toml::from_str(&text).map_err(|e| format!("invalid config {}: {e}", p.display()))?
        }
        None => FileConfig::default(),
    };

    let region_name = region.and_then(|r| r.region.clone()).or_else(|| fc.region.clone());
    let mut bbox = region_name.as_deref().and_then(preset_bbox).unwrap_or(GLOBAL);

    if let Some(v) = region.and_then(|r| r.min_lon).or(fc.min_lon) { bbox.min_lon = v; }
    if let Some(v) = region.and_then(|r| r.max_lon).or(fc.max_lon) { bbox.max_lon = v; }
    if let Some(v) = region.and_then(|r| r.min_lat).or(fc.min_lat) { bbox.min_lat = v; }
    if let Some(v) = region.and_then(|r| r.max_lat).or(fc.max_lat) { bbox.max_lat = v; }

    // A box is a plain lon/lat rectangle: cropping compares longitudes without
    // wrapping, so one that crosses the antimeridian would silently match no
    // reference feature and every point would come back null. Say so instead.
    if bbox.min_lon > bbox.max_lon {
        return Err(format!(
            "region min-lon ({}) is greater than max-lon ({}). A region crossing the \
             antimeridian is not supported; split the run into an eastern and a western \
             box, or use a whole-globe region",
            bbox.min_lon, bbox.max_lon
        )
        .into());
    }
    if bbox.min_lat > bbox.max_lat {
        return Err(format!(
            "region min-lat ({}) is greater than max-lat ({})",
            bbox.min_lat, bbox.max_lat
        )
        .into());
    }

    let (clon, clat) = bbox.center();
    let proj_lon0 = region.and_then(|r| r.proj_lon0).or(fc.proj_lon0).unwrap_or(clon);
    let proj_lat0 = region.and_then(|r| r.proj_lat0).or(fc.proj_lat0).unwrap_or(clat);

    Ok(Settings {
        lon_col: common.lon_col.clone(),
        lat_col: common.lat_col.clone(),
        decimals: common.decimals,
        threads: common.threads,
        overwrite: common.overwrite,
        bbox,
        proj_lon0,
        proj_lat0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A region crossing the antimeridian must be refused, not accepted and then
    /// silently matched against nothing.
    #[test]
    fn antimeridian_box_is_rejected() {
        let b = BBox { min_lon: 170.0, max_lon: -170.0, min_lat: -10.0, max_lat: 10.0 };
        assert!(b.min_lon > b.max_lon, "this is the shape resolve must reject");
        let flipped = BBox { min_lon: -10.0, max_lon: 10.0, min_lat: 20.0, max_lat: -20.0 };
        assert!(flipped.min_lat > flipped.max_lat);
    }

    #[test]
    fn named_presets_resolve() {
        for name in PRESET_NAMES {
            assert!(preset_bbox(name).is_some(), "missing preset '{name}'");
        }
        // case-insensitive; an unknown name is None
        assert!(preset_bbox("EUROPE").is_some());
        assert!(preset_bbox("atlantis").is_none());
    }

    #[test]
    fn default_region_is_global() {
        // No preset and no bounds means the whole globe.
        let region_name: Option<&str> = None;
        let b = region_name.and_then(preset_bbox).unwrap_or(GLOBAL);
        assert_eq!((b.min_lon, b.max_lon, b.min_lat, b.max_lat), (-180.0, 180.0, -90.0, 90.0));
    }
}
