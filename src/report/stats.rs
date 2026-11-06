//! `report stats`: summarise the columns the stamping commands wrote.
//!
//! The file is reduced to unique locations exactly as the stamping pipeline
//! reduces it (longitude and latitude rounded to `--decimals`, rows without a
//! finite pair set aside), so a location here is one lookup the stamper made.
//! Each location's value is read from its first row: the stamper wrote the same
//! value to every row of a location, so any row would do.
//!
//! Statistics (min / max / mean) are taken over locations, so a cast with many
//! rows weighs no more than one with few. Every count is given twice, for rows
//! and for locations.
//!
//! The output is one long table, so the same layout serves the whole file and a
//! `--by` breakdown, and `report summary` can parse it back. Its columns are
//! `[<by>] column kind item unit rows locations min max mean`, and the rows per
//! stamped column are:
//!
//! | kind | items |
//! |------|-------|
//! | `total` (column `(all)`) | `(located)`, `(no position)` |
//! | `distance` | `(value)` (with min/max/mean), `(null)`, one per band |
//! | `depth` | `(value)`, `(null)`, `land`, one per band below sea level |
//! | `text` | one per value, most locations first, then `(null)` |
//! | `flag` | `true`, `false`, `(null)` |
//!
//! Every item is written even when its counts are zero, so a reader never has
//! to guess whether a missing row means zero or "not computed".

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;

use polars::prelude::*;

use crate::cli::{DistUnit, Format, ReportStatsArgs};
use crate::io::{read_frame, resolve_format};
use crate::report::table::{self, Table};

/// The pseudo column carrying the file totals.
pub const ALL: &str = "(all)";
/// Totals item: rows with a finite longitude and latitude, and their locations.
pub const LOCATED: &str = "(located)";
/// Totals item: rows without a usable position (they get no stamped value).
pub const NO_POSITION: &str = "(no position)";
/// Numeric item: locations with a value, carrying min / max / mean.
pub const VALUE: &str = "(value)";
/// Item for locations without a value.
pub const NULL: &str = "(null)";
/// Depth item: elevation at or above sea level.
pub const LAND: &str = "land";

/// The fixed columns of the report, after the optional `--by` column.
pub const HEADER: [&str; 9] =
    ["column", "kind", "item", "unit", "rows", "locations", "min", "max", "mean"];

/// What a stamped column holds, which decides the rows reported for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Total,
    Distance,
    Depth,
    Text,
    Flag,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Total => "total",
            Kind::Distance => "distance",
            Kind::Depth => "depth",
            Kind::Text => "text",
            Kind::Flag => "flag",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "total" => Kind::Total,
            "distance" => Kind::Distance,
            "depth" => Kind::Depth,
            "text" => Kind::Text,
            "flag" => Kind::Flag,
            _ => return None,
        })
    }
}

/// The stamped columns under their default names, in report order. `country_code`
/// is left out: it restates `country`.
pub const STAMPED: [(&str, Kind); 9] = [
    ("dist_to_coast", Kind::Distance),
    ("bathymetry", Kind::Depth),
    ("on_land", Kind::Flag),
    ("sea_name", Kind::Text),
    ("country", Kind::Text),
    ("municipality", Kind::Text),
    ("municipality_dist", Kind::Distance),
    ("nearest_name", Kind::Text),
    ("nearest_dist", Kind::Distance),
];

/// One stamped column to report on.
struct Target {
    name: String,
    kind: Kind,
    unit: &'static str,
    /// Band edges in the column's own unit (empty for text and flags).
    edges: Vec<f64>,
}

impl Target {
    fn labels(&self) -> Vec<String> {
        band_labels(&self.edges)
    }
}

/// `< a`, `a-b`, ..., `>= z` for edges `a < b < ... < z`.
pub fn band_labels(edges: &[f64]) -> Vec<String> {
    let mut v = Vec::with_capacity(edges.len() + 1);
    if let Some(first) = edges.first() {
        v.push(format!("< {}", num(*first)));
    }
    for w in edges.windows(2) {
        v.push(format!("{}-{}", num(w[0]), num(w[1])));
    }
    if let Some(last) = edges.last() {
        v.push(format!(">= {}", num(*last)));
    }
    v
}

/// Shortest plain rendering of a band edge: `1`, `0.5`, `1000`.
fn num(v: f64) -> String {
    format!("{v}")
}

/// Index of the band holding `v`: the first edge above it, else the last band.
fn band_of(v: f64, edges: &[f64]) -> usize {
    edges.iter().position(|&e| v < e).unwrap_or(edges.len())
}

fn check_edges(flag: &str, edges: &[f64]) -> Result<(), Box<dyn Error>> {
    if edges.is_empty() {
        return Err(format!("{flag} needs at least one band edge").into());
    }
    if edges.iter().any(|e| !e.is_finite() || *e <= 0.0) {
        return Err(format!("{flag}: band edges must be positive numbers").into());
    }
    if edges.windows(2).any(|w| w[0] >= w[1]) {
        return Err(format!("{flag}: band edges must be strictly increasing").into());
    }
    Ok(())
}

fn unit_str(u: DistUnit) -> &'static str {
    match u {
        DistUnit::Km => "km",
        DistUnit::M => "m",
    }
}

pub fn run(a: ReportStatsArgs) -> Result<(), Box<dyn Error>> {
    if let Some(n) = a.threads {
        // Polars sizes its pool from this on first use, which has not happened yet.
        std::env::set_var("POLARS_MAX_THREADS", n.max(1).to_string());
    }
    check_edges("--dist-bands", &a.dist_bands)?;
    check_edges("--depth-bands", &a.depth_bands)?;

    let available = column_names(&a.input, a.in_format)?;
    for (col, flag) in [(&a.lon_col, "--lon-col"), (&a.lat_col, "--lat-col")] {
        if !available.contains(col) {
            return Err(format!("input has no column '{col}' (see {flag})").into());
        }
    }
    if let Some(by) = &a.by {
        if !available.contains(by) {
            return Err(format!("input has no column '{by}' (see --by)").into());
        }
    }

    let mut targets = Vec::new();
    for (name, kind) in STAMPED {
        if !available.iter().any(|c| c == name) {
            continue;
        }
        let (unit, edges) = match kind {
            Kind::Distance => {
                let u = match name {
                    "dist_to_coast" => a.coast_unit,
                    "municipality_dist" => a.place_unit,
                    _ => a.nearest_unit,
                };
                let scale = if u == DistUnit::M { 1000.0 } else { 1.0 };
                (unit_str(u), a.dist_bands.iter().map(|e| e * scale).collect())
            }
            Kind::Depth => ("m", a.depth_bands.clone()),
            _ => ("", Vec::new()),
        };
        targets.push(Target { name: name.to_string(), kind, unit, edges });
    }
    if targets.is_empty() {
        let names: Vec<&str> = STAMPED.iter().map(|(n, _)| *n).collect();
        eprintln!(
            "[seastamp] report: no stamped columns found (looked for {}); only the totals are reported",
            names.join(", ")
        );
    }

    let mut wanted: Vec<String> = vec![a.lon_col.clone(), a.lat_col.clone()];
    if let Some(by) = &a.by {
        if !wanted.contains(by) {
            wanted.push(by.clone());
        }
    }
    for t in &targets {
        if !wanted.contains(&t.name) {
            wanted.push(t.name.clone());
        }
    }
    let df = load(&a.input, a.in_format, &wanted)?;

    let report = compute(&df, &a, &mut targets)?;
    table::write(&report.table, a.format, a.dest.as_deref())?;
    eprintln!(
        "[seastamp] report: {} rows, {} {}, {} stamped column{} -> {}",
        df.height(),
        report.locations,
        // Under --by a location is counted once per group it appears in.
        a.by.as_ref().map_or("unique locations".to_string(), |b| {
            format!("locations over {} {b} groups", report.groups)
        }),
        targets.len(),
        if targets.len() == 1 { "" } else { "s" },
        a.dest.as_ref().map_or("stdout".to_string(), |p| p.display().to_string()),
    );
    Ok(())
}

/// Column names of the input, without reading its data where the format allows.
fn column_names(path: &Path, fmt: Format) -> Result<Vec<String>, Box<dyn Error>> {
    if resolve_format(path, fmt) == Format::Parquet {
        let f = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let schema = ParquetReader::new(f).schema()?;
        return Ok(schema.iter_names().map(|n| n.to_string()).collect());
    }
    // Text formats have no separate schema; reading the header means reading
    // the file, which `load` does again. They are the small-file formats.
    let df = read_frame(path, fmt)?;
    Ok(df.get_column_names().iter().map(|n| n.to_string()).collect())
}

/// Read only the columns the report needs. Parquet reads just those columns;
/// the text formats are read whole and then narrowed.
fn load(path: &Path, fmt: Format, cols: &[String]) -> Result<DataFrame, Box<dyn Error>> {
    if resolve_format(path, fmt) == Format::Parquet {
        let f = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        return Ok(ParquetReader::new(f).with_columns(Some(cols.to_vec())).finish()?);
    }
    Ok(read_frame(path, fmt)?.select(cols)?)
}

struct Report {
    table: Table,
    locations: usize,
    groups: usize,
}

/// Per-group accumulator for a numeric column.
#[derive(Clone)]
struct NumAcc {
    rows: u64,
    locs: u64,
    null_rows: u64,
    null_locs: u64,
    land_rows: u64,
    land_locs: u64,
    min: f64,
    max: f64,
    sum: f64,
    band_rows: Vec<u64>,
    band_locs: Vec<u64>,
}

impl NumAcc {
    fn new(nbands: usize) -> NumAcc {
        NumAcc {
            rows: 0,
            locs: 0,
            null_rows: 0,
            null_locs: 0,
            land_rows: 0,
            land_locs: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
            band_rows: vec![0; nbands],
            band_locs: vec![0; nbands],
        }
    }
}

/// Per-group counts for a text or flag column: value -> (rows, locations).
#[derive(Clone, Default)]
struct CatAcc {
    counts: HashMap<String, (u64, u64)>,
    null: (u64, u64),
}

fn compute(df: &DataFrame, a: &ReportStatsArgs, targets: &mut [Target]) -> Result<Report, Box<dyn Error>> {
    let n = df.height();
    let lon = df.column(&a.lon_col)?.cast(&DataType::Float64)?;
    let lat = df.column(&a.lat_col)?.cast(&DataType::Float64)?;
    let (lon, lat) = (lon.f64()?, lat.f64()?);

    // ---- Groups ------------------------------------------------------------
    // One group (the whole file) unless --by is given. Groups are numbered as
    // Polars finds them and listed in the order of their values, nulls last.
    let (group_of_row, group_names, group_order) = match &a.by {
        None => (None, vec![String::new()], vec![0usize]),
        Some(by) => {
            let gb = df.group_by([by.as_str()])?;
            let mut ids = vec![0u32; n];
            let mut firsts: Vec<IdxSize> = Vec::new();
            match gb.get_groups() {
                GroupsProxy::Idx(g) => {
                    for (gi, (first, idx)) in g.iter().enumerate() {
                        firsts.push(first);
                        for &i in idx.iter() {
                            ids[i as usize] = gi as u32;
                        }
                    }
                }
                GroupsProxy::Slice { groups, .. } => {
                    for (gi, [first, len]) in groups.iter().enumerate() {
                        firsts.push(*first);
                        for i in *first..first + len {
                            ids[i as usize] = gi as u32;
                        }
                    }
                }
            }
            let idx = IdxCa::from_vec("idx".into(), firsts);
            let values = df.column(by)?.take(&idx)?;
            let order: Vec<usize> = values
                .arg_sort(SortOptions::default().with_nulls_last(true))
                .into_no_null_iter()
                .map(|i| i as usize)
                .collect();
            let names_s = values.cast(&DataType::String)?;
            let names: Vec<String> = names_s
                .str()?
                .into_iter()
                .map(|o| o.map_or_else(|| NULL.to_string(), str::to_string))
                .collect();
            (Some(ids), names, order)
        }
    };
    let ngroups = group_names.len();

    // ---- Locations ---------------------------------------------------------
    // The same reduction as pipeline::reduce, keyed per group as well, so a
    // position shared by two groups is one location in each.
    let scale = 10f64.powi(a.decimals as i32);
    let round = |v: f64| (v * scale).round() / scale;
    let mut index: HashMap<(u32, i64, i64), usize> = HashMap::new();
    let mut first_row: Vec<usize> = Vec::new();
    let mut loc_rows: Vec<u64> = Vec::new();
    let mut loc_group: Vec<u32> = Vec::new();
    let mut located = vec![(0u64, 0u64); ngroups];
    let mut no_pos = vec![0u64; ngroups];
    for (i, (lo, la)) in lon.into_iter().zip(lat).enumerate() {
        let g = group_of_row.as_ref().map_or(0, |v| v[i]);
        match (lo, la) {
            (Some(lo), Some(la)) if lo.is_finite() && la.is_finite() => {
                let k = (g, (round(lo) * scale).round() as i64, (round(la) * scale).round() as i64);
                let l = *index.entry(k).or_insert_with(|| {
                    first_row.push(i);
                    loc_rows.push(0);
                    loc_group.push(g);
                    located[g as usize].1 += 1;
                    first_row.len() - 1
                });
                loc_rows[l] += 1;
                located[g as usize].0 += 1;
            }
            _ => no_pos[g as usize] += 1,
        }
    }
    drop(index);
    let nlocs = first_row.len();
    // Each stamped column is narrowed to one value per location (its first
    // row) before anything else, so no full-length copy of it is ever made.
    let firsts = IdxCa::from_vec("first".into(), first_row.iter().map(|&i| i as IdxSize).collect());
    drop(first_row);

    // ---- Rows of the report, per group --------------------------------------
    let mut out: Vec<Vec<Vec<String>>> = vec![Vec::new(); ngroups];
    for g in 0..ngroups {
        out[g].push(row(ALL, Kind::Total, LOCATED, "", located[g].0, located[g].1, None));
        out[g].push(row(ALL, Kind::Total, NO_POSITION, "", no_pos[g], 0, None));
    }

    for t in targets.iter_mut() {
        let s = df.column(&t.name)?.take(&firsts)?;
        // A column whose dtype does not fit its expected kind is reported by
        // what it actually holds: a non-numeric "distance" as text, a
        // non-boolean "flag" as text.
        let numeric = s.dtype().is_numeric();
        if matches!(t.kind, Kind::Distance | Kind::Depth) && !numeric {
            eprintln!("[seastamp] report: '{}' is not numeric; reporting it as text", t.name);
            t.kind = Kind::Text;
            t.unit = "";
        }
        if t.kind == Kind::Flag && s.dtype() != &DataType::Boolean {
            eprintln!("[seastamp] report: '{}' is not boolean; reporting it as text", t.name);
            t.kind = Kind::Text;
        }

        match t.kind {
            Kind::Distance | Kind::Depth => {
                let f = s.cast(&DataType::Float64)?;
                let f = f.f64()?;
                let nb = t.edges.len() + 1;
                let mut acc = vec![NumAcc::new(nb); ngroups];
                for l in 0..nlocs {
                    let (g, r) = (loc_group[l] as usize, loc_rows[l]);
                    let ac = &mut acc[g];
                    match f.get(l).filter(|v| !v.is_nan()) {
                        None => {
                            ac.null_rows += r;
                            ac.null_locs += 1;
                        }
                        Some(v) => {
                            ac.rows += r;
                            ac.locs += 1;
                            ac.min = ac.min.min(v);
                            ac.max = ac.max.max(v);
                            ac.sum += v;
                            let banded = if t.kind == Kind::Depth {
                                // Depth below sea level, whichever sign the file uses.
                                let d = if a.depth_positive { v } else { -v };
                                if d <= 0.0 {
                                    ac.land_rows += r;
                                    ac.land_locs += 1;
                                    None
                                } else {
                                    Some(d)
                                }
                            } else {
                                Some(v)
                            };
                            if let Some(d) = banded {
                                let b = band_of(d, &t.edges);
                                ac.band_rows[b] += r;
                                ac.band_locs[b] += 1;
                            }
                        }
                    }
                }
                let labels = t.labels();
                for g in 0..ngroups {
                    let ac = &acc[g];
                    let stats = (ac.locs > 0).then(|| (ac.min, ac.max, ac.sum / ac.locs as f64));
                    let o = &mut out[g];
                    o.push(row(&t.name, t.kind, VALUE, t.unit, ac.rows, ac.locs, stats));
                    o.push(row(&t.name, t.kind, NULL, t.unit, ac.null_rows, ac.null_locs, None));
                    if t.kind == Kind::Depth {
                        o.push(row(&t.name, t.kind, LAND, t.unit, ac.land_rows, ac.land_locs, None));
                    }
                    for (b, label) in labels.iter().enumerate() {
                        o.push(row(&t.name, t.kind, label, t.unit, ac.band_rows[b], ac.band_locs[b], None));
                    }
                }
            }
            Kind::Text | Kind::Flag => {
                let mut acc = vec![CatAcc::default(); ngroups];
                let values: Box<dyn Fn(usize) -> Option<String>> = if t.kind == Kind::Flag {
                    let b = s.bool()?.clone();
                    Box::new(move |i| b.get(i).map(|v| v.to_string()))
                } else {
                    let st = s.cast(&DataType::String)?;
                    let st = st.str()?.clone();
                    Box::new(move |i| st.get(i).map(str::to_string))
                };
                for l in 0..nlocs {
                    let (g, r) = (loc_group[l] as usize, loc_rows[l]);
                    let ac = &mut acc[g];
                    match values(l) {
                        None => {
                            ac.null.0 += r;
                            ac.null.1 += 1;
                        }
                        Some(v) => {
                            let e = ac.counts.entry(v).or_insert((0, 0));
                            e.0 += r;
                            e.1 += 1;
                        }
                    }
                }
                for g in 0..ngroups {
                    let ac = &acc[g];
                    let mut items: Vec<(&String, &(u64, u64))> = if t.kind == Kind::Flag {
                        // Fixed order, both always present.
                        Vec::new()
                    } else {
                        let mut v: Vec<_> = ac.counts.iter().collect();
                        v.sort_by(|x, y| y.1 .1.cmp(&x.1 .1).then(y.1 .0.cmp(&x.1 .0)).then(x.0.cmp(y.0)));
                        v
                    };
                    let o = &mut out[g];
                    if t.kind == Kind::Flag {
                        for key in ["true", "false"] {
                            let (r, l) = ac.counts.get(key).copied().unwrap_or((0, 0));
                            o.push(row(&t.name, t.kind, key, "", r, l, None));
                        }
                    }
                    for (v, &(r, l)) in items.drain(..) {
                        o.push(row(&t.name, t.kind, v, "", r, l, None));
                    }
                    o.push(row(&t.name, t.kind, NULL, "", ac.null.0, ac.null.1, None));
                }
            }
            Kind::Total => unreachable!("totals are not a stamped column"),
        }
    }

    // ---- Assemble -----------------------------------------------------------
    let mut header: Vec<String> = Vec::new();
    let mut numeric: Vec<bool> = Vec::new();
    if let Some(by) = &a.by {
        header.push(by.clone());
        numeric.push(false);
    }
    header.extend(HEADER.iter().map(|h| h.to_string()));
    numeric.extend([false, false, false, false, true, true, true, true, true]);

    let mut rows = Vec::new();
    for &g in &group_order {
        for mut r in std::mem::take(&mut out[g]) {
            if a.by.is_some() {
                r.insert(0, group_names[g].clone());
            }
            rows.push(r);
        }
    }
    Ok(Report { table: Table { header, numeric, rows }, locations: nlocs, groups: ngroups })
}

fn row(
    column: &str,
    kind: Kind,
    item: &str,
    unit: &str,
    rows: u64,
    locs: u64,
    stats: Option<(f64, f64, f64)>,
) -> Vec<String> {
    let f = |v: f64| format!("{v:.3}");
    let (min, max, mean) = stats.map_or((String::new(), String::new(), String::new()), |(a, b, c)| {
        (f(a), f(b), f(c))
    });
    vec![
        column.to_string(),
        kind.as_str().to_string(),
        item.to_string(),
        unit.to_string(),
        rows.to_string(),
        locs.to_string(),
        min,
        max,
        mean,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_label_and_place_values() {
        let e = [1.0, 5.0, 20.0];
        assert_eq!(band_labels(&e), ["< 1", "1-5", "5-20", ">= 20"]);
        assert_eq!(band_of(0.2, &e), 0);
        assert_eq!(band_of(1.0, &e), 1);
        assert_eq!(band_of(19.9, &e), 2);
        assert_eq!(band_of(20.0, &e), 3);
        assert_eq!(band_labels(&[0.5, 1000.0]), ["< 0.5", "0.5-1000", ">= 1000"]);
    }

    #[test]
    fn edges_must_be_positive_and_increasing() {
        assert!(check_edges("x", &[1.0, 5.0]).is_ok());
        assert!(check_edges("x", &[]).is_err());
        assert!(check_edges("x", &[5.0, 1.0]).is_err());
        assert!(check_edges("x", &[0.0, 1.0]).is_err());
    }
}
