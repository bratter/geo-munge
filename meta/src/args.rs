use clap::{Parser, Subcommand};

/// We look in the current directory for a data.shp file by default
const DEFAULT_SHP_PATH: &str = "./data.shp";

/// Command line utility to extract metadata and properties from geospatial
/// file formats and convert to a flatfile. Because this is a flatten
/// operation, it may not capture all data for complex cases.
#[derive(Parser, Debug)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// The shapefile to read from. If not provided will use
    /// {n}the default at ./data.shp.
    #[arg(global = true, default_value = DEFAULT_SHP_PATH)]
    pub path: std::path::PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Print any file header contents to stdout.
    Header,

    /// Print the number of shapes to stdout.
    Count,

    /// Print metadata field names to stdout. Depending on the input
    /// {n}format, fields may not be easily represented. This command
    /// {n}makes a best effort only and is not guaranteed to be
    /// {n}complete.
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

        /// Add a sequential index field to each record
        #[arg(short, long)]
        index: bool,
    },
}
