//! Bathymetric depth from a GEBCO gridded NetCDF file.
//!
//! GEBCO is a regular lon/lat grid, so no nearest-neighbor search (the R code's
//! `nn2`) is needed. We read the `lon`/`lat` axes once to learn each axis origin
//! and spacing, then map every point straight to the enclosing cell by
//! arithmetic. That is O(1) per point and exact to the grid. The `netcdf` crate
//! (linking HDF5, as ctddump already does) reads a single `elevation` cell per
//! location, so the whole grid never needs to be resident.
//!
//! Unlike the other modules this one enriches on a single thread
//! ([`Enricher::parallel`] returns `false`). HDF5 is commonly built serial, and a
//! serial build cannot be entered from more than one thread even when every call
//! is under a mutex: it keeps state that assumes one thread of execution, and
//! reading a grid from rayon workers crashed (SIGSEGV in release, an error-stack
//! assertion in debug). Nothing is lost, because the reads were already funnelled
//! through one mutex and so never ran concurrently anyway.
//!
//! Sign convention: GEBCO elevation is negative below sea level. By default the
//! output reports the elevation as stored (negative under water); `--positive`
//! flips the sign so depth reads positive under water (and land reads negative).
//!
//! Caveats: only nearest-cell sampling is done (no bilinear interpolation), the
//! variable is assumed to be `elevation` with dimension order `(lat, lon)`, and
//! CF packing (`scale_factor` / `add_offset`) is not applied, matching how GEBCO
//! stores elevation directly in meters.

use std::error::Error;
use std::path::Path;
use std::sync::Mutex;

use crate::cli::DepthArgs;
use crate::config::{resolve, Settings};
use crate::pipeline::{run_module, Enricher, OutputKind, OutputSpec, Value};

/// Regular-grid geometry of one axis: the coordinate of index 0 (a cell center),
/// the spacing between adjacent centers, and the number of cells. Nearest-cell
/// index is then pure arithmetic. `step` may be negative for a descending axis.
struct Axis {
    origin: f64,
    step: f64,
    len: usize,
}

impl Axis {
    fn from_coords(coords: &[f64]) -> Result<Self, Box<dyn Error>> {
        if coords.len() < 2 {
            return Err("axis needs at least two coordinates".into());
        }
        let origin = coords[0];
        let step = (coords[coords.len() - 1] - coords[0]) / (coords.len() - 1) as f64;
        Ok(Axis { origin, step, len: coords.len() })
    }

    /// Nearest cell index for a query coordinate, or `None` if it falls more than
    /// half a cell outside the axis range.
    fn index(&self, q: f64) -> Option<usize> {
        if self.step == 0.0 {
            return None;
        }
        let f = (q - self.origin) / self.step; // fractional index
        if f < -0.5 || f > self.len as f64 - 0.5 {
            return None;
        }
        let i = (f.round() as isize).clamp(0, self.len as isize - 1);
        Some(i as usize)
    }
}

/// netcdf-c probes each variable for optional attributes (such as a `_FillValue`
/// a complete grid never defines) while reading metadata. Each miss makes HDF5's
/// default handler dump its error stack to stderr, though the miss is handled
/// gracefully. This disables that automatic printing; real errors still surface
/// through the `Result` return values.
///
/// HDF5's auto-print setting is per thread on a thread-safe build, so this runs
/// on every thread that touches NetCDF rather than once per process. The
/// `thread_local` guard makes the FFI call at most once per thread. It is public
/// so code that writes NetCDF before opening an enricher (for example a test that
/// builds a grid) can silence it too.
///
/// Safety: this reaches past the `netcdf` crate straight into HDF5, so it is not
/// covered by the global lock that crate holds for every netcdf-c call. Call it
/// only from a thread that is entitled to be inside HDF5, and with no other
/// thread inside NetCDF. Both callers here satisfy that: enrichment for this
/// module is single-threaded, and [`DepthEnricher::enrich`] additionally holds
/// the file mutex across the call.
pub fn silence_hdf5_diagnostics() {
    thread_local! {
        static SILENCED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    SILENCED.with(|done| {
        if done.get() {
            return;
        }
        // H5Eset_auto2(hid_t estack_id, H5E_auto2_t func, void *client_data):
        // H5E_DEFAULT is 0 and a null callback turns off stack printing. hid_t is
        // 64-bit in HDF5 >= 1.10, which the netcdf crate links.
        extern "C" {
            fn H5Eset_auto2(
                estack_id: i64,
                func: *const std::ffi::c_void,
                client_data: *mut std::ffi::c_void,
            ) -> i32;
        }
        unsafe {
            H5Eset_auto2(0, std::ptr::null(), std::ptr::null_mut());
        }
        done.set(true);
    });
}

/// Wrap a longitude into `[-180, 180)` so `0..360` inputs also index correctly.
fn normalize_lon(lon: f64) -> f64 {
    if !lon.is_finite() {
        return lon;
    }
    ((lon + 180.0).rem_euclid(360.0)) - 180.0
}

pub struct DepthEnricher {
    // The system HDF5 (serial build) is not guaranteed thread-safe, so reads are
    // serialized through this mutex even though locations enrich on rayon workers.
    // A single-cell read is cheap and the location set is already de-duplicated,
    // so serializing it is not a bottleneck.
    file: Mutex<netcdf::File>,
    var_name: String,
    lat: Axis,
    lon: Axis,
    column: String,
    positive: bool,
}

impl DepthEnricher {
    /// Open a GEBCO NetCDF file and read its `lat`/`lon` axes. Fails if the file
    /// is missing the `lat`, `lon`, or `elevation` variables.
    pub fn open(path: &Path, column: String, positive: bool) -> Result<Self, Box<dyn Error>> {
        silence_hdf5_diagnostics();
        let file = netcdf::open(path)
            .map_err(|e| format!("cannot open GEBCO file {}: {e}", path.display()))?;

        let lat_coords: Vec<f64> = file
            .variable("lat")
            .ok_or("GEBCO file has no 'lat' variable")?
            .get_values(..)
            .map_err(|e| format!("reading 'lat': {e}"))?;
        let lon_coords: Vec<f64> = file
            .variable("lon")
            .ok_or("GEBCO file has no 'lon' variable")?
            .get_values(..)
            .map_err(|e| format!("reading 'lon': {e}"))?;

        if file.variable("elevation").is_none() {
            return Err("GEBCO file has no 'elevation' variable".into());
        }

        let lat = Axis::from_coords(&lat_coords)?;
        let lon = Axis::from_coords(&lon_coords)?;

        Ok(Self {
            file: Mutex::new(file),
            var_name: "elevation".to_string(),
            lat,
            lon,
            column,
            positive,
        })
    }
}

impl Enricher for DepthEnricher {
    /// HDF5 must be entered from one thread only; see the module header.
    fn parallel(&self) -> bool {
        false
    }

    fn outputs(&self) -> Vec<OutputSpec> {
        Vec::from([OutputSpec {
            name: self.column.clone(),
            kind: OutputKind::Float,
        }])
    }

    fn enrich(&self, lon: f64, lat: f64) -> Vec<Value> {
        let value = match (self.lat.index(lat), self.lon.index(normalize_lon(lon))) {
            (Some(i), Some(j)) => {
                let file = self.file.lock().expect("depth file mutex poisoned");
                // Quiet HDF5 before touching NetCDF. Kept under the lock because
                // it calls HDF5 directly, bypassing the netcdf crate's own lock.
                silence_hdf5_diagnostics();
                let var = file
                    .variable(&self.var_name)
                    .expect("elevation presence checked in open");
                match var.get_value::<f64, _>([i, j]) {
                    Ok(v) if self.positive => -v,
                    Ok(v) => v,
                    Err(_) => f64::NAN,
                }
            }
            _ => f64::NAN,
        };
        Vec::from([Value::Float(value)])
    }
}

pub fn run(args: DepthArgs) -> Result<(), Box<dyn Error>> {
    // Depth is a grid lookup, so it needs no region box or projection.
    let s: Settings = resolve(&args.common, None)?;
    let data = args
        .data
        .ok_or("depth requires --data <GEBCO NetCDF file>")?;
    let df = crate::io::read_frame(&args.common.input, args.common.in_format)?;
    let out_path = args
        .common
        .output
        .clone()
        .unwrap_or_else(|| super::default_output(&args.common.input, "depth", args.common.in_format));

    let enr = DepthEnricher::open(&data, args.column, args.positive)?;
    run_module(&enr, df, &s, &out_path, args.common.out_format)
}
