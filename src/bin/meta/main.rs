mod args;
mod geojson;
mod shapefile;
mod kml;

use std::path::PathBuf;

use clap::Parser;
use geo_munge::error::FiberError;

use crate::args::{Cli, Command};
use crate::shapefile::ShapefileMeta;
use crate::geojson::GeoJsonMeta;
use crate::kml::KmlMeta;

type MetaResult = Result<(), Box<dyn std::error::Error>>;

struct DataOpts {
    pub headers: bool,
    pub delimiter: String,
    pub start: usize,
    pub length: Option<usize>,
    pub index: bool,
}

trait Meta {
    /// Print header information to stdout.
    fn headers(&self) -> MetaResult;

    /// Print a list of metadata fields to stdout, with optional type if
    /// appropriate for the filetype.
    ///
    /// Depending on the input format, fields may be nested, not representable,
    /// or sparsely populated. This method makes a best effort only to print
    /// what it can in a flattened format, but makes no promises of being
    /// exhaustive.
    fn fields(&self, show_types: bool) -> MetaResult;

    /// Print the number of top-level records to stdout.
    fn count(&self) -> MetaResult;

    /// Print metadata in csv format for records, adapted as appropriate for the
    /// filetype and the passed options.
    fn data(&self, opts: DataOpts) -> MetaResult;
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    // Load the appropriate meta based on the incoming file type
    let meta = get_meta_from_path(args.path)?;

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

fn get_meta_from_path(path: PathBuf) -> Result<Box<dyn Meta>, FiberError> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or(FiberError::IO("Cannot parse file extension"))?
    {
        "shp" => Ok(Box::new(ShapefileMeta::new(path))),
        "json" | "geojson" => Ok(Box::new(GeoJsonMeta::new(path))),
        "kml" | "kmz" => Ok(Box::new(KmlMeta::new(path))),
        _ => Err(FiberError::IO("Unsupported file type")),
    }
}
