//! Longitude arcs and the spherical center of a point set.
//!
//! Both exist because longitude wraps and latitude does not. A minimum and a
//! maximum describe a latitude band correctly and a longitude span only by
//! luck: a set of points either side of the antimeridian has longitudes near
//! -180 and near 180, so `min`/`max` reports the whole globe for something a
//! few degrees wide.
//!
//! [`covering_arc`] instead finds the widest longitude gap nothing occupies and
//! reports everything else, which is the smallest arc that actually contains
//! the input. [`spherical_center`] sidesteps the question entirely by averaging
//! unit vectors in three dimensions, so it is correct across the seam and at
//! the poles, where no averaging of angles is.

use crate::geo::unit_sphere;

/// A gap this narrow counts as no gap at all: the input wraps the globe, and
/// its longitude extent is the whole range rather than an arc that happens to
/// stop a fraction of a degree short of where it started.
pub const CIRCUMPOLAR_GAP_DEG: f64 = 1.0;

/// Longitude normalized to `0..360`, the frame the arc arithmetic works in.
pub fn norm360(lon: f64) -> f64 {
    let x = lon % 360.0;
    if x < 0.0 {
        x + 360.0
    } else {
        x
    }
}

/// Back from `0..360` to the `-180..180` the rest of seastamp uses.
pub fn to180(lon360: f64) -> f64 {
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
pub fn edge_interval(lon_a: f64, lon_b: f64) -> (f64, f64) {
    let (a, b) = (norm360(lon_a), norm360(lon_b));
    let east = norm360(b - a); // degrees travelled going east from a to b
    if east <= 180.0 {
        (a, east)
    } else {
        (b, 360.0 - east)
    }
}

/// Push one edge's longitude interval onto `intervals`, splitting it when it
/// runs past the seam so the sweep in [`covering_arc`] can stay on a plain
/// sorted list. A single point is an edge of zero length.
pub fn push_interval(intervals: &mut Vec<(f64, f64)>, lon_a: f64, lon_b: f64) {
    let (start, len) = edge_interval(lon_a, lon_b);
    if start + len > 360.0 {
        intervals.push((start, 360.0));
        intervals.push((0.0, start + len - 360.0));
    } else {
        intervals.push((start, start + len));
    }
}

/// Smallest longitude arc covering every interval, as
/// `(min_lon, max_lon, crosses_antimeridian)` in degrees.
///
/// The arc is the complement of the widest gap left uncovered. A fully covered
/// circle, or one whose widest gap is under [`CIRCUMPOLAR_GAP_DEG`], reports
/// the whole -180 to 180 range. When the arc crosses the antimeridian the
/// returned `min_lon` is greater than `max_lon`, which is what the flag says.
pub fn covering_arc(intervals: &mut [(f64, f64)]) -> Option<(f64, f64, bool)> {
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

/// The smallest longitude arc covering a set of points, each contributing a
/// zero-length interval.
pub fn points_arc(lons: impl IntoIterator<Item = f64>) -> Option<(f64, f64, bool)> {
    let mut iv: Vec<(f64, f64)> = Vec::new();
    for lon in lons {
        if lon.is_finite() {
            push_interval(&mut iv, lon, lon);
        }
    }
    covering_arc(&mut iv)
}

/// The mean direction of a set of lon/lat points, with the length of the mean
/// vector as a measure of how tightly they cluster.
///
/// Returns `(lon, lat, resultant)`. The resultant runs from 1 (every point in
/// one place) down to 0 (points spread so evenly over the globe that they have
/// no mean direction at all), which is the only honest signal that no single
/// projection center can serve the input. `None` when there is nothing to
/// average, or when the points cancel so completely that even the direction is
/// meaningless.
///
/// Averaging in three dimensions rather than in degrees is what makes this
/// correct across the antimeridian and at the poles: a ring of points around
/// the North Pole averages to the pole, as it should, where averaging their
/// longitudes would give an arbitrary answer.
pub fn spherical_center(pts: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let (mut sx, mut sy, mut sz, mut n) = (0.0, 0.0, 0.0, 0usize);
    for &(lon, lat) in pts {
        if !lon.is_finite() || !lat.is_finite() {
            continue;
        }
        let [x, y, z] = unit_sphere(lon, lat);
        sx += x;
        sy += y;
        sz += z;
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let n = n as f64;
    let (mx, my, mz) = (sx / n, sy / n, sz / n);
    let r = (mx * mx + my * my + mz * mz).sqrt();
    if r < 1e-12 {
        return None; // perfectly cancelling: there is no mean direction
    }
    let lat = (mz / r).asin().to_degrees();
    let lon = my.atan2(mx).to_degrees();
    Some((lon, lat, r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_takes_the_short_arc() {
        assert_eq!(edge_interval(10.0, 40.0), (10.0, 30.0));
        assert_eq!(edge_interval(40.0, 10.0), (10.0, 30.0));
        // across the seam: 170 E to 170 W is 20 degrees, not 340
        assert_eq!(edge_interval(170.0, -170.0), (170.0, 20.0));
    }

    #[test]
    fn norm360_and_back() {
        assert_eq!(norm360(-180.0), 180.0);
        assert_eq!(norm360(-90.0), 270.0);
        assert_eq!(to180(270.0), -90.0);
        assert_eq!(to180(180.0), 180.0);
    }

    #[test]
    fn simple_box_extent() {
        let mut iv = vec![(10.0, 20.0), (20.0, 30.0)];
        assert_eq!(covering_arc(&mut iv), Some((10.0, 30.0, false)));
    }

    #[test]
    fn near_full_circle_is_circumpolar() {
        let mut iv = vec![(0.0, 359.5)];
        assert_eq!(covering_arc(&mut iv), Some((-180.0, 180.0, false)));
    }

    /// Points either side of the antimeridian describe a narrow arc, not the
    /// whole globe, and are flagged as crossing.
    #[test]
    fn points_across_the_seam() {
        let (west, east, crosses) = points_arc([176.0, 179.0, -179.0, -176.0]).unwrap();
        assert_eq!((west, east), (176.0, -176.0));
        assert!(crosses);
    }

    /// A ring of points around the North Pole averages to the pole. No average
    /// of their longitudes could say anything useful.
    #[test]
    fn ring_around_the_pole_centers_on_it() {
        let pts: Vec<(f64, f64)> = (-180..180).step_by(10).map(|l| (l as f64, 75.0)).collect();
        let (_, lat, r) = spherical_center(&pts).unwrap();
        assert!((lat - 90.0).abs() < 1e-6, "lat was {lat}");
        // cos(15 degrees): tightly clustered in direction, despite spanning
        // every longitude.
        assert!((r - 15f64.to_radians().cos()).abs() < 1e-6, "r was {r}");
    }

    /// A globally spread grid: the unit vectors cancel to nothing, so there is
    /// no mean direction at all. That is the extreme of the case `auto` has to
    /// recognize, and `None` is the honest answer.
    #[test]
    fn a_symmetric_global_grid_has_no_direction() {
        let mut pts = Vec::new();
        for lon in (-180..180).step_by(30) {
            for lat in [-60, -30, 0, 30, 60] {
                pts.push((lon as f64, lat as f64));
            }
        }
        assert!(spherical_center(&pts).is_none(), "a symmetric grid cancels");
    }

    /// The realistic version: spread over the globe but not perfectly balanced.
    /// A direction exists, and the resultant is low enough that `auto` reports
    /// it cannot help.
    #[test]
    fn globally_spread_points_barely_cluster() {
        let mut pts = Vec::new();
        for lon in (-180..180).step_by(30) {
            for lat in [-60, -30, 0, 30, 60] {
                pts.push((lon as f64, lat as f64));
            }
        }
        pts.truncate(pts.len() - 7); // break the symmetry, as real data would
        let (_, _, r) = spherical_center(&pts).unwrap();
        assert!(r < 0.5, "a globally spread set should barely cluster: r = {r}");
    }
}
