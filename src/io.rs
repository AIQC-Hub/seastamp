//! Reading and writing the supported tabular formats: parquet, csv, tsv, and the
//! gzip variants csv.gz / tsv.gz. Gzip is handled with `flate2` (decompress into
//! memory on read, wrap the writer on write) rather than relying on a Polars
//! feature, so behavior is identical across Polars versions.

use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use polars::prelude::*;

use crate::cli::Format;

/// Resolve `Auto` to a concrete format from the file name. Unknown extensions
/// fall back to Parquet, the project default.
pub fn resolve_format(path: &Path, explicit: Format) -> Format {
    if explicit != Format::Auto {
        return explicit;
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".parquet") || name.ends_with(".pq") {
        Format::Parquet
    } else if name.ends_with(".tsv.gz") {
        Format::TsvGz
    } else if name.ends_with(".csv.gz") {
        Format::CsvGz
    } else if name.ends_with(".tsv") {
        Format::Tsv
    } else if name.ends_with(".csv") {
        Format::Csv
    } else {
        Format::Parquet
    }
}

/// The canonical file extension for a concrete format, used to build default
/// output names. `Auto` has no extension of its own and maps to parquet, the
/// project default.
pub fn format_ext(fmt: Format) -> &'static str {
    match fmt {
        Format::Parquet | Format::Auto => "parquet",
        Format::Csv => "csv",
        Format::Tsv => "tsv",
        Format::CsvGz => "csv.gz",
        Format::TsvGz => "tsv.gz",
    }
}

fn is_tab(fmt: Format) -> bool {
    matches!(fmt, Format::Tsv | Format::TsvGz)
}

fn is_gz(fmt: Format) -> bool {
    matches!(fmt, Format::CsvGz | Format::TsvGz)
}

/// Read a whole table into memory as a [`DataFrame`].
///
/// The full input is read eagerly because the join back onto every row needs it;
/// the set stamped is only the unique locations. For very large inputs a
/// streamed two-pass version (like ctddump's converters) is the natural next
/// step: see the note in CLAUDE.md.
pub fn read_frame(path: &Path, fmt: Format) -> Result<DataFrame, Box<dyn Error>> {
    let fmt = resolve_format(path, fmt);
    match fmt {
        Format::Parquet => {
            let f = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
            Ok(ParquetReader::new(f).finish()?)
        }
        Format::Csv | Format::Tsv | Format::CsvGz | Format::TsvGz => {
            let sep = if is_tab(fmt) { b'\t' } else { b',' };
            let cursor = read_bytes(path, is_gz(fmt))?;
            let df = CsvReadOptions::default()
                .with_has_header(true)
                .with_parse_options(CsvParseOptions::default().with_separator(sep))
                .into_reader_with_file_handle(cursor)
                .finish()?;
            Ok(df)
        }
        Format::Auto => unreachable!("resolve_format removes Auto"),
    }
}

fn read_bytes(path: &Path, gz: bool) -> Result<Cursor<Vec<u8>>, Box<dyn Error>> {
    let mut f = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut buf = Vec::new();
    if gz {
        GzDecoder::new(f).read_to_end(&mut buf)?;
    } else {
        f.read_to_end(&mut buf)?;
    }
    Ok(Cursor::new(buf))
}

/// Write a [`DataFrame`] to `path` in the given format.
pub fn write_frame(mut df: DataFrame, path: &Path, fmt: Format) -> Result<(), Box<dyn Error>> {
    let fmt = resolve_format(path, fmt);
    let f = File::create(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    match fmt {
        Format::Parquet => {
            // set_parallel(false) mirrors ctddump: Polars 0.43's parallel column
            // encoder leaks memory per call; the single-thread path is safe.
            ParquetWriter::new(f).set_parallel(false).finish(&mut df)?;
        }
        Format::Csv | Format::Tsv | Format::CsvGz | Format::TsvGz => {
            let sep = if is_tab(fmt) { b'\t' } else { b',' };
            if is_gz(fmt) {
                let mut w = GzEncoder::new(BufWriter::new(f), Compression::default());
                CsvWriter::new(&mut w)
                    .with_separator(sep)
                    .include_header(true)
                    .finish(&mut df)?;
                w.finish()?; // flush the gzip footer
            } else {
                let mut w = BufWriter::new(f);
                CsvWriter::new(&mut w)
                    .with_separator(sep)
                    .include_header(true)
                    .finish(&mut df)?;
                w.flush()?;
            }
        }
        Format::Auto => unreachable!("resolve_format removes Auto"),
    }
    Ok(())
}
