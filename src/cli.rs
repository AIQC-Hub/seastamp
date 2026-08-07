use clap::builder::{PossibleValue, TypedValueParser};
use clap::{Args, Parser, Subcommand, ValueEnum, ValueHint};
use std::ffi::OsStr;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "seastamp",
    version,
    about = "Stamp longitude/latitude points with sea attributes"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Distance to the nearest coast (GSHHG shorelines)
    Coast(CoastArgs),
    /// Bathymetric depth at each point (GEBCO grid)
    #[command(after_help = "The grid lookup always runs on one thread, so --threads \
does not speed it up. HDF5, which the NetCDF reader sits on, is often built \
without thread safety, and such a build cannot be read from several threads at \
once. Nothing is lost: the reads were serialized anyway.")]
    Depth(DepthArgs),
    /// Sea / ocean name at each point (IHO Sea Areas)
    Sea(SeaArgs),
    /// Nearest country and municipality (Natural Earth + GISCO)
    Place(PlaceArgs),
    /// Nearest location in a second table, with its distance
    Nearest(NearestArgs),
    /// List sea and ocean bounding boxes (region presets, or an IHO Sea Areas file)
    #[command(after_help = "Without --data this lists the built-in --region presets. With it, \
every named area in an IHO Sea Areas file is reduced to one bounding box, which is how you find \
a region for data outside the presets.\n\nA listed box is a crop box: widen it to taste, and \
remember that for `coast` it also sets where distances are measured from, so a very large sea \
makes a poor region. An area whose box crosses the antimeridian is flagged and has min_lon \
greater than max_lon; seastamp cannot take such a box, so run it as two.")]
    Regions(RegionsArgs),
    /// Print a shell completion script to stdout
    #[command(after_help = "The script completes subcommands, flags, their enumerated values, \
and the names --region accepts. Write it somewhere your shell reads at startup:\n\n  \
bash:  seastamp completions bash > ~/.local/share/bash-completion/completions/seastamp\n  \
zsh:   seastamp completions zsh > ~/.zfunc/_seastamp        (with ~/.zfunc on $fpath)\n  \
fish:  seastamp completions fish > ~/.config/fish/completions/seastamp.fish\n\n\
Regenerate it after upgrading seastamp: the script is a snapshot of this version's CLI.\n\n\
One rough edge, in bash and zsh only: a region name containing a space completes to its \
first word, so 'Barentsz Sea' stops at 'Barentsz'. Running that reports the full name back \
at you, so finish it by hand or quote it. fish completes such names whole.")]
    Completions(CompletionsArgs),
}

/// The `completions` generator. It runs no pipeline and reads no data: it walks
/// the clap command tree defined in this file and writes a script for one shell.
#[derive(Args, Debug)]
pub struct CompletionsArgs {
    /// Shell to generate the script for
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

/// Input / output tabular format. `Auto` infers from the file extension and
/// falls back to Parquet when the extension is unknown.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Auto,
    Parquet,
    Csv,
    Tsv,
    #[value(name = "csv.gz")]
    CsvGz,
    #[value(name = "tsv.gz")]
    TsvGz,
}

/// Unit for a distance-valued output column.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistUnit {
    Km,
    M,
}

/// Options every module shares: input, output, format, coordinate columns, and
/// the rounding/threading knobs that drive de-duplication and parallelism.
#[derive(Args, Debug)]
pub struct CommonArgs {
    /// Input file (parquet, csv, tsv, csv.gz, tsv.gz)
    #[arg(value_hint = ValueHint::FilePath)]
    pub input: PathBuf,

    /// Output file (default: <input stem>.<module>.<input format> beside the input)
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,

    /// TOML config file. CLI flags override individual fields.
    #[arg(short = 'c', long, value_hint = ValueHint::FilePath)]
    pub config: Option<PathBuf>,

    /// Input format (default: inferred from the extension, else parquet)
    #[arg(long, value_enum, default_value_t = Format::Auto)]
    pub in_format: Format,

    /// Output format (default: inferred from --output, else parquet)
    #[arg(long, value_enum, default_value_t = Format::Auto)]
    pub out_format: Format,

    /// Overwrite input columns that clash with the output columns
    /// (default: a clashing column is an error)
    #[arg(long)]
    pub overwrite: bool,

    /// Longitude column name
    #[arg(long, default_value = "longitude")]
    pub lon_col: String,

    /// Latitude column name
    #[arg(long, default_value = "latitude")]
    pub lat_col: String,

    /// Decimal places longitude/latitude are rounded to before de-duplicating
    #[arg(long, default_value_t = 3)]
    pub decimals: u32,

    /// Worker threads (default: all logical cores)
    #[arg(short = 't', long)]
    pub threads: Option<usize>,
}

/// Every name `--region` understands: `auto`, then the presets, then the IHO
/// Sea Areas. The antimeridian crossers are in the list even though they cannot
/// serve as a crop box, because the error they raise names the workaround, and
/// that is more use than a name that mysteriously refuses to complete.
pub fn region_names() -> impl Iterator<Item = &'static str> {
    std::iter::once(crate::config::AUTO_REGION)
        .chain(crate::config::PRESET_NAMES)
        .chain(crate::config::IHO_AREAS.iter().map(|a| a.name))
}

/// A `--region` parser that takes any string but advertises every known name,
/// which is what shell completion offers.
///
/// Staying permissive is deliberate. [`crate::config::region_bbox`] is the one
/// place that judges a region name, and its word-level "did you mean" is better
/// than what clap would produce here. Rejecting at the parser would split one
/// mistake across two different messages, and would still miss a bad region set
/// in a TOML config, which never reaches clap at all.
#[derive(Clone, Copy, Debug)]
struct RegionNameParser;

impl TypedValueParser for RegionNameParser {
    type Value = String;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        Ok(value.to_string_lossy().into_owned())
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(region_names().map(PossibleValue::new)))
    }
}

/// Region controls shared by the modules that need a bounding box (to crop the
/// reference data) and a projection center (for planar distances). Defaults come
/// from the resolved config; a named `--region` preset sets both at once.
#[derive(Args, Debug)]
pub struct RegionArgs {
    /// Region: "auto" (the default, derived from your points), a preset
    /// (global, baltic, norway, arctic, atlantic, europe, mediterranean), or an
    /// IHO Sea Areas name such as "Barentsz Sea" (the IHO spelling, which is
    /// not always the everyday one). Run `seastamp regions` to list every
    /// accepted name, or press Tab if completions are installed
    // The 109 names go to shell completion but are hidden from --help, where
    // they would bury every other flag.
    #[arg(long, value_parser = RegionNameParser, hide_possible_values = true)]
    pub region: Option<String>,

    /// Western bound of the reference-data crop box
    #[arg(long, allow_hyphen_values = true)]
    pub min_lon: Option<f64>,
    /// Eastern bound of the reference-data crop box
    #[arg(long, allow_hyphen_values = true)]
    pub max_lon: Option<f64>,
    /// Southern bound of the reference-data crop box
    #[arg(long, allow_hyphen_values = true)]
    pub min_lat: Option<f64>,
    /// Northern bound of the reference-data crop box
    #[arg(long, allow_hyphen_values = true)]
    pub max_lat: Option<f64>,

    /// Longitude of the LAEA projection center (default: region center)
    #[arg(long, allow_hyphen_values = true)]
    pub proj_lon0: Option<f64>,
    /// Latitude of the LAEA projection center (default: region center)
    #[arg(long, allow_hyphen_values = true)]
    pub proj_lat0: Option<f64>,
}

#[derive(Args, Debug)]
pub struct CoastArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[command(flatten)]
    pub region: RegionArgs,

    /// Directory of GSHHG shapefiles (resolution 'f' recommended)
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub data: Option<PathBuf>,

    /// Distance unit for the output column
    #[arg(long, value_enum, default_value_t = DistUnit::Km)]
    pub unit: DistUnit,

    /// Output column name
    #[arg(long, default_value = "dist_to_coast")]
    pub column: String,
}

#[derive(Args, Debug)]
pub struct DepthArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// GEBCO bathymetry NetCDF file
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub data: Option<PathBuf>,

    /// Report depth as positive below sea level (negate GEBCO elevation, which is
    /// negative under water); land then reads negative
    #[arg(long)]
    pub positive: bool,

    /// Append an `on_land` boolean column, true where the GEBCO elevation is at or
    /// above sea level, so land points are flagged rather than only inferable from
    /// the sign. The depth value itself is reported either way
    #[arg(long)]
    pub on_land: bool,

    /// Output column name
    #[arg(long, default_value = "bathymetry")]
    pub column: String,
}

#[derive(Args, Debug)]
pub struct SeaArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[command(flatten)]
    pub region: RegionArgs,

    /// IHO Sea Areas polygons (GeoJSON or shapefile)
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub data: Option<PathBuf>,

    /// Property / attribute field holding the sea name
    #[arg(long, default_value = "NAME")]
    pub name_field: String,

    /// Output column name
    #[arg(long, default_value = "sea_name")]
    pub column: String,
}

#[derive(Args, Debug)]
pub struct PlaceArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    #[command(flatten)]
    pub region: RegionArgs,

    /// Natural Earth countries (shapefile) for the nearest-country lookup
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub countries: Option<PathBuf>,

    /// GISCO LAU municipalities (shapefile) for the nearest-municipality lookup
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub municipalities: Option<PathBuf>,

    /// Drop the municipality when the nearest one is further away than this, in
    /// --unit. The match is otherwise unbounded, so a site outside the coverage
    /// (GISCO LAU is Europe only) is assigned however distant a municipality
    #[arg(long)]
    pub max_municipality_dist: Option<f64>,

    /// Distance unit for the municipality_dist column and --max-municipality-dist
    #[arg(long, value_enum, default_value_t = DistUnit::Km)]
    pub unit: DistUnit,
}

/// The `regions` listing. It takes no input table and no region of its own: it
/// is where regions come from, so [`CommonArgs`] and [`RegionArgs`] would both
/// be meaningless here.
#[derive(Args, Debug)]
pub struct RegionsArgs {
    /// IHO Sea Areas polygons (GeoJSON or shapefile), or any named polygon
    /// layer. Without it, the built-in --region presets are listed instead
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub data: Option<PathBuf>,

    /// Property / attribute field holding the area name
    #[arg(long, default_value = "NAME")]
    pub name_field: String,

    /// Keep only areas whose name contains this text (case-insensitive)
    #[arg(long)]
    pub name: Option<String>,

    /// Also write the list to a file (parquet, csv, tsv, csv.gz, tsv.gz)
    #[arg(short, long, value_hint = ValueHint::FilePath)]
    pub output: Option<PathBuf>,

    /// Output format (default: inferred from --output, else parquet)
    #[arg(long, value_enum, default_value_t = Format::Auto)]
    pub out_format: Format,

    /// Do not print the table, only write it (use with --output)
    #[arg(short, long)]
    pub quiet: bool,
}

#[derive(Args, Debug)]
pub struct NearestArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Reference table: the second set of locations to measure the distance to
    /// (any tabular format, same as the input)
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub to: PathBuf,

    /// Format of the reference table (default: inferred from the extension)
    #[arg(long, value_enum, default_value_t = Format::Auto)]
    pub to_format: Format,

    /// Longitude column in the reference table
    #[arg(long, default_value = "longitude")]
    pub to_lon_col: String,

    /// Latitude column in the reference table
    #[arg(long, default_value = "latitude")]
    pub to_lat_col: String,

    /// Column in the reference table holding each location's name
    #[arg(long, default_value = "name")]
    pub name_field: String,

    /// Distance unit for the output distance column
    #[arg(long, value_enum, default_value_t = DistUnit::Km)]
    pub unit: DistUnit,

    /// Output column for the nearest location's name
    #[arg(long, default_value = "nearest_name")]
    pub name_column: String,

    /// Output column for the distance to the nearest location
    #[arg(long, default_value = "nearest_dist")]
    pub dist_column: String,
}
