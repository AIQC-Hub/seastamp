//! `report summary`: one Markdown or HTML page per file stem, built from the TSV
//! `report stats` wrote. It reads no data file, only that small table, so it is
//! instant and can be re-run with a new title or notes at no cost.
//!
//! The page has an Overview of rows and locations, then a section per stamped
//! module the report covers, in the order the modules run: Distance to coast,
//! Depth (with the land flag when present), Sea area, Place (country,
//! municipality, and the distance to it), and Nearest location. A module the
//! file was not stamped with has no rows in the report and no section here.
//!
//! The section prose is the same for every file; anything specific to one file
//! (which product it is, how it was stamped) goes in `--title` / `--note`, which
//! `scripts/summary_data.sh` supplies per stem.

use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::{ReportSummaryArgs, SummaryFormat};
use crate::report::stats::{Kind, ALL, HEADER, LAND, LOCATED, NO_POSITION, NULL, VALUE};

/// Where `report stats` output for `stem` sits under the report root.
pub fn report_path(report_dir: &Path, stem: &str) -> PathBuf {
    report_dir.join("stamp").join(format!("{stem}.tsv"))
}

pub fn run(a: ReportSummaryArgs) -> Result<(), Box<dyn Error>> {
    let path = report_path(&a.report_dir, &a.stem);
    let recs = read_report(&path)?;
    let title = a.title.clone().unwrap_or_else(|| format!("Summary: {}", a.stem));
    let sections = build_sections(&recs, &path, a.top)?;
    let page = match a.format {
        SummaryFormat::Md => render_md(&title, &a.note, &sections),
        SummaryFormat::Html => render_html(&title, &a.note, &sections),
    };
    let mut w: Box<dyn Write> = match &a.output {
        Some(p) => Box::new(
            File::create(p).map_err(|e| format!("cannot create {}: {e}", p.display()))?,
        ),
        None => Box::new(io::stdout().lock()),
    };
    w.write_all(page.as_bytes())?;
    w.flush()?;
    Ok(())
}

// ---- Reading the report ------------------------------------------------------

/// One row of a `report stats` TSV.
struct Rec {
    column: String,
    kind: Kind,
    item: String,
    unit: String,
    rows: u64,
    locs: u64,
    min: Option<f64>,
    max: Option<f64>,
    mean: Option<f64>,
}

fn read_report(path: &Path) -> Result<Vec<Rec>, Box<dyn Error>> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "cannot read {}: {e} (write it with `seastamp report stats <file> {}`)",
            path.display(),
            path.display()
        )
    })?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    if header != HEADER {
        if header.len() == HEADER.len() + 1 && header[1..] == HEADER {
            return Err(format!(
                "{} was written with --by {}; a summary page needs the whole-file report (run report stats without --by)",
                path.display(),
                header[0]
            )
            .into());
        }
        return Err(format!("{} is not a `seastamp report stats` TSV (unexpected header)", path.display()).into());
    }
    let mut recs = Vec::new();
    for (n, line) in lines.enumerate() {
        if line.is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        let bad = || format!("{} line {}: malformed report row", path.display(), n + 2);
        if c.len() != HEADER.len() {
            return Err(bad().into());
        }
        let kind = Kind::parse(c[1]).ok_or_else(bad)?;
        let int = |s: &str| s.parse::<u64>().map_err(|_| bad());
        let flt = |s: &str| if s.is_empty() { None } else { s.parse::<f64>().ok() };
        recs.push(Rec {
            column: c[0].to_string(),
            kind,
            item: c[2].to_string(),
            unit: c[3].to_string(),
            rows: int(c[4])?,
            locs: int(c[5])?,
            min: flt(c[6]),
            max: flt(c[7]),
            mean: flt(c[8]),
        });
    }
    Ok(recs)
}

// ---- Page model --------------------------------------------------------------

struct Section {
    title: String,
    desc: String,
    files: Vec<String>,
    tables: Vec<Table>,
}

struct Table {
    title: Option<String>,
    desc: String,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

// ---- Section prose -----------------------------------------------------------
// Generic on purpose: the same for every file and region.

const DESC_OVERVIEW: &str = "Rows and unique locations in the stamped file. seastamp rounds \
longitude and latitude, looks each unique location up once, and copies the result to every row \
there, so the locations are the lookups that were made and the rows are what the file carries. \
Rows without a usable longitude and latitude get no stamped value and are left out of every \
section below.";

const DESC_COAST: &str = "Distance from each location to the nearest GSHHG shoreline, \
written by `coast`. The distance is unsigned, so a location on land gets its distance to the \
coast as well.";

const DESC_DEPTH: &str = "GEBCO grid value at each location, written by `depth`. A location \
whose elevation is at or above sea level counts as land; the bands split the others by depth \
below sea level. Minimum, mean, and maximum are of the values as the file stores them, which are \
elevations (negative under water) unless depth was stamped with `--positive`.";

const DESC_SEA: &str = "IHO sea area containing each location, written by `sea`. A location \
inside no IHO polygon has no value, which is expected on land.";

const DESC_PLACE: &str = "Nearest Natural Earth country and nearest GISCO LAU municipality, \
written by `place`. GISCO covers Europe only, and a municipality further away than \
`--max-municipality-dist` is dropped, so locations elsewhere have no municipality.";

const DESC_NEAREST: &str = "Nearest location in the reference table given to `nearest`, and \
the distance to it.";

const DESC_STATS: &str = "Over unique locations, so a location with many rows counts once. \
Percentages are of the located rows and locations in the Overview.";

const DESC_DIST_BANDS: &str = "Each band includes its lower edge and excludes its upper one.";

const DESC_DEPTH_BANDS: &str = "Land first, then the locations at sea by depth below sea \
level. Each band includes its lower edge and excludes its upper one.";

const DESC_FLAG: &str = "The `on_land` column, written when depth is stamped with \
`--on-land`. It is judged from the same elevation, so it agrees with the Land band above.";

/// Section a stamped column belongs to, its label within it, and the section prose.
fn placement(column: &str) -> (&'static str, &'static str, &'static str) {
    match column {
        "dist_to_coast" => ("Distance to coast", "Distance to coast", DESC_COAST),
        "bathymetry" => ("Depth", "Depth", DESC_DEPTH),
        "on_land" => ("Depth", "Land flag", DESC_DEPTH),
        "sea_name" => ("Sea area", "Sea area", DESC_SEA),
        "country" => ("Place", "Country", DESC_PLACE),
        "municipality" => ("Place", "Municipality", DESC_PLACE),
        "municipality_dist" => ("Place", "Distance to municipality", DESC_PLACE),
        "nearest_name" => ("Nearest location", "Nearest location", DESC_NEAREST),
        "nearest_dist" => ("Nearest location", "Distance to nearest location", DESC_NEAREST),
        _ => ("Other columns", "", ""),
    }
}

// ---- Section assembly --------------------------------------------------------

fn build_sections(recs: &[Rec], path: &Path, top: usize) -> Result<Vec<Section>, Box<dyn Error>> {
    let find = |col: &str, item: &str| recs.iter().find(|r| r.column == col && r.item == item);
    let located = find(ALL, LOCATED).ok_or("report has no totals row")?;
    let no_pos = find(ALL, NO_POSITION).map_or(0, |r| r.rows);
    let base = (located.rows, located.locs);

    let mut sections = vec![Section {
        title: "Overview".into(),
        desc: DESC_OVERVIEW.into(),
        files: vec![path.to_string_lossy().replace('\\', "/")],
        tables: vec![Table {
            title: None,
            desc: String::new(),
            headers: vec!["".into(), "Rows".into(), "Unique locations".into()],
            rows: vec![
                vec!["With a position".into(), fmt_int(located.rows), fmt_int(located.locs)],
                vec!["Without a position".into(), fmt_int(no_pos), String::new()],
                vec!["Total".into(), fmt_int(located.rows + no_pos), fmt_int(located.locs)],
            ],
        }],
    }];

    // Columns in report order, each with its rows.
    let mut columns: Vec<(&str, Kind, Vec<&Rec>)> = Vec::new();
    for r in recs.iter().filter(|r| r.column != ALL) {
        match columns.iter_mut().find(|c| c.0 == r.column) {
            Some(c) => c.2.push(r),
            None => columns.push((&r.column, r.kind, vec![r])),
        }
    }

    // How many columns land in each section decides whether table titles need
    // the column's label to tell them apart.
    let per_section = |s: &str| columns.iter().filter(|c| placement(c.0).0 == s).count();

    for (col, kind, rows) in &columns {
        let (sec_title, label, desc) = placement(col);
        let label = if label.is_empty() { *col } else { label };
        let shared = per_section(sec_title) > 1;
        let titled = |t: &str| if shared { format!("{label}: {t}") } else { capitalize(t) };
        let tables = match kind {
            Kind::Distance | Kind::Depth => numeric_tables(*kind, rows, base, &titled),
            Kind::Text => vec![value_table(label, rows, base, top, shared)],
            Kind::Flag => vec![flag_table(label, rows, base)],
            Kind::Total => Vec::new(),
        };
        match sections.iter_mut().find(|s| s.title == sec_title) {
            Some(s) => s.tables.extend(tables),
            None => sections.push(Section {
                title: sec_title.into(),
                desc: desc.into(),
                files: Vec::new(),
                tables,
            }),
        }
    }
    Ok(sections)
}

fn numeric_tables(kind: Kind, rows: &[&Rec], base: (u64, u64), titled: &dyn Fn(&str) -> String) -> Vec<Table> {
    let get = |item: &str| rows.iter().find(|r| r.item == item);
    let unit = rows.first().map_or("", |r| r.unit.as_str());
    let with_unit = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.3} {unit}").trim_end().to_string());

    let val = get(VALUE);
    let null = get(NULL);
    let stats = Table {
        title: Some(titled("statistics")),
        desc: DESC_STATS.into(),
        headers: vec!["Statistic".into(), "Value".into()],
        rows: vec![
            vec!["Locations with a value".into(), count_pct(val.map_or(0, |r| r.locs), base.1)],
            vec!["Locations without a value".into(), count_pct(null.map_or(0, |r| r.locs), base.1)],
            vec!["Rows without a value".into(), count_pct(null.map_or(0, |r| r.rows), base.0)],
            vec!["Minimum".into(), with_unit(val.and_then(|r| r.min))],
            vec!["Mean".into(), with_unit(val.and_then(|r| r.mean))],
            vec!["Maximum".into(), with_unit(val.and_then(|r| r.max))],
        ],
    };

    let (band_head, desc) = if kind == Kind::Depth {
        ("Depth below sea level", DESC_DEPTH_BANDS)
    } else {
        ("Distance", DESC_DIST_BANDS)
    };
    let head = if unit.is_empty() { band_head.to_string() } else { format!("{band_head} ({unit})") };
    let mut band_rows = Vec::new();
    for r in rows.iter().filter(|r| r.item != VALUE && r.item != NULL) {
        let label = if r.item == LAND { "Land (at or above sea level)".to_string() } else { r.item.clone() };
        band_rows.push(count_row(label, r, base));
    }
    if let Some(n) = null {
        band_rows.push(count_row("No value".into(), n, base));
    }
    let bands = Table {
        title: Some(titled("bands")),
        desc: desc.into(),
        headers: count_headers(&head),
        rows: band_rows,
    };
    vec![stats, bands]
}

fn value_table(label: &str, rows: &[&Rec], base: (u64, u64), top: usize, shared: bool) -> Table {
    let values: Vec<&&Rec> = rows.iter().filter(|r| r.item != NULL).collect();
    let mut out: Vec<Vec<String>> = values.iter().take(top).map(|r| count_row(r.item.clone(), r, base)).collect();
    let rest = &values[values.len().min(top)..];
    if !rest.is_empty() {
        let (r, l) = rest.iter().fold((0, 0), |(a, b), x| (a + x.rows, b + x.locs));
        let k = rest.len();
        let agg = Rec {
            column: String::new(),
            kind: Kind::Text,
            item: String::new(),
            unit: String::new(),
            rows: r,
            locs: l,
            min: None,
            max: None,
            mean: None,
        };
        out.push(count_row(format!("Other ({k} value{})", if k == 1 { "" } else { "s" }), &agg, base));
    }
    if let Some(n) = rows.iter().find(|r| r.item == NULL) {
        out.push(count_row("No value".into(), n, base));
    }
    let desc = if rest.is_empty() {
        format!("{} distinct value{}.", values.len(), if values.len() == 1 { "" } else { "s" })
    } else {
        format!(
            "{} distinct values. The table lists the {top} with the most locations and sums the rest in one row.",
            values.len()
        )
    };
    Table {
        title: shared.then(|| label.to_string()),
        desc,
        headers: count_headers(label),
        rows: out,
    }
}

fn flag_table(label: &str, rows: &[&Rec], base: (u64, u64)) -> Table {
    let mut out = Vec::new();
    for (item, name) in [("true", "Yes"), ("false", "No"), (NULL, "No value")] {
        if let Some(r) = rows.iter().find(|r| r.item == item) {
            out.push(count_row(name.into(), r, base));
        }
    }
    Table {
        title: Some(label.to_string()),
        desc: DESC_FLAG.into(),
        headers: count_headers("On land"),
        rows: out,
    }
}

fn count_headers(first: &str) -> Vec<String> {
    vec![
        first.to_string(),
        "Locations".into(),
        "% of locations".into(),
        "Rows".into(),
        "% of rows".into(),
    ]
}

fn count_row(label: String, r: &Rec, base: (u64, u64)) -> Vec<String> {
    vec![label, fmt_int(r.locs), fmt_pct(r.locs, base.1), fmt_int(r.rows), fmt_pct(r.rows, base.0)]
}

fn count_pct(n: u64, base: u64) -> String {
    format!("{} ({})", fmt_int(n), fmt_pct(n, base))
}

fn fmt_pct(n: u64, base: u64) -> String {
    if base == 0 {
        return String::new();
    }
    format!("{:.2}%", 100.0 * n as f64 / base as f64)
}

/// Thousands separated with commas: 1234567 -> 1,234,567.
fn fmt_int(n: u64) -> String {
    let s = n.to_string();
    let mut o = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            o.push(',');
        }
        o.push(ch);
    }
    o
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

// ---- Renderers ---------------------------------------------------------------

fn render_md(title: &str, notes: &[String], sections: &[Section]) -> String {
    let cell = |s: &str| s.replace('|', "\\|");
    let mut s = format!("# {title}\n\n");
    for n in notes {
        s.push_str(&format!("> {n}\n\n"));
    }
    for sec in sections {
        s.push_str(&format!("## {}\n\n", sec.title));
        if !sec.desc.is_empty() {
            s.push_str(&format!("{}\n\n", sec.desc));
        }
        for f in &sec.files {
            s.push_str(&format!("- File: `{f}`\n"));
        }
        if !sec.files.is_empty() {
            s.push('\n');
        }
        for t in &sec.tables {
            if let Some(tt) = &t.title {
                s.push_str(&format!("**{tt}**\n\n"));
            }
            if !t.desc.is_empty() {
                s.push_str(&format!("{}\n\n", t.desc));
            }
            let heads: Vec<String> = t.headers.iter().map(|h| cell(h)).collect();
            s.push_str(&format!("| {} |\n", heads.join(" | ")));
            let align: Vec<&str> = (0..t.headers.len()).map(|i| if i == 0 { " --- " } else { " ---: " }).collect();
            s.push_str(&format!("|{}|\n", align.join("|")));
            for row in &t.rows {
                let cells: Vec<String> = row.iter().map(|c| cell(c)).collect();
                s.push_str(&format!("| {} |\n", cells.join(" | ")));
            }
            s.push('\n');
        }
    }
    s
}

fn render_html(title: &str, notes: &[String], sections: &[Section]) -> String {
    let mut b = String::new();
    b.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    b.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    b.push_str(&format!("<title>{}</title>\n", esc(title)));
    b.push_str(
        "<style>\n\
         :root{--fg:#1a1a1a;--muted:#555;--rule:#ccc;--head:#f2f2f2;--code:#f5f5f5;\
         --note-bar:#b0c4de;--note-bg:#f7fafd;--bg:#fff}\n\
         @media (prefers-color-scheme: dark){:root{--fg:#e6e6e6;--muted:#aaa;--rule:#444;\
         --head:#2a2a2a;--code:#2a2a2a;--note-bar:#5a7394;--note-bg:#1d232b;--bg:#161616}}\n\
         body{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;background:var(--bg);\
         max-width:60rem;margin:2rem auto;padding:0 1rem;color:var(--fg);line-height:1.5}\n\
         h1{border-bottom:2px solid var(--rule);padding-bottom:.2em}\n\
         h2{margin-top:2rem;border-bottom:1px solid var(--rule);padding-bottom:.2em}\n\
         .scroll{overflow-x:auto}\n\
         table{border-collapse:collapse;margin:.5rem 0 1.5rem}\n\
         th,td{border:1px solid var(--rule);padding:.35rem .7rem;text-align:left}\n\
         th{background:var(--head)}\n\
         td:nth-child(n+2){text-align:right;font-variant-numeric:tabular-nums}\n\
         .files{color:var(--muted);font-size:.9em;list-style:none;padding-left:0}\n\
         code{background:var(--code);padding:.1em .3em;border-radius:3px}\n\
         .tabletitle{font-weight:600;margin:.5rem 0 .2rem}\n\
         .desc{margin:.5rem 0;max-width:48rem}\n\
         .note{border-left:4px solid var(--note-bar);background:var(--note-bg);margin:.6rem 0;\
         padding:.5rem .9rem}\n\
         </style>\n</head>\n<body>\n",
    );
    b.push_str(&format!("<h1>{}</h1>\n", esc(title)));
    for n in notes {
        b.push_str(&format!("<p class=\"note\">{}</p>\n", esc(n)));
    }
    for sec in sections {
        b.push_str(&format!("<h2>{}</h2>\n", esc(&sec.title)));
        if !sec.desc.is_empty() {
            b.push_str(&format!("<p class=\"desc\">{}</p>\n", code_spans(&esc(&sec.desc))));
        }
        if !sec.files.is_empty() {
            b.push_str("<ul class=\"files\">\n");
            for f in &sec.files {
                b.push_str(&format!("<li>File: <code>{}</code></li>\n", esc(f)));
            }
            b.push_str("</ul>\n");
        }
        for t in &sec.tables {
            if let Some(tt) = &t.title {
                b.push_str(&format!("<p class=\"tabletitle\">{}</p>\n", esc(tt)));
            }
            if !t.desc.is_empty() {
                b.push_str(&format!("<p class=\"desc\">{}</p>\n", code_spans(&esc(&t.desc))));
            }
            b.push_str("<div class=\"scroll\"><table>\n<thead><tr>");
            for h in &t.headers {
                b.push_str(&format!("<th>{}</th>", esc(h)));
            }
            b.push_str("</tr></thead>\n<tbody>\n");
            for row in &t.rows {
                b.push_str("<tr>");
                for c in row {
                    b.push_str(&format!("<td>{}</td>", esc(c)));
                }
                b.push_str("</tr>\n");
            }
            b.push_str("</tbody>\n</table></div>\n");
        }
    }
    b.push_str("</body>\n</html>\n");
    b
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Turn the prose's Markdown `code` spans into `<code>` for the HTML page.
fn code_spans(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut open = false;
    for ch in s.chars() {
        if ch == '`' {
            out.push_str(if open { "</code>" } else { "<code>" });
            open = !open;
        } else {
            out.push(ch);
        }
    }
    if open {
        out.push_str("</code>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_get_thousands_separators() {
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(999), "999");
        assert_eq!(fmt_int(1000), "1,000");
        assert_eq!(fmt_int(118_234_567), "118,234,567");
    }

    #[test]
    fn code_spans_become_html() {
        assert_eq!(code_spans("run `coast` now"), "run <code>coast</code> now");
    }
}
