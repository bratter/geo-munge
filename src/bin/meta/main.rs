mod args;
mod shapefile;

use clap::Parser;

use crate::args::{Cli, Command};
use crate::shapefile::ShapefileMeta;

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
