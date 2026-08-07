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
use crate::config::{BBox, Settings};
use crate::geo::partition::{partition, worst_distortion, Partition, DEFAULT_TOLERANCE};
use crate::geo::projection::{laea_error_at, DISTORTION_LIMIT};
use crate::geo::vector::crop_fraction;
use crate::geo::Laea;
use crate::io;

/// One appended column's name and type.
#[derive(Clone)]
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

/// Warn when the input lies far enough from the projection center for planar
/// distances to be noticeably off. Only fires past a threshold, so a run whose
/// region actually matches its data stays quiet.
///
/// The threshold is [`DISTORTION_LIMIT`], the same figure `--partition` splits
/// against, so the advice this prints is advice `--partition` can act on.
fn warn_if_far_from_center(uniq: &[(f64, f64)], center: (f64, f64)) {
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
    let err = laea_error_at(worst_m);
    if err.abs() < DISTORTION_LIMIT {
        return;
    }
    eprintln!(
        "[seastamp] warning: the farthest input point is {:.0} km from the projection center \
         ({clon:.1}, {clat:.1}), where planar distances are off by roughly {:.0}%.",
        worst_m / 1000.0,
        err.abs() * 100.0
    );
    eprintln!(
        "[seastamp] warning: pass --region, or --proj-lon0 / --proj-lat0, centered on your data, \
         or --partition to measure each area in its own projection."
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

/// The input's `(lon, lat)` pairs, in row order, nulls and non-numerics as NaN.
/// Used by `--region auto` to derive the region before the reference data is
/// opened, which is why it is separate from [`run_module`]'s own extraction.
pub fn locations(df: &DataFrame, s: &Settings) -> Result<Vec<(f64, f64)>, Box<dyn Error>> {
    let lon = column_f64(df, &s.lon_col)?;
    let lat = column_f64(df, &s.lat_col)?;
    Ok(lon.into_iter().zip(lat).collect())
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

/// The input reduced to unique rounded locations, with the mapping back to rows.
struct Reduced {
    /// Unique rounded `(lon, lat)`, the only coordinates an enricher ever sees.
    uniq: Vec<(f64, f64)>,
    index: HashMap<(i64, i64), usize>,
    /// Each input row's key, or `None` where the row has no usable location.
    row_key: Vec<Option<(i64, i64)>>,
}

/// Reduce the input to unique rounded locations. Rows without a usable
/// coordinate are remembered as `None` and get null outputs; every entry in
/// `uniq` is therefore finite, which the partitioner relies on.
fn reduce(df: &DataFrame, s: &Settings) -> Result<Reduced, Box<dyn Error>> {
    let n = df.height();
    let lon = column_f64(df, &s.lon_col)?;
    let lat = column_f64(df, &s.lat_col)?;

    let scale = 10f64.powi(s.decimals as i32);
    let round = |v: f64| (v * scale).round() / scale;
    let key_of =
        |rlon: f64, rlat: f64| ((rlon * scale).round() as i64, (rlat * scale).round() as i64);

    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    let mut uniq: Vec<(f64, f64)> = Vec::new();
    let mut row_key: Vec<Option<(i64, i64)>> = Vec::with_capacity(n);
    for i in 0..n {
        let (lo, la) = (lon[i], lat[i]);
        if !lo.is_finite() || !la.is_finite() {
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
    Ok(Reduced { uniq, index, row_key })
}

/// Refuse to clobber an existing column unless asked. Checked before enrichment
/// so a clash fails fast rather than after a long run.
fn check_clashes(df: &DataFrame, specs: &[OutputSpec], s: &Settings) -> Result<(), Box<dyn Error>> {
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
    Ok(())
}

/// Apply the thread cap, if one was given.
fn set_threads(s: &Settings) {
    if let Some(n) = s.threads {
        // build_global can only succeed once per process; ignore if already set.
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
    }
}

/// Enrich a slice of unique locations, in parallel unless the module forbids it
/// (see [`Enricher::parallel`]). The serial path stays on this thread
/// throughout, which is what a module backed by a single-threaded C library
/// needs.
fn enrich_all(enr: &dyn Enricher, locs: &[(f64, f64)]) -> Vec<Vec<Value>> {
    if enr.parallel() {
        locs.par_iter().map(|&(lo, la)| enr.enrich(lo, la)).collect()
    } else {
        locs.iter().map(|&(lo, la)| enr.enrich(lo, la)).collect()
    }
}

/// Run the shared pipeline for one enricher and write the result.
pub fn run_module(
    enr: &dyn Enricher,
    df: DataFrame,
    s: &Settings,
    out_path: &Path,
    out_fmt: Format,
) -> Result<(), Box<dyn Error>> {
    set_threads(s);
    let specs = enr.outputs();
    check_clashes(&df, &specs, s)?;
    let r = reduce(&df, s)?;

    // Warn before enriching, so the advice is visible above the results rather
    // than buried after a long run.
    if let Some(c) = enr.projection_center() {
        warn_if_far_from_center(&r.uniq, c);
    }

    let results = enrich_all(enr, &r.uniq);
    finish(df, specs, &results, &r, s, out_path, out_fmt)
}

/// Expand per-location results back to one value per input row, append the
/// columns, and write the table.
fn finish(
    df: DataFrame,
    specs: Vec<OutputSpec>,
    results: &[Vec<Value>],
    r: &Reduced,
    s: &Settings,
    out_path: &Path,
    out_fmt: Format,
) -> Result<(), Box<dyn Error>> {
    let n = df.height();
    let (index, row_key) = (&r.index, &r.row_key);
    let mut new_cols: Vec<Series> = Vec::with_capacity(specs.len());
    for (j, spec) in specs.iter().enumerate() {
        match spec.kind {
            OutputKind::Float => {
                let mut col = Vec::with_capacity(n);
                for &rk in row_key {
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
                for &rk in row_key {
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
                for &rk in row_key {
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
        r.uniq.len(),
        out_path.display()
    );
    io::write_frame(out, out_path, out_fmt)?;
    Ok(())
}

/// How much reference data, as a fraction of the globe's surface, to hold in
/// memory at once when `--partition` splits a run.
///
/// Partitions are cropped generously (their own extent plus about 10 degrees),
/// so their crops overlap: a fully global input measured 4.35 globes of crop
/// across 64 partitions, which is that much shoreline held at once if every
/// partition is built together. Batching to a budget trades a second read of
/// the reference file for a bound on memory, and the common regional run stays
/// under the budget and reads once regardless.
const CROP_BUDGET_GLOBES: f64 = 1.5;

/// Group partitions into batches whose crops fit the budget, preserving order.
/// A single partition over budget forms a batch of its own: the budget bounds
/// what batching can help with, not what a run is allowed to do.
fn batches(parts: &[Partition]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut acc) = (0, 0.0);
    for (i, p) in parts.iter().enumerate() {
        let f = crop_fraction(&p.bbox);
        if i > start && acc + f > CROP_BUDGET_GLOBES {
            out.push(start..i);
            (start, acc) = (i, 0.0);
        }
        acc += f;
    }
    out.push(start..parts.len());
    out
}

/// Build the enrichers for one batch of partitions, given each partition's crop
/// box and projection. A module implements this by reading its reference data
/// once and cropping it to every box in the slice, which is what keeps a
/// partitioned run from re-reading the file per partition.
pub type BuildBatch<'a> =
    dyn Fn(&[(BBox, Laea)]) -> Result<Vec<Box<dyn Enricher + 'a>>, Box<dyn Error>> + 'a;

/// Run the pipeline with one projection per partition, for `--partition`.
///
/// Splits the unique locations into pieces no single LAEA projection is too
/// distorted for, builds an enricher per piece, and enriches each piece in its
/// own projection. The join back onto input rows is the same scatter-gather
/// [`run_module`] uses, so the output is identical in shape: only the accuracy
/// of the numbers differs.
pub fn run_partitioned(
    build: &BuildBatch<'_>,
    outputs: &[OutputSpec],
    df: DataFrame,
    s: &Settings,
    out_path: &Path,
    out_fmt: Format,
) -> Result<(), Box<dyn Error>> {
    set_threads(s);
    check_clashes(&df, outputs, s)?;
    let r = reduce(&df, s)?;

    let parts = partition(&r.uniq, DEFAULT_TOLERANCE);
    if parts.is_empty() {
        return Err("--partition found no usable coordinates in the input".into());
    }
    let batched = batches(&parts);
    eprintln!(
        "[seastamp] --partition: {} partition{} over {} unique locations, worst distortion {:.2}%{}",
        parts.len(),
        if parts.len() == 1 { "" } else { "s" },
        r.uniq.len(),
        worst_distortion(&parts) * 100.0,
        if batched.len() > 1 {
            format!(
                ", reference data read {} times to stay within memory",
                batched.len()
            )
        } else {
            String::new()
        }
    );

    // One slot per unique location. Every location belongs to exactly one
    // partition (`reduce` has already dropped the unusable ones), so every slot
    // is filled before `finish` reads them.
    let mut results: Vec<Option<Vec<Value>>> = (0..r.uniq.len()).map(|_| None).collect();
    for range in batches(&parts) {
        let group = &parts[range];
        let regions: Vec<(BBox, Laea)> = group
            .iter()
            .map(|p| (p.bbox, Laea::new(p.center.0, p.center.1)))
            .collect();
        let enrichers = build(&regions)?;
        if enrichers.len() != group.len() {
            return Err("internal error: an enricher per partition was not built".into());
        }
        for (p, enr) in group.iter().zip(&enrichers) {
            let locs: Vec<(f64, f64)> = p.members.iter().map(|&i| r.uniq[i]).collect();
            for (&i, v) in p.members.iter().zip(enrich_all(enr.as_ref(), &locs)) {
                results[i] = Some(v);
            }
        }
    }

    let results: Vec<Vec<Value>> = results
        .into_iter()
        .map(|v| v.expect("every unique location belongs to a partition"))
        .collect();
    finish(df, outputs.to_vec(), &results, &r, s, out_path, out_fmt)
}
