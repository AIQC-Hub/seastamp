//! The enrichment modules. Each builds an [`crate::pipeline::Enricher`] from
//! its data source and options, then hands off to `pipeline::run_module`:
//! `coast` (nearest GSHHG shoreline distance), `depth` (GEBCO grid lookup),
//! `sea` (IHO Sea Areas point in polygon), `place` (nearest Natural Earth
//! country and GISCO LAU municipality), and `nearest` (nearest point of a
//! caller-supplied table).
//!
//! `regions` is the exception: it enriches nothing and has no `Enricher`. It
//! lists sea and ocean bounding boxes, which is what the other modules' region
//! options consume.

use std::error::Error;
use std::path::{Path, PathBuf};

use crate::cli::Format;
use crate::geo::vector::Rings;
use crate::io::{format_ext, resolve_format};

pub mod coast;
pub mod depth;
pub mod nearest;
pub mod place;
pub mod regions;
pub mod sea;

/// Default output path when `--output` is omitted: the input renamed to
/// `<stem>.<tag>.<ext>` beside it, where `<ext>` matches the input format, so
/// the output format defaults to the input format. An unrecognized input
/// extension falls back to parquet, the project default.
pub(crate) fn default_output(input: &Path, tag: &str, in_fmt: Format) -> PathBuf {
    let ext = format_ext(resolve_format(input, in_fmt));
    // Strip the recognized format extension whole (".csv.gz" spans two Path
    // extensions), so a gzip input does not leave a stray ".csv" in the stem.
    const KNOWN: [&str; 6] = [".parquet", ".pq", ".csv.gz", ".tsv.gz", ".csv", ".tsv"];
    let stem = input
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|n| {
            let lower = n.to_ascii_lowercase();
            KNOWN
                .iter()
                .find_map(|e| lower.ends_with(e).then(|| &n[..n.len() - e.len()]))
        })
        .or_else(|| input.file_stem().and_then(|s| s.to_str()))
        .unwrap_or("output");
    let name = format!("{stem}.{tag}.{ext}");
    match input.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name),
        _ => PathBuf::from(name),
    }
}

/// Read every polygon from a shapefile as (rings, attribute record) pairs,
/// skipping non-polygon shapes. Used by the sea and place modules; coast keeps
/// its own streaming read because it crops segments as it goes and never needs
/// whole rings in memory.
pub(crate) fn shp_polygons(
    path: &Path,
) -> Result<Vec<(Rings, shapefile::dbase::Record)>, Box<dyn Error>> {
    let mut reader = shapefile::Reader::from_path(path)
        .map_err(|e| format!("cannot read shapefile {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for item in reader.iter_shapes_and_records() {
        let (shape, record) = item.map_err(|e| format!("reading {}: {e}", path.display()))?;
        if let shapefile::Shape::Polygon(poly) = shape {
            let rings: Rings = poly
                .rings()
                .iter()
                .map(|ring| ring.points().iter().map(|p| (p.x, p.y)).collect())
                .collect();
            out.push((rings, record));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_follows_input_format() {
        let d = |p: &str, fmt| default_output(Path::new(p), "sea", fmt);
        assert_eq!(d("dir/cores.parquet", Format::Auto), PathBuf::from("dir/cores.sea.parquet"));
        assert_eq!(d("cores.csv", Format::Auto), PathBuf::from("cores.sea.csv"));
        assert_eq!(d("cores.tsv", Format::Auto), PathBuf::from("cores.sea.tsv"));
        // the whole ".csv.gz" is replaced: no stray ".csv" in the stem
        assert_eq!(d("cores.csv.gz", Format::Auto), PathBuf::from("cores.sea.csv.gz"));
        // ".pq" reads as parquet and normalizes to the canonical extension
        assert_eq!(d("cores.pq", Format::Auto), PathBuf::from("cores.sea.parquet"));
        // an explicit --in-format wins over the extension
        assert_eq!(d("cores.dat", Format::Tsv), PathBuf::from("cores.sea.tsv"));
        // unrecognized extension: parquet, the project default
        assert_eq!(d("cores.dat", Format::Auto), PathBuf::from("cores.sea.parquet"));
    }
}
