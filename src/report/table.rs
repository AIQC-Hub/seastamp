//! A small string table and its three renderings (TSV, aligned text, JSON), so
//! the report's cell formatting (fixed decimals, empty for missing) is fully
//! under this module's control rather than a Polars writer's. Same approach as
//! ctddump's report writer.

use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use crate::cli::ReportFormat;

/// Header plus string rows. An empty cell means "no value"; JSON renders it as
/// `null`. Columns flagged `numeric` are written to JSON unquoted.
pub struct Table {
    pub header: Vec<String>,
    pub numeric: Vec<bool>,
    pub rows: Vec<Vec<String>>,
}

/// Write `t` to `dest`, or to stdout when `None`.
pub fn write(t: &Table, format: ReportFormat, dest: Option<&Path>) -> Result<(), Box<dyn Error>> {
    let mut w: Box<dyn Write> = match dest {
        Some(p) => Box::new(io::BufWriter::new(
            File::create(p).map_err(|e| format!("cannot create {}: {e}", p.display()))?,
        )),
        None => Box::new(io::stdout().lock()),
    };
    match format {
        ReportFormat::Tsv => write_tsv(t, &mut *w)?,
        ReportFormat::Text => write_text(t, &mut *w)?,
        ReportFormat::Json => write_json(t, &mut *w)?,
    }
    w.flush()?;
    Ok(())
}

fn write_tsv(t: &Table, w: &mut dyn Write) -> io::Result<()> {
    writeln!(w, "{}", t.header.join("\t"))?;
    for row in &t.rows {
        // A tab or newline inside a value would break the row; values come from
        // reference-data names, which never carry them, but be safe.
        let cells: Vec<String> = row.iter().map(|c| c.replace(['\t', '\n', '\r'], " ")).collect();
        writeln!(w, "{}", cells.join("\t"))?;
    }
    Ok(())
}

fn write_text(t: &Table, w: &mut dyn Write) -> io::Result<()> {
    let n = t.header.len();
    let widths: Vec<usize> = (0..n)
        .map(|i| {
            t.rows
                .iter()
                .map(|r| r[i].chars().count())
                .chain([t.header[i].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: &[String]| -> String {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let pad = widths[i] - c.chars().count();
                // Numbers right-aligned, text left-aligned.
                if t.numeric[i] {
                    format!("{}{c}", " ".repeat(pad))
                } else {
                    format!("{c}{}", " ".repeat(pad))
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    writeln!(w, "{}", line(&t.header))?;
    let rule: Vec<String> = widths.iter().map(|&k| "-".repeat(k)).collect();
    writeln!(w, "{}", rule.join("  "))?;
    for row in &t.rows {
        writeln!(w, "{}", line(row))?;
    }
    Ok(())
}

fn write_json(t: &Table, w: &mut dyn Write) -> io::Result<()> {
    writeln!(w, "[")?;
    for (k, row) in t.rows.iter().enumerate() {
        let fields: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let v = if c.is_empty() {
                    "null".to_string()
                } else if t.numeric[i] {
                    c.clone()
                } else {
                    json_str(c)
                };
                format!("{}: {v}", json_str(&t.header[i]))
            })
            .collect();
        let sep = if k + 1 < t.rows.len() { "," } else { "" };
        writeln!(w, "  {{{}}}{sep}", fields.join(", "))?;
    }
    writeln!(w, "]")?;
    Ok(())
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
