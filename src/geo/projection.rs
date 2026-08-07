//! Spherical Lambert Azimuthal Equal-Area (LAEA) projection and great-circle
//! distance, both in pure Rust.
//!
//! Distances are taken the way a planar CRS such as EPSG:3035 (LAEA Europe)
//! would give them, but without binding PROJ: a LAEA centered on the region
//! turns lon/lat into meters, and planar distance there is accurate for the
//! "nearest coast" style query at regional scale. A single sphere (authalic
//! radius) is used rather than the GRS80 ellipsoid; the error is well under
//! coastline resolution for regional work. An ellipsoidal variant can replace
//! this if sub-meter accuracy is ever required (see CLAUDE.md).
//!
//! Formulas: Snyder, "Map Projections: A Working Manual" (USGS PP 1395), LAEA
//! spherical case.

/// Authalic (equal-area) radius of the GRS80 ellipsoid, in meters. Matches the
/// sphere EPSG:3035 is defined against closely enough for regional distances.
pub const EARTH_RADIUS_M: f64 = 6_371_007.181;

/// Mean Earth radius used for great-circle distance, in meters.
pub const MEAN_RADIUS_M: f64 = 6_371_008.8;

/// A LAEA projection centered at `(lon0, lat0)`.
#[derive(Debug, Clone, Copy)]
pub struct Laea {
    lam0: f64,
    phi0: f64,
    r: f64,
}

impl Laea {
    /// Center given in degrees.
    pub fn new(lon0_deg: f64, lat0_deg: f64) -> Self {
        Self {
            lam0: lon0_deg.to_radians(),
            phi0: lat0_deg.to_radians(),
            r: EARTH_RADIUS_M,
        }
    }

    /// The projection center as `(lon, lat)` in degrees. Distances measured in
    /// this projection are accurate near it and degrade with distance from it,
    /// so callers report it when warning about far-away input.
    pub fn center(&self) -> (f64, f64) {
        (self.lam0.to_degrees(), self.phi0.to_degrees())
    }

    /// Project lon/lat (degrees) to `(x, y)` in meters.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> (f64, f64) {
        let lam = lon_deg.to_radians();
        let phi = lat_deg.to_radians();
        let dlam = lam - self.lam0;
        let (sin_phi, cos_phi) = (phi.sin(), phi.cos());
        let (sin_phi0, cos_phi0) = (self.phi0.sin(), self.phi0.cos());
        let denom = 1.0 + sin_phi0 * sin_phi + cos_phi0 * cos_phi * dlam.cos();
        // Guard the antipode where denom -> 0.
        let kp = if denom > 0.0 { (2.0 / denom).sqrt() } else { 0.0 };
        let x = self.r * kp * cos_phi * dlam.sin();
        let y = self.r * kp * (cos_phi0 * sin_phi - sin_phi0 * cos_phi * dlam.cos());
        (x, y)
    }

    /// Inverse: `(x, y)` meters back to lon/lat (degrees).
    pub fn inverse(&self, x: f64, y: f64) -> (f64, f64) {
        let rho = (x * x + y * y).sqrt();
        if rho < 1e-9 {
            return (self.lam0.to_degrees(), self.phi0.to_degrees());
        }
        let c = 2.0 * (rho / (2.0 * self.r)).asin();
        let (sin_c, cos_c) = (c.sin(), c.cos());
        let (sin_phi0, cos_phi0) = (self.phi0.sin(), self.phi0.cos());
        let phi = (cos_c * sin_phi0 + (y * sin_c * cos_phi0) / rho).asin();
        let lam = self.lam0
            + (x * sin_c).atan2(rho * cos_phi0 * cos_c - y * sin_phi0 * sin_c);
        (lam.to_degrees(), phi.to_degrees())
    }
}

/// Unit-sphere `(x, y, z)` for a lon/lat point in degrees.
///
/// Nearest neighbor by Euclidean chord distance between these vectors is the
/// nearest by great-circle distance (the chord grows monotonically with the
/// central angle), so a 3D R-tree over unit-sphere points answers "nearest of a
/// set" correctly anywhere on the globe, with no projection center and none of
/// the distortion a single planar projection has away from its center. Pair it
/// with [`chord2_to_m`] to turn the squared chord the R-tree reports back into
/// meters.
pub fn unit_sphere(lon_deg: f64, lat_deg: f64) -> [f64; 3] {
    let (lam, phi) = (lon_deg.to_radians(), lat_deg.to_radians());
    let (cos_phi, sin_phi) = (phi.cos(), phi.sin());
    [cos_phi * lam.cos(), cos_phi * lam.sin(), sin_phi]
}

/// Great-circle distance in meters from a squared chord length between two
/// [`unit_sphere`] vectors. The chord `c` and central angle `theta` relate by
/// `c = 2 sin(theta/2)`, so `theta = 2 asin(c/2)` and the distance is
/// `R * theta`. The half-chord is clamped to `[0, 1]` against rounding at the
/// antipode.
pub fn chord2_to_m(chord2: f64) -> f64 {
    let half = (chord2.max(0.0).sqrt() / 2.0).min(1.0);
    2.0 * MEAN_RADIUS_M * half.asin()
}

/// Great-circle distance between two lon/lat points (degrees), in meters.
pub fn haversine_m(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dphi = (lat2 - lat1).to_radians();
    let dlam = (lon2 - lon1).to_radians();
    let a = (dphi / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dlam / 2.0).sin().powi(2);
    2.0 * MEAN_RADIUS_M * a.sqrt().asin()
}

/// The relative error a planar LAEA measurement picks up at angular distance
/// `angle_rad` from its center.
///
/// For an azimuthal equal-area projection the radial scale at angular distance
/// `c` is `1/k'` with `k' = sqrt(2 / (1 + cos c))`, so a length measured
/// radially comes out scaled by `sqrt((1 + cos c) / 2)`. Returned as a signed
/// fraction: negative means the projection understates the true distance.
pub fn laea_radial_error(angle_rad: f64) -> f64 {
    ((1.0 + angle_rad.cos()) / 2.0).sqrt() - 1.0
}

/// The same, from a great-circle distance in meters.
pub fn laea_error_at(dist_m: f64) -> f64 {
    laea_radial_error(dist_m / MEAN_RADIUS_M)
}

/// The inverse: how far from its center, in meters, a LAEA projection stays
/// within `tolerance`. This is the radius one projection can serve at a given
/// accuracy, which is what `--partition` sizes its pieces against.
///
/// From `sqrt((1 + cos c) / 2) = 1 - tolerance`, so
/// `cos c = 2 (1 - tolerance)^2 - 1`.
pub fn laea_radius_m(tolerance: f64) -> f64 {
    let cos_c = (2.0 * (1.0 - tolerance).powi(2) - 1.0).clamp(-1.0, 1.0);
    MEAN_RADIUS_M * cos_c.acos()
}

/// The planar-distance error at which seastamp stops being quiet, as a
/// fraction: about 23 degrees, or 2500 km, from the projection center.
///
/// One number serves two purposes on purpose. [`crate::pipeline::run_module`]
/// warns when the input exceeds it, and `--partition` splits until no partition
/// does, so the tool can never warn about a distortion the partitioner would
/// have accepted.
pub const DISTORTION_LIMIT: f64 = 0.02;

#[cfg(test)]
mod tests {
    use super::*;

    /// The limit has to name a believable distance, since the docs quote one.
    #[test]
    fn the_distortion_limit_is_about_23_degrees() {
        let at = |deg: f64| laea_radial_error(deg.to_radians()).abs();
        assert!(at(22.0) < DISTORTION_LIMIT, "22 deg: {}", at(22.0));
        assert!(at(24.0) > DISTORTION_LIMIT, "24 deg: {}", at(24.0));
        // and it understates, never overstates
        assert!(laea_radial_error(0.4) < 0.0);
        assert_eq!(laea_radial_error(0.0), 0.0);
    }

    #[test]
    fn center_maps_to_origin() {
        let p = Laea::new(19.5, 59.5);
        let (x, y) = p.forward(19.5, 59.5);
        assert!(x.abs() < 1e-6, "x={x}");
        assert!(y.abs() < 1e-6, "y={y}");
    }

    #[test]
    fn forward_inverse_roundtrip() {
        let p = Laea::new(19.5, 59.5);
        for &(lon, lat) in &[(20.0, 60.0), (8.0, 54.0), (30.0, 65.0), (12.3, 57.8)] {
            let (x, y) = p.forward(lon, lat);
            let (lon2, lat2) = p.inverse(x, y);
            assert!((lon - lon2).abs() < 1e-7, "lon {lon} -> {lon2}");
            assert!((lat - lat2).abs() < 1e-7, "lat {lat} -> {lat2}");
        }
    }

    #[test]
    fn planar_distance_matches_great_circle_locally() {
        // Two nearby Baltic points: planar LAEA distance should track the
        // great-circle distance to well under 1% at this separation.
        let p = Laea::new(19.5, 59.5);
        let (ax, ay) = p.forward(18.0, 58.0);
        let (bx, by) = p.forward(18.1, 58.05);
        let planar = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
        let gc = haversine_m(18.0, 58.0, 18.1, 58.05);
        let rel = (planar - gc).abs() / gc;
        assert!(rel < 0.01, "planar={planar} gc={gc} rel={rel}");
    }

    #[test]
    fn haversine_known_distance() {
        // Helsinki to Stockholm is ~395 km.
        let d = haversine_m(24.9384, 60.1699, 18.0686, 59.3293);
        assert!((390_000.0..402_000.0).contains(&d), "d={d}");
    }

    #[test]
    fn chord_distance_matches_haversine_globally() {
        // The unit-sphere chord distance must equal the great-circle distance
        // even over long, cross-globe separations where a planar projection
        // would be far off. Helsinki to Stockholm (short) and Oslo to Sydney
        // (a third of the way around the Earth).
        for &(alon, alat, blon, blat) in &[
            (24.9384, 60.1699, 18.0686, 59.3293),
            (10.7522, 59.9139, 151.2093, -33.8688),
        ] {
            let a = unit_sphere(alon, alat);
            let b = unit_sphere(blon, blat);
            let chord2 = (0..3).map(|i| (a[i] - b[i]).powi(2)).sum();
            let d = chord2_to_m(chord2);
            let gc = haversine_m(alon, alat, blon, blat);
            let rel = (d - gc).abs() / gc;
            assert!(rel < 1e-9, "chord={d} gc={gc} rel={rel}");
        }
    }
}
