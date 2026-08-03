//! Vector-geometry helpers shared by the coast, sea, and place modules:
//! point-to-segment distance, an R-tree segment wrapper tagged with its source
//! feature, an even-odd point-in-polygon test, and [`PolygonIndex`], which
//! resolves a point to a polygon feature by containment with a nearest-boundary
//! fallback. All hand-rolled against the pure-Rust LAEA projection, so the
//! `geo` crate is not needed.

use rstar::{PointDistance, RTree, RTreeObject, AABB};

use crate::config::BBox;
use crate::geo::Laea;

/// Extra degrees added around the region box when cropping reference geometry,
/// so points near a region edge still see features lying just outside it.
pub const CROP_MARGIN_DEG: f64 = 5.0;

/// One polygon feature's rings in lon/lat degrees: outer ring first, then holes.
pub type Rings = Vec<Vec<(f64, f64)>>;

/// Squared distance from point `p` to segment `a`-`b`, all in the same plane.
pub fn point_seg_dist2(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let (cx, cy) = if len2 <= 0.0 {
        (ax, ay) // degenerate segment: a == b
    } else {
        let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
        (ax + t * dx, ay + t * dy)
    };
    let (ex, ey) = (px - cx, py - cy);
    ex * ex + ey * ey
}

/// Region box grown by `m` degrees on every side (used to crop reference data).
pub fn expand(b: &BBox, m: f64) -> BBox {
    BBox {
        min_lon: b.min_lon - m,
        max_lon: b.max_lon + m,
        min_lat: b.min_lat - m,
        max_lat: b.max_lat + m,
    }
}

/// One boundary segment in projected (LAEA) meters, tagged with the index of
/// the feature it belongs to.
pub struct TaggedSegment {
    pub ax: f64,
    pub ay: f64,
    pub bx: f64,
    pub by: f64,
    pub tag: usize,
}

impl RTreeObject for TaggedSegment {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(
            [self.ax.min(self.bx), self.ay.min(self.by)],
            [self.ax.max(self.bx), self.ay.max(self.by)],
        )
    }
}

impl PointDistance for TaggedSegment {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        point_seg_dist2(point[0], point[1], self.ax, self.ay, self.bx, self.by)
    }
}

/// Even-odd ray cast across every ring: `true` when the point lies inside the
/// polygon. Counting crossings over the outer ring and the holes together makes
/// points in a hole read as outside automatically.
pub fn point_in_rings(lon: f64, lat: f64, rings: &[Vec<(f64, f64)>]) -> bool {
    let mut inside = false;
    for ring in rings {
        let n = ring.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = ring[i];
            let (xj, yj) = ring[j];
            if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

/// Lon/lat bounding box of a feature's rings, or `None` for an empty feature.
fn rings_bbox(rings: &Rings) -> Option<BBox> {
    let mut b: Option<BBox> = None;
    for &(lon, lat) in rings.iter().flatten() {
        b = Some(match b {
            None => BBox { min_lon: lon, max_lon: lon, min_lat: lat, max_lat: lat },
            Some(b) => BBox {
                min_lon: b.min_lon.min(lon),
                max_lon: b.max_lon.max(lon),
                min_lat: b.min_lat.min(lat),
                max_lat: b.max_lat.max(lat),
            },
        });
    }
    b
}

/// A kept feature's lon/lat bounding box in the candidate R-tree.
struct PolyBBox {
    aabb: AABB<[f64; 2]>,
    idx: usize,
}

impl RTreeObject for PolyBBox {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.aabb
    }
}

/// A cropped set of polygon features answering "which feature is this point in,
/// or nearest to?". Containment is an even-odd test over candidates from a
/// bounding-box R-tree; points inside no feature fall back to the feature with
/// the nearest boundary segment by planar (LAEA) distance. `A` is whatever
/// per-feature attribute the module wants back (a name, a name/code pair).
pub struct PolygonIndex<A> {
    feats: Vec<(Rings, A)>,
    bbox_tree: RTree<PolyBBox>,
    seg_tree: RTree<TaggedSegment>,
    proj: Laea,
}

impl<A> PolygonIndex<A> {
    /// Build the index from features cropped to `region` grown by `margin_deg`:
    /// a feature is kept whole when its bounding box intersects the grown box,
    /// and dropped otherwise. Features are never clipped, so containment stays
    /// exact; only the nearest-boundary fallback can be off for points whose
    /// true feature lies entirely outside the grown box.
    pub fn build(feats: Vec<(Rings, A)>, region: BBox, margin_deg: f64, proj: Laea) -> Self {
        let crop = expand(&region, margin_deg);
        let kept: Vec<(Rings, A)> = feats
            .into_iter()
            .filter(|(rings, _)| match rings_bbox(rings) {
                Some(b) => {
                    b.max_lon >= crop.min_lon
                        && b.min_lon <= crop.max_lon
                        && b.max_lat >= crop.min_lat
                        && b.min_lat <= crop.max_lat
                }
                None => false,
            })
            .collect();

        let mut boxes = Vec::with_capacity(kept.len());
        let mut segs = Vec::new();
        for (idx, (rings, _)) in kept.iter().enumerate() {
            let b = rings_bbox(rings).expect("kept features have vertices");
            boxes.push(PolyBBox {
                aabb: AABB::from_corners([b.min_lon, b.min_lat], [b.max_lon, b.max_lat]),
                idx,
            });
            for ring in rings {
                for w in ring.windows(2) {
                    let (ax, ay) = proj.forward(w[0].0, w[0].1);
                    let (bx, by) = proj.forward(w[1].0, w[1].1);
                    segs.push(TaggedSegment { ax, ay, bx, by, tag: idx });
                }
            }
        }

        PolygonIndex {
            feats: kept,
            bbox_tree: RTree::bulk_load(boxes),
            seg_tree: RTree::bulk_load(segs),
            proj,
        }
    }

    /// The attribute of the feature containing the point, else of the feature
    /// with the nearest boundary, else `None` when the index is empty.
    pub fn locate(&self, lon: f64, lat: f64) -> Option<&A> {
        self.locate_with_dist(lon, lat).map(|(a, _)| a)
    }

    /// As [`Self::locate`], plus how far the point is from the feature: `0.0` when
    /// it is contained, otherwise the planar distance in meters to the nearest
    /// boundary segment, measured in the index's LAEA projection (the same basis
    /// `coast` uses).
    ///
    /// The distance is what tells a real containment apart from a nearest-feature
    /// fallback that reached a long way, which matters for an index covering only
    /// part of the globe: a point far outside the covered area still gets the
    /// nearest feature in it, however distant.
    pub fn locate_with_dist(&self, lon: f64, lat: f64) -> Option<(&A, f64)> {
        let q = AABB::from_point([lon, lat]);
        for cand in self.bbox_tree.locate_in_envelope_intersecting(q) {
            if point_in_rings(lon, lat, &self.feats[cand.idx].0) {
                return Some((&self.feats[cand.idx].1, 0.0));
            }
        }
        let (x, y) = self.proj.forward(lon, lat);
        self.seg_tree
            .nearest_neighbor([x, y])
            .map(|s| (&self.feats[s.tag].1, s.distance_2(&[x, y]).sqrt()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_odd_handles_holes() {
        let rings: Rings = vec![
            vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)],
            vec![(4.0, 4.0), (6.0, 4.0), (6.0, 6.0), (4.0, 6.0), (4.0, 4.0)],
        ];
        assert!(point_in_rings(2.0, 2.0, &rings)); // inside, off the hole
        assert!(!point_in_rings(5.0, 5.0, &rings)); // in the hole
        assert!(!point_in_rings(20.0, 5.0, &rings)); // outside
    }

    #[test]
    fn point_seg_dist2_clamps_to_endpoints() {
        // Perpendicular foot inside the segment.
        assert!((point_seg_dist2(0.0, 1.0, -1.0, 0.0, 1.0, 0.0) - 1.0).abs() < 1e-12);
        // Beyond endpoint b: distance to b, squared (2^2).
        assert!((point_seg_dist2(3.0, 0.0, -1.0, 0.0, 1.0, 0.0) - 4.0).abs() < 1e-12);
        // Degenerate a == b: distance to the point (1^2 + 1^2).
        assert!((point_seg_dist2(1.0, 1.0, 0.0, 0.0, 0.0, 0.0) - 2.0).abs() < 1e-12);
    }
}
