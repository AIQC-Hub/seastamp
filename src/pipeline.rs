//! The one pipeline every module runs.
//!
//! 1. Read the input table (done by the caller, passed in as a `DataFrame`).
//! 2. Reduce it to unique rounded `(lon, lat)` locations.
//! 3. Enrich those unique locations in parallel (rayon).
//! 4. Join the results back onto every input row and write the table out.
//!
//! A module only implements [`Enricher`]: it declares the columns it appends and
//! computes their values for one location. Everything else lives here, so all
//! four modules share the same de-duplication, parallelism, and join.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

use polars::prelude::*;
use rayon::prelude::*;

use crate::cli::Format;
use crate::config::Settings;
use crate::io;

/// One appended column's name and type.
pub struct OutputSpec {
    pub name: String,
    pub kind: OutputKind,
}

#[derive(Clone, Copy)]
pub enum OutputKind {
    Float,
    Text,
    Bool,
}

/// A single computed value, matching an [`OutputSpec`] by position.
pub enum Value {
    Float(f64),
    Text(Option<String>),
    Bool(Option<bool>),
}

/// A module's per-location logic. `Sync` so locations run in parallel.
pub trait Enricher: Sync {
    /// The columns this enricher appends, in order.
    fn outputs(&self) -> Vec<OutputSpec>;
    /// Compute the values for one unique location. The returned vector must line
    /// up with `outputs()`.
    fn enrich(&self, lon: f64, lat: f64) -> Vec<Value>;

    /// Whether locations may enrich on many threads at once. Default `true`.
    ///
    /// A module returns `false` when its backing library cannot be entered from
    /// more than one thread, whatever the locking. `depth` does: a serial HDF5
    /// build crashes when read from several threads even with every call under a
    /// mutex, because the library keeps state that assumes a single thread of
    /// execution. Serializing costs such a module nothing, since a lock would
    /// have made the work sequential anyway.
    fn parallel(&self) -> bool {
        true
    }

    /// The center of the planar projection this module measures in, as
    /// `(lon, lat)` degrees, or `None` for a module whose results do not depend
    /// on one (`depth` reads a grid, `nearest` works on the sphere).
    ///
    /// [`run_module`] uses it to warn when the input sits far enough from the
    /// center that planar distances are visibly distorted, which is otherwise
    /// silent: the numbers look plausible and are simply wrong.
    fn projection_center(&self) -> Option<(f64, f64)> {
        None
    }
}

/// Great-circle distance in meters between two lon/lat points, and the relative
/// error a planar LAEA measurement picks up at that separation from its center.
///
/// For an azimuthal equal-area projection the radial scale at angular distance
/// `c` is `1/k'` with `k' = sqrt(2 / (1 + cos c))`, so a length measured radially
/// comes out scaled by `sqrt((1 + cos c) / 2)`. Returned as a signed fraction:
/// negative means the projection understates the true distance.
fn laea_radial_error(angle_rad: f64) -> f64 {
    ((1.0 + angle_rad.cos()) / 2.0).sqrt() - 1.0
}

/// Warn when the input lies far enough from the projection center for planar
/// distances to be noticeably off. Only fires past a threshold, so a run whose
/// region actually matches its data stays quiet.
fn warn_if_far_from_center(uniq: &[(f64, f64)], center: (f64, f64)) {
    const WARN_ABOVE: f64 = 0.02; // 2% understatement, about 23 degrees away

    let (clon, clat) = center;
    let mut worst_m = 0.0_f64;
    for &(lon, lat) in uniq {
        if !lon.is_finite() || !lat.is_finite() {
            continue;
        }
        let d = crate::geo::haversine_m(clon, clat, lon, lat);
        if d > worst_m {
            worst_m = d;
        }
    }
    if worst_m <= 0.0 {
        return;
    }
    let err = laea_radial_error(worst_m / crate::geo::projection::MEAN_RADIUS_M);
    if err.abs() < WARN_ABOVE {
        return;
    }
    eprintln!(
        "[seastamp] warning: the farthest input point is {:.0} km from the projection center \
         ({clon:.1}, {clat:.1}), where planar distances are off by roughly {:.0}%.",
        worst_m / 1000.0,
        err.abs() * 100.0
    );
    eprintln!(
        "[seastamp] warning: pass --region, or --proj-lon0 / --proj-lat0, centered on your data."
    );
    // Only worth saying when the center looks like the untouched global default,
    // otherwise it contradicts the region the user actually chose.
    if clon.abs() < 1e-9 && clat.abs() < 1e-9 {
        eprintln!(
            "[seastamp] warning: no region was given, so the projection defaults to the center \
             of the whole globe, (0, 0)."
        );
    }
}

/// Extract a column as `f64`, mapping nulls to NaN. Casts from any numeric dtype.
fn column_f64(df: &DataFrame, name: &str) -> Result<Vec<f64>, Box<dyn Error>> {
    let s = df
        .column(name)
        .map_err(|_| format!("input has no column '{name}'"))?;
    let ca = s.cast(&DataType::Float64)?;
    let f = ca.f64()?;
    Ok(f.into_iter().map(|o| o.unwrap_or(f64::NAN)).collect())
}

/// Run the shared pipeline for one enricher and write the result.
pub fn run_module(
    enr: &dyn Enricher,
    df: DataFrame,
    s: &Settings,
    out_path: &Path,
    out_fmt: Format,
) -> Result<(), Box<dyn Error>> {
    if let Some(n) = s.threads {
        // build_global can only succeed once per process; ignore if already set.
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
    }

    let n = df.height();
    let lon = column_f64(&df, &s.lon_col)?;
    let lat = column_f64(&df, &s.lat_col)?;

    // Output columns already present in the input are an error by default,
    // and are replaced in place (keeping their position) with --overwrite.
    // Checked before enrichment so a clash fails fast.
    let specs = enr.outputs();
    let clashes: Vec<&str> = specs
        .iter()
        .map(|sp| sp.name.as_str())
        .filter(|name| df.column(name).is_ok())
        .collect();
    if !clashes.is_empty() && !s.overwrite {
        return Err(format!(
            "the input already has output column(s) '{}'; pass --overwrite to replace them",
            clashes.join("', '")
        )
        .into());
    }

    let scale = 10f64.powi(s.decimals as i32);
    let round = |v: f64| (v * scale).round() / scale;
    let key_of = |rlon: f64, rlat: f64| ((rlon * scale).round() as i64, (rlat * scale).round() as i64);

    // Reduce to unique rounded locations; remember each row's key for the join.
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    let mut uniq: Vec<(f64, f64)> = Vec::new();
    let mut row_key: Vec<Option<(i64, i64)>> = Vec::with_capacity(n);
    for i in 0..n {
        let (lo, la) = (lon[i], lat[i]);
        if lo.is_nan() || la.is_nan() {
            row_key.push(None);
            continue;
        }
        let (rlo, rla) = (round(lo), round(la));
        let k = key_of(rlo, rla);
        if !index.contains_key(&k) {
            index.insert(k, uniq.len());
            uniq.push((rlo, rla));
        }
        row_key.push(Some(k));
    }

    // Warn before enriching, so the advice is visible above the results rather
    // than buried after a long run.
    if let Some(c) = enr.projection_center() {
        warn_if_far_from_center(&uniq, c);
    }

    // Enrich unique locations, in parallel unless the module forbids it (see
    // Enricher::parallel). The serial path stays on this thread throughout, which
    // is what a module backed by a single-threaded C library needs.
    let results: Vec<Vec<Value>> = if enr.parallel() {
        uniq.par_iter().map(|&(lo, la)| enr.enrich(lo, la)).collect()
    } else {
        uniq.iter().map(|&(lo, la)| enr.enrich(lo, la)).collect()
    };

    // Expand back to one value per input row and append the columns.
    let mut new_cols: Vec<Series> = Vec::with_capacity(specs.len());
    for (j, spec) in specs.iter().enumerate() {
        match spec.kind {
            OutputKind::Float => {
                let mut col = Vec::with_capacity(n);
                for rk in &row_key {
                    let v = rk
                        .and_then(|k| index.get(&k))
                        .map(|&idx| match &results[idx][j] {
                            Value::Float(f) => *f,
                            _ => f64::NAN,
                        })
                        .unwrap_or(f64::NAN);
                    col.push(v);
                }
                new_cols.push(Series::new(spec.name.as_str().into(), col));
            }
            OutputKind::Text => {
                let mut col: Vec<Option<String>> = Vec::with_capacity(n);
                for rk in &row_key {
                    let v = rk.and_then(|k| index.get(&k)).and_then(|&idx| {
                        match &results[idx][j] {
                            Value::Text(t) => t.clone(),
                            _ => None,
                        }
                    });
                    col.push(v);
                }
                new_cols.push(Series::new(spec.name.as_str().into(), col));
            }
            OutputKind::Bool => {
                let mut col: Vec<Option<bool>> = Vec::with_capacity(n);
                for rk in &row_key {
                    let v = rk.and_then(|k| index.get(&k)).and_then(|&idx| {
                        match &results[idx][j] {
                            Value::Bool(b) => *b,
                            _ => None,
                        }
                    });
                    col.push(v);
                }
                new_cols.push(Series::new(spec.name.as_str().into(), col));
            }
        }
    }

    let out = if s.overwrite {
        // `with_column` replaces a same-named column in place and appends the
        // rest, so untouched input columns keep their order.
        let mut out = df;
        for col in new_cols {
            out.with_column(col)?;
        }
        out
    } else {
        df.hstack(&new_cols)?
    };
    eprintln!(
        "[seastamp] {} rows, {} unique locations -> {}",
        n,
        uniq.len(),
        out_path.display()
    );
    io::write_frame(out, out_path, out_fmt)?;
    Ok(())
}
