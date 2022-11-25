mod shapefile;

use clap::{Parser, Subcommand};

use crate::shapefile::ShapefileMeta;

/// We look in the current directory for a data.shp file by default
const DEFAULT_SHP_PATH: &str = "./data.shp";

/// Command line utility to extract metadata and properties from geospatial
/// file formats and convert to a flatfile. Because this is a flatten
/// operation, it may not capture all data for complex cases.
#[derive(Parser, Debug)]
struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// The shapefile to read from. If not provided will use
    /// {n}the default at ./data.shp.
    #[arg(global = true, default_value = DEFAULT_SHP_PATH)]
    pub shp: std::path::PathBuf,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print any file header contents to stdout.
    Header,

    /// Print the number of shapes to stdout.
    Count,

    /// Print the metadata field names to stdout.
    Fields {
        /// Print a representation of the field's type with the field.
        /// Only works when --fields is also passed.
        #[arg(long)]
        types: bool,
    },

    /// Print the first level of any metadata to stdout in csv format.
    Data {
        /// Add a header row to the output data
        #[arg(long, short = 'r')]
        headers: bool,

        /// Replace the standard delimiter ',' with an alternative character
        #[arg(long, short = 'l', default_value = ",")]
        delimiter: String,

        /// Skip s 0-indexed records from the beginning
        #[arg(long, short, default_value = "0")]
        start: usize,

        /// Take only the first n records
        #[arg(long, short = 'n')]
        length: Option<usize>,

        /// Add a sequaential index field to each record
        #[arg(short, long)]
        index: bool,
    },
}

type MetaResult = Result<(), Box<dyn std::error::Error>>;

struct DataOpts {
    pub headers: bool,
    pub delimiter: String,
    pub start: usize,
    pub length: Option<usize>,
    pub index: bool,
}

trait Meta {
    fn headers(&self) -> MetaResult;

    fn fields(&self, show_types: bool) -> MetaResult;

    fn count(&self) -> MetaResult;

    fn data(&self, opts: DataOpts) -> MetaResult;
}

// TODO: Meta should be a match on the filetype that returns a dyn Meta
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    // Load the shapefile, exiting with an error if the file cannot read
    let meta = ShapefileMeta::new(args.shp.clone());

    match args.command {
        Command::Header => meta.headers(),
        Command::Count => meta.count(),
        Command::Fields { types } => meta.fields(types),
        Command::Data {
            headers,
            delimiter,
            start,
            length,
            index,
        } => meta.data(DataOpts {
            headers,
            delimiter,
            start,
            length,
            index,
        }),
    }
}
