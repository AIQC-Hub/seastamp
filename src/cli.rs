use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "seastamp",
    version,
    about = "Enrich longitude/latitude points with geospatial attributes"
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
    pub input: PathBuf,

    /// Output file (default: <input stem>.<module>.<input format> beside the input)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// TOML config file. CLI flags override individual fields.
    #[arg(short = 'c', long)]
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

/// Region controls shared by the modules that need a bounding box (to crop the
/// reference data) and a projection center (for planar distances). Defaults come
/// from the resolved config; a named `--region` preset sets both at once.
#[derive(Args, Debug)]
pub struct RegionArgs {
    /// Named region preset: global (default), baltic, norway, arctic, atlantic,
    /// europe, mediterranean
    #[arg(long)]
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
    #[arg(long)]
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
    #[arg(long)]
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
    #[arg(long)]
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
    #[arg(long)]
    pub countries: Option<PathBuf>,

    /// GISCO LAU municipalities (shapefile) for the nearest-municipality lookup
    #[arg(long)]
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

#[derive(Args, Debug)]
pub struct NearestArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Reference table: the second set of locations to measure the distance to
    /// (any tabular format, same as the input)
    #[arg(long)]
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
