//! Pure-Rust geometry: no PROJ / GDAL, so consuming projects need no extra
//! system libraries. A spherical LAEA projection (for planar distances in a
//! region), a great-circle distance, and the vector helpers (point to segment,
//! point in polygon, nearest feature) shared by the coast, sea, and place
//! modules.

pub mod arc;
pub mod projection;
pub mod vector;

pub use projection::{chord2_to_m, haversine_m, unit_sphere, Laea};
