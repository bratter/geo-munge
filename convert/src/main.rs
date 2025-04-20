//! Geo-munge convert binary.
//!
//! Convert between basic GIS formats, optionally preserving metadata.

mod args;

use std::{
    fs::File,
    io::{stderr, stdin, stdout, BufRead, BufReader, Read, Stdin, Stdout, Write},
    path::PathBuf,
    str::FromStr,
};

use anyhow::{anyhow, Error, Result};
use args::Cli;
use clap::ValueEnum;

use geolib::{
    format::{FormatReader, FormatTransformer, GeoItemIterator, Mode},
    geojson::{JsonReader, JsonTransformer, NdjsonReader, NdjsonTransformer},
};

#[cfg(test)]
use std::io::Cursor;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

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

#[derive(Debug, Clone)]
pub struct IO {
    pub stream: Stream,
    pub format: Format,
}

impl IO {
    pub fn new(stream: Stream, format: Format) -> Self {
        Self { stream, format }
    }

    #[cfg(test)]
    pub fn with_str(str: String, format: Format) -> Self {
        Self {
            stream: Stream::String(str),
            format,
        }
    }

    #[cfg(test)]
    pub fn with_output_str(str: Arc<Mutex<String>>, format: Format) -> Self {
        Self {
            stream: Stream::OutputString(str),
            format,
        }
    }

    pub fn create_reader(&self, mode: Mode) -> impl GeoItemIterator {
        let reader = InputStream::from(self.stream.clone());

        match self.format {
            Format::Json => FormatReader::Json(JsonReader::new(reader, mode)),
            Format::Ndjson => FormatReader::Ndjson(NdjsonReader::new(reader, mode)),
            _ => todo!(),
        }
    }

    // TODO: This should also take the error handling option - noting that it
    pub fn create_writer(
        &self,
        byte_iter: impl Iterator<Item = Result<impl AsRef<[u8]>>>,
        quiet: bool,
    ) -> impl Iterator<Item = Result<()>> {
        let mut writer = OutputStream::from(self.stream.clone());

        byte_iter.map(move |byte_result| match byte_result {
            Ok(bytes) => {
                writer.write(&bytes.as_ref())?;
                Ok(())
            }
            Err(e) => {
                if !quiet {
                    writeln!(stderr(), "{}", e)?;
                }
                Ok(())
            }
        })
    }
}

#[derive(Debug, Clone)]
pub enum Stream {
    StdIo,
    File(PathBuf),
    #[cfg(test)]
    String(String),

    /// Use an Arc<Mutex> to easily enable testing without using lifetimes (as non of the non-testing options need
    /// references). Cannot use Rc<RefCell> as this doesn't meeting parsing trait bounds.
    #[cfg(test)]
    OutputString(Arc<Mutex<String>>),
}

impl FromStr for Stream {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "-" {
            Ok(Self::StdIo)
        } else {
            Ok(Self::File(PathBuf::from(s)))
        }
    }
}

// TODO: Move this somewhere else, import the types
pub enum InputStream {
    Stdin(BufReader<Stdin>),
    File(BufReader<File>),
    #[cfg(test)]
    String(BufReader<Cursor<String>>),
}

impl InputStream {
    pub fn new(stream: Stream) -> Self {
        match stream {
            Stream::StdIo => InputStream::Stdin(BufReader::new(stdin())),
            Stream::File(path) => {
                // TODO: Make this fallible?
                let file = File::open(path).expect("Failed to open file");
                InputStream::File(BufReader::new(file))
            }
            #[cfg(test)]
            Stream::String(s) => InputStream::String(BufReader::new(Cursor::new(s))),
            #[cfg(test)]
            Stream::OutputString(_) => unreachable!("Should not be used"),
        }
    }
}

impl From<Stream> for InputStream {
    fn from(stream: Stream) -> Self {
        InputStream::new(stream)
    }
}

impl Read for InputStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            InputStream::Stdin(r) => r.read(buf),
            InputStream::File(r) => r.read(buf),
            #[cfg(test)]
            InputStream::String(r) => r.read(buf),
        }
    }
}

impl BufRead for InputStream {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        match self {
            InputStream::Stdin(r) => r.fill_buf(),
            InputStream::File(r) => r.fill_buf(),
            #[cfg(test)]
            InputStream::String(r) => r.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            InputStream::Stdin(r) => r.consume(amt),
            InputStream::File(r) => r.consume(amt),
            #[cfg(test)]
            InputStream::String(r) => r.consume(amt),
        }
    }
}

// TODO: Move this somewhere else, import the types
pub enum OutputStream {
    Stdout(Stdout),
    File(File),
    #[cfg(test)]
    String(Arc<Mutex<String>>),
}

impl OutputStream {
    pub fn new(stream: Stream) -> Self {
        match stream {
            Stream::StdIo => OutputStream::Stdout(stdout()),
            Stream::File(path) => {
                // TODO: Make this fallible?
                let file = File::open(path).expect("Failed to open file");
                OutputStream::File(file)
            }
            #[cfg(test)]
            Stream::String(_) => unimplemented!("Should not be used"),
            #[cfg(test)]
            Stream::OutputString(s) => OutputStream::String(s),
        }
    }
}

impl From<Stream> for OutputStream {
    fn from(stream: Stream) -> Self {
        OutputStream::new(stream)
    }
}

impl Write for OutputStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            OutputStream::Stdout(w) => w.write(buf),
            OutputStream::File(w) => w.write(buf),
            #[cfg(test)]
            OutputStream::String(s) => {
                let str = std::str::from_utf8(buf).expect("Valid utf8");
                s.lock().unwrap().push_str(str);
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            OutputStream::Stdout(w) => w.flush(),
            OutputStream::File(w) => w.flush(),
            // No-op
            #[cfg(test)]
            OutputStream::String(_) => Ok(()),
        }
    }
}

/// Available GIS formats for input and output
/// TODO: Do we want to add csv - it would be wkt in the geometry column
/// TODO: This needs to be moved somewhere else
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

// TODO: If format moves to geolib, then this should move there too - keep most of the detail of the writers out of
// convert main file.
impl Format {
    pub fn create_transformer<I>(
        &self,
        iter: I,
        mode: Mode,
    ) -> impl Iterator<Item = Result<impl AsRef<[u8]>>>
    where
        I: GeoItemIterator,
    {
        match self {
            Self::Json => FormatTransformer::Json(JsonTransformer::new(iter, mode)),
            Self::Ndjson => FormatTransformer::Ndjson(NdjsonTransformer::new(iter, mode)),
            _ => todo!(),
        }
    }
}

impl TryFrom<&PathBuf> for Format {
    type Error = Error;

    fn try_from(path: &PathBuf) -> Result<Self, Self::Error> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("shp") => Ok(Format::Shp),
            Some("json") | Some("geojson") => Ok(Format::Json),
            Some("ndjson") => Ok(Format::Ndjson),
            Some("kml") => Ok(Format::Kml),
            Some("kmz") => Ok(Format::Kmz),
            Some("wkb") => Ok(Format::Wkb),
            Some("wkt") => Ok(Format::Wkt),
            _ => Err(anyhow!("Unknown format")),
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

fn main() -> Result<()> {
    let args = Cli::parse();

    // TODO: Args needs to have a quiet option, also used in run
    if true {
        eprintln!(
            "Starting to process, converting {} to {}",
            args.input.format, args.output.format
        );
    }
    // TODO: Delete this when done testing
    eprintln!("{args:?}");

    // TODO: Better exit?
    let (ok_chunks, err_chunks) = run(args)?;

    // TODO: Args needs to have a quiet option, also used in run
    if true {
        eprintln!(
            "\nProcessing complete, emitted {} chunks with {} errors",
            ok_chunks, err_chunks
        );
        eprintln!("Note that chunks do not map 1:1 with emitted shapes",);
    }

    Ok(())
}

fn run(args: Cli) -> Result<(usize, usize)> {
    // TODO: Any other transforms, flattens, filters, etc. can be introduced in between the reader and the output
    // transformer as long as they are GeoItemIterators
    // TODO: This should have the option to flatten if not done in the reader
    // TODO: We also want a quiet mode that supresses errors as a clap option
    // TODO: Also want a seek setting to pre-pull fields for unstructured metadata formats like json

    // Let's work out the meta mode
    // TODO: This should come straight from clap and needs to have all three options
    let mode = Mode::Full;

    // Prepare the reader and writer
    let reader = args.input.create_reader(mode);
    let transformer = args.output.format.create_transformer(reader, mode);
    let writer = args.output.create_writer(transformer, false);

    // Drive the writer
    let mut ok_chunks = 0;
    let mut err_chunks = 0;
    for res in writer {
        match res {
            Ok(_) => ok_chunks += 1,
            Err(_) => err_chunks += 1,
        }
    }

    Ok((ok_chunks, err_chunks))
}

#[cfg(test)]
mod tests {
    use args::Meta;

    use super::*;

    const JSON: &'static str = r#"
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

    // TODO: Test other formats, and error cases
    #[test]
    fn convert_geojson_to_ndjson() {
        let output = Arc::new(Mutex::new(String::new()));
        let args = Cli {
            input: IO::with_str(JSON.to_string(), Format::Json),
            output: IO::with_output_str(output.clone(), Format::Ndjson),
            meta: Meta::Preserve,
        };

        let (ok_chunks, err_chunks) = run(args).expect("Run succeeded");
        assert_eq!(ok_chunks, 2);
        assert_eq!(err_chunks, 0);

        // TODO: Map this into geojson and check that it is right
        let count = output.lock().unwrap().split('\n').count();
        assert_eq!(count, 2);
    }
}
