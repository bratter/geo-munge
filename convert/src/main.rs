/// Geo-munge convert binary.
///
/// Convert between basic GIS formats, optionally preserving metadata.
mod args;

use std::{
    borrow::Cow,
    io::{stdout, Write},
    path::PathBuf,
};

use args::Cli;
use clap::ValueEnum;
use geo::Geometry;

use geolib::{
    format::{FormatReader, FormatWriter, GeoItemIterator, Mode, Writer},
    geojson::{JsonReader, JsonWriter, NdjsonWriter},
};

// TODO: Work out how this is going to work
// - What is the full list of formats?
//   And which formats support buffer-based/incremental parsing vs having to load the whole file
//   Do we want to support FeatureCollections in anything other than the outermost type?
// - Can conversion just work with geozero?
// - Should there be a trait that manages all the main methods?
// - Where should this be implemented? In geolib?
// - What should the methods be on the reader?
//      - iter: iterates through shapes and metadata
//      - iter_shapes: iterates through shapes only
//      - iter_meta: iterates through the metadata
// - What should the methods be on the writer?
//      - Does it need anything other than write?
// - Should we attempt to stream everything, so we don't have to worry about memory?
//   But then how to manage things like shapefile output when the metafields or shape type changes? Just error?
//   Perhaps there can be an option that buffers a certain amount of data?
//   Also how to manage what gets emitted per iteration? Ideally a complete shape so we can leverage it elsewhere
// - Can we preserve some form of id?
// - If yes do we need to capture it in the CLI input?
// - Should we have a flatten option that just flattens nexted geoms or collections if it needs to?
// - Probably needs some form of permissiveness control that decides when to abort vs log an issue

/// Available GIS formats for input and output
/// TODO: Do we want to add csv - it would be wkt in the geometry column
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Shapefile.
    ///
    /// Streamable, reading and writing can be done by feature.
    /// TODO: Check that Stdout can actually stream
    Shp,

    /// JSON.
    ///
    /// Not streamable, requires buffering the whole file in memory for input or output. Only supports
    /// TODO: Support for collections outside of the top level
    Json,

    /// Newline delimited JSON.
    ///
    /// Streamable, with individual features separated by `\n` (input also supports \r\n`). Each underlying feature must
    /// be a valid geojson Feature, we do not support FeatureCollections or GeometryCollections for simplicity and
    /// compatibility.
    Ndjson,

    /// KML, uncompressed.
    ///
    /// Not streamable, requires buffering the whole file in memory for input or output.
    /// TODO: For KML and KMZ, should we de-nest, or just error out if too complex?
    Kml,

    /// KMZ, compressed KML.
    ///
    /// Not streamable, requires buffering the whole file in memory for input or output.
    Kmz,

    // TODO: WKB and WKT
    // They are both technically streamable as it is easy to parse a termination point
    // Same comment that we won't parse a collection unless it is the first feature
    Wkb,
    Wkt,
}

impl Format {
    pub fn create_writer<I>(
        &self,
        iter: I,
        mode: Mode,
    ) -> impl Iterator<Item = anyhow::Result<impl AsRef<[u8]>>>
    where
        I: GeoItemIterator,
    {
        match self {
            Self::Json => Writer::Json(JsonWriter::new(iter, mode)),
            Self::Ndjson => Writer::Ndjson(NdjsonWriter::new(iter, mode)),
            _ => todo!(),
        }
    }
}

impl TryFrom<&PathBuf> for Format {
    // TODO: Error format type
    type Error = &'static str;

    fn try_from(path: &PathBuf) -> Result<Self, Self::Error> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("shp") => Ok(Format::Shp),
            Some("json") | Some("geojson") => Ok(Format::Json),
            Some("ndjson") => Ok(Format::Ndjson),
            Some("kml") => Ok(Format::Kml),
            Some("kmz") => Ok(Format::Kmz),
            Some("wkb") => Ok(Format::Wkb),
            Some("wkt") => Ok(Format::Wkt),
            _ => Err("Unknown format"),
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Shp => "shp",
            Self::Json => "json",
            Self::Ndjson => "ndjson",
            Self::Kml => "kml",
            Self::Kmz => "kmz",
            Self::Wkb => "wkb",
            Self::Wkt => "wkt",
        };

        f.write_str(name)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    println!("Parsed CLI arguments");
    println!("{args:?}");

    // Prepare the reader and writer
    // TODO: The reader and writer will be a BufReader and BufWriter around the file or stdio
    // Then the reader should also return the appropriate read and transform instance depending on the metadata state
    // and potentially the flattening algorithm to apply, although we may for simplicty do the processing in the
    // iterator chain.
    let s = r#"
      {
        type: "FeatureCollection",
        features: [
          {
            "type": "Feature",
            "geometry": {
              "type": "Point",
              "coordinates": [1.1, 1.2]
            },
            "properties": { "x": 1 }
          },
          {
            "type": "Feature",
            "geometry": {
              "type": "Point",
              "coordinates": [2.1, 2.2]
            },
            "properties": { }
          }
        ]
      }
    "#;

    // TODO: This should potentially flatten if not done in the reader, then write to the writer
    // Should the writer come with mirroring methods to the reader? Should the writer serialize only or actually write?
    // How to deal with writers that don't stream? Do they just wait until the final flush?
    // What to do about errors? Depends on the mode?
    // TODO: We want to be able to do an "and_then" which is easy enough to do in an iterator's map, and could also
    // be implemented in a custom trait with a blanket application
    // TODO: The writer should output something that is a Result<&[u8], E>?
    // TODO: Can work out the form of the transform chaining later, but the writer iterator has to take an iterator
    // of geoms and turns it into an iterator of Result<&[u8]>

    // Let's work out the meta mode
    // TODO: This should come straight from clap and needs to have all three options
    let mode = Mode::Full;

    let r2 = JsonReader::try_new(s.as_bytes()).unwrap();
    let writer = args.output.format.create_writer(r2.iter(), mode);

    for item in writer {
        stdout().write(&item?.as_ref())?;
    }

    // TODO: Better exit
    //res.map_err(|_| "exited with an error".into())
    Ok(())
}
