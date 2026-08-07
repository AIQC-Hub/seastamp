//! Splitting a point set into pieces each of which one LAEA projection can
//! serve, for `--partition`.
//!
//! A single projection center is only accurate near itself: past
//! [`DISTORTION_LIMIT`] the planar distances `coast`, `sea`, and `place` report
//! are visibly short. `--region auto` picks the best single center it can and
//! warns when the data is too spread for any center to work. This module is the
//! answer to that warning: split until every piece is inside the limit, measure
//! each piece in its own projection, and join the results back together.
//!
//! **The split is driven by the error bound, not by a cell size or a count.**
//! There is no `k` to choose. [`partition`] bisects a cluster whenever its worst
//! point exceeds the tolerance and stops as soon as it does not, so the number
//! of partitions is whatever the data needs and the guarantee ("every distance
//! is within the tolerance of its true value") holds by construction. A fixed
//! scheme cannot promise that: partitioning by IHO area was measured at 2.5 to
//! 6.7% out for the ocean-sized areas, since those areas are as big as the whole
//! globe was.
//!
//! Bisection is by farthest pair, which is deterministic: no seed, no random
//! restarts, and the same input always yields the same partitions. That matters
//! more than an optimal split would, because the output is a data product.

use crate::config::{auto_bbox, BBox};
use crate::geo::arc::spherical_center;
use crate::geo::projection::{laea_error_at, DISTORTION_LIMIT};
use crate::geo::haversine_m;

/// One piece of the input: which locations belong to it, the box that crops its
/// reference data, and the projection its distances are measured in.
#[derive(Debug, Clone)]
pub struct Partition {
    /// Indices into the location slice passed to [`partition`].
    pub members: Vec<usize>,
    /// Crop box for this partition's reference data, padded like `auto`'s.
    pub bbox: BBox,
    /// LAEA center, the mean direction of this partition's own points.
    pub center: (f64, f64),
    /// Worst planar-distance error over this partition, as a fraction. Always
    /// at or under the tolerance [`partition`] was given, except for the
    /// degenerate single-location partition, which is exact.
    pub distortion: f64,
}

/// Split `pts` into partitions no worse than `tolerance`, in fractional
/// distance error. Non-finite coordinates are dropped, so a returned
/// `members` never indexes one.
///
/// Returns an empty vector when nothing is usable, which the caller should treat
/// the way `auto` treats a table of nulls.
pub fn partition(pts: &[(f64, f64)], tolerance: f64) -> Vec<Partition> {
    let usable: Vec<usize> = (0..pts.len())
        .filter(|&i| pts[i].0.is_finite() && pts[i].1.is_finite())
        .collect();
    if usable.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<Partition> = Vec::new();
    let mut pending = vec![usable];
    while let Some(members) = pending.pop() {
        match settle(pts, members, tolerance) {
            Ok(p) => out.push(p),
            Err(members) => {
                let (a, b) = bisect(pts, &members);
                pending.push(a);
                pending.push(b);
            }
        }
    }
    out = coalesce(pts, out, tolerance);

    // Stack order is already deterministic; sorting only makes the summary the
    // command prints read west to east.
    out.sort_by(|a, b| {
        (a.center.0, a.center.1)
            .partial_cmp(&(b.center.0, b.center.1))
            .expect("centers are finite")
    });
    out
}

/// The center a set of locations would take and the worst error it would then
/// suffer. `None` when the unit vectors cancel exactly, leaving no direction to
/// center on.
fn evaluate(coords: &[(f64, f64)]) -> Option<((f64, f64), f64)> {
    let (clon, clat, _) = spherical_center(coords)?;
    let worst_m = coords
        .iter()
        .map(|&(lo, la)| haversine_m(clon, clat, lo, la))
        .fold(0.0_f64, f64::max);
    Some(((clon, clat), laea_error_at(worst_m).abs()))
}

/// Accept `members` as a partition, or hand them back to be split.
///
/// A cluster is accepted when its worst point is inside the tolerance, and also
/// when it holds a single location, which is exact by definition and is what
/// stops the recursion. A cluster whose unit vectors cancel exactly has no mean
/// direction at all and is always split.
fn settle(pts: &[(f64, f64)], members: Vec<usize>, tolerance: f64) -> Result<Partition, Vec<usize>> {
    let coords: Vec<(f64, f64)> = members.iter().map(|&i| pts[i]).collect();
    let Some((center, distortion)) = evaluate(&coords) else {
        return Err(members); // perfectly cancelling: nothing to center on yet
    };
    if distortion > tolerance && members.len() > 1 {
        return Err(members);
    }
    let (bbox, _) = auto_bbox(&coords).expect("a non-empty finite set has a covering arc");
    Ok(Partition {
        members,
        bbox,
        center,
        distortion,
    })
}

/// Merge back any two partitions whose union is still inside the tolerance.
///
/// Bisection splits a cluster the moment one point falls outside, so it
/// routinely overshoots: a set 25 degrees across becomes two of 15 rather than
/// the one of 23 that would have served. On a globally spread grid that
/// overshoot cost a measured 88 partitions where 30-odd suffice, and every extra
/// partition is another crop of the reference data to hold in memory.
///
/// Greedy and exact: each round scores every admissible pair and merges the one
/// whose union comes out tightest, so a merge never spends accuracy it did not
/// have to. Pairs whose centers are more than two serviceable radii apart cannot
/// possibly unite inside the tolerance and are skipped without scoring, which is
/// what keeps this affordable at a hundred partitions.
fn coalesce(pts: &[(f64, f64)], mut parts: Vec<Partition>, tolerance: f64) -> Vec<Partition> {
    /// The best merge found so far in one round.
    struct Merge {
        distortion: f64,
        center: (f64, f64),
        members: Vec<usize>,
        /// The two partitions it would replace, `left < right`.
        left: usize,
        right: usize,
    }

    let reach = 2.0 * crate::geo::projection::laea_radius_m(tolerance);
    loop {
        let mut best: Option<Merge> = None;
        for left in 0..parts.len() {
            for right in (left + 1)..parts.len() {
                let (a, b) = (&parts[left], &parts[right]);
                if haversine_m(a.center.0, a.center.1, b.center.0, b.center.1) > reach {
                    continue;
                }
                let mut members: Vec<usize> = a.members.iter().chain(&b.members).copied().collect();
                // Keep member order stable so the result does not depend on
                // which index happened to come first out of the split.
                members.sort_unstable();
                let coords: Vec<(f64, f64)> = members.iter().map(|&k| pts[k]).collect();
                let Some((center, distortion)) = evaluate(&coords) else {
                    continue;
                };
                if distortion > tolerance {
                    continue;
                }
                if best.as_ref().is_none_or(|m| distortion < m.distortion) {
                    best = Some(Merge { distortion, center, members, left, right });
                }
            }
        }
        let Some(m) = best else {
            return parts;
        };
        let coords: Vec<(f64, f64)> = m.members.iter().map(|&k| pts[k]).collect();
        let (bbox, _) = auto_bbox(&coords).expect("a non-empty finite set has a covering arc");
        parts.swap_remove(m.right); // right > left, so removing it cannot move left
        parts[m.left] = Partition {
            members: m.members,
            bbox,
            center: m.center,
            distortion: m.distortion,
        };
    }
}

/// Split a cluster in two about its farthest pair: seed from the member
/// farthest from an arbitrary one, then from the member farthest from that, and
/// give every member to whichever seed is nearer.
///
/// Both halves are always non-empty, since each seed keeps itself, so the
/// recursion strictly shrinks and terminates. Ties go to the lower index, which
/// is what makes the result reproducible.
fn bisect(pts: &[(f64, f64)], members: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let farthest_from = |from: usize| -> usize {
        let (flon, flat) = pts[from];
        let mut best = (f64::NEG_INFINITY, from);
        for &i in members {
            let d = haversine_m(flon, flat, pts[i].0, pts[i].1);
            if d > best.0 {
                best = (d, i);
            }
        }
        best.1
    };
    let a = farthest_from(members[0]);
    let b = farthest_from(a);
    if a == b {
        // Every member sits in one place, so no pair is farther than any other.
        // `settle` would have accepted such a cluster, but splitting by index
        // keeps this function total rather than leaving a way to loop forever.
        let mid = members.len() / 2;
        return (members[..mid].to_vec(), members[mid..].to_vec());
    }

    let (mut left, mut right) = (Vec::new(), Vec::new());
    for &i in members {
        let da = haversine_m(pts[a].0, pts[a].1, pts[i].0, pts[i].1);
        let db = haversine_m(pts[b].0, pts[b].1, pts[i].0, pts[i].1);
        if da <= db {
            left.push(i);
        } else {
            right.push(i);
        }
    }
    (left, right)
}

/// For each location, the partition that owns it, ready for the pipeline's
/// scatter-gather join. Locations in no partition (the non-finite ones) get
/// `None`.
pub fn owners(parts: &[Partition], n: usize) -> Vec<Option<usize>> {
    let mut owner = vec![None; n];
    for (p, part) in parts.iter().enumerate() {
        for &i in &part.members {
            owner[i] = Some(p);
        }
    }
    owner
}

/// The worst distortion over every partition, which is what the run reports as
/// its accuracy. Zero for an empty set.
pub fn worst_distortion(parts: &[Partition]) -> f64 {
    parts.iter().map(|p| p.distortion).fold(0.0, f64::max)
}

/// The default tolerance: the same figure the pipeline warns above, so a
/// partitioned run can never trip its own warning.
pub const DEFAULT_TOLERANCE: f64 = DISTORTION_LIMIT;

#[cfg(test)]
mod tests {
    use super::*;

    /// A survey that one projection already serves is left whole: partitioning
    /// must not fragment data that never needed it.
    #[test]
    fn a_local_survey_stays_one_partition() {
        let pts = [(4.1, 60.4), (4.9, 60.9), (3.7, 60.1), (5.5, 61.2)];
        let parts = partition(&pts, DEFAULT_TOLERANCE);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].members.len(), 4);
        assert!(parts[0].distortion < 1e-3, "{}", parts[0].distortion);
        // and it earns the same box auto would have given it
        assert!((parts[0].bbox.min_lon - (3.7 - crate::config::AUTO_PAD_DEG)).abs() < 1e-9);
    }

    /// The case the feature exists for. A globally spread set is split until
    /// every piece is inside the limit, and the pieces still cover every point
    /// exactly once.
    #[test]
    fn a_global_set_splits_until_every_piece_is_accurate() {
        let mut pts = Vec::new();
        for lon in (-180..180).step_by(15) {
            for lat in (-60..75).step_by(15) {
                pts.push((lon as f64, lat as f64));
            }
        }
        let parts = partition(&pts, DEFAULT_TOLERANCE);
        assert!(parts.len() > 1, "a global grid needs splitting");
        for p in &parts {
            assert!(
                p.distortion <= DEFAULT_TOLERANCE,
                "partition of {} points is {:.3} out",
                p.members.len(),
                p.distortion
            );
        }
        // a partition of the input, in the mathematical sense: no point lost,
        // none counted twice
        let mut seen: Vec<usize> = parts.iter().flat_map(|p| p.members.iter().copied()).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..pts.len()).collect::<Vec<_>>());
    }

    /// The whole point of splitting by the error bound rather than by a fixed
    /// scheme: tighten the tolerance and you get more, smaller pieces.
    #[test]
    fn a_tighter_tolerance_buys_more_partitions() {
        let pts: Vec<(f64, f64)> = (-80..80).step_by(5).map(|l| (l as f64, 30.0)).collect();
        let loose = partition(&pts, 0.02).len();
        let tight = partition(&pts, 0.002).len();
        assert!(tight > loose, "loose {loose}, tight {tight}");
        for p in partition(&pts, 0.002) {
            assert!(p.distortion <= 0.002, "{}", p.distortion);
        }
    }

    /// Two runs of the same data must give the same answer, or the output is
    /// not a data product. This is why bisection is by farthest pair and not
    /// by k-means.
    #[test]
    fn partitioning_is_deterministic() {
        let pts: Vec<(f64, f64)> = (0..200)
            .map(|i| {
                let f = i as f64;
                ((f * 7.3) % 360.0 - 180.0, (f * 3.1) % 140.0 - 70.0)
            })
            .collect();
        let a = partition(&pts, DEFAULT_TOLERANCE);
        let b = partition(&pts, DEFAULT_TOLERANCE);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.members, y.members);
            assert_eq!(x.center, y.center);
        }
    }

    /// Antipodal points cancel to no mean direction, the case `spherical_center`
    /// returns `None` for. It must split rather than give up.
    #[test]
    fn antipodal_points_split_instead_of_cancelling() {
        let pts = [(0.0, 0.0), (180.0, 0.0)];
        let parts = partition(&pts, DEFAULT_TOLERANCE);
        assert_eq!(parts.len(), 2);
        for p in &parts {
            assert_eq!(p.members.len(), 1);
        }
    }

    /// Duplicated and non-finite coordinates are ordinary in real tables.
    /// Neither may hang the recursion or land in a partition.
    #[test]
    fn duplicates_and_nulls_are_handled() {
        let pts = [
            (5.0, 60.0),
            (5.0, 60.0),
            (f64::NAN, 60.0),
            (5.0, f64::NAN),
            (5.0, 60.0),
        ];
        let parts = partition(&pts, DEFAULT_TOLERANCE);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].members, vec![0, 1, 4]);
        assert_eq!(owners(&parts, pts.len()), vec![Some(0), Some(0), None, None, Some(0)]);
    }

    /// A table with nothing usable in it returns nothing, rather than an
    /// invented partition centered on (0, 0).
    #[test]
    fn an_unusable_table_gives_no_partitions() {
        let parts = partition(&[(f64::NAN, f64::NAN)], DEFAULT_TOLERANCE);
        assert!(parts.is_empty());
        assert_eq!(worst_distortion(&parts), 0.0);
    }

    /// A partition either side of the antimeridian keeps every longitude in its
    /// crop, exactly as `auto` does, and still centers on its own data.
    #[test]
    fn a_partition_across_the_dateline_keeps_its_center() {
        let pts = [(178.5, -17.5), (-179.2, -16.8), (179.9, -18.1)];
        let parts = partition(&pts, DEFAULT_TOLERANCE);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].center.0.abs() > 179.0, "{:?}", parts[0].center);
        assert_eq!((parts[0].bbox.min_lon, parts[0].bbox.max_lon), (-180.0, 180.0));
    }
}
