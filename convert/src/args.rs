use std::path::PathBuf;

use clap::{error::ErrorKind, ArgAction, CommandFactory, Parser, ValueEnum};
use geolib::{
    csv::{CsvGeom, CsvSettings},
    format::ContentMode,
};

use crate::{
    io::{FileOnlyFormat, InputSpec, OutputSpec, StreamableFormat},
    stream::StreamKind,
};

/// Main CLI argument parser.
///
/// Is not a clap parser itself, but calls clap and layers additional parsing on top.
#[derive(Debug)]
pub struct Cli {
    pub input: InputSpec,
    pub output: OutputSpec,
    pub mode: ContentMode,
    pub quiet: QuietLevel,
    pub csv_settings: CsvSettings,
}

impl Cli {
    /// Parse from os args, exiting on error.
    pub fn parse() -> Self {
        let args = Args::parse();

        Self {
            mode: args.mode(),
            input: Self::parse_input("input", args.input, args.input_format),
            output: Self::parse_output("output", args.output, args.output_format),
            quiet: args.quiet.into(),
            csv_settings: CsvSettings {
                geom: args.csv_geom,
                delimiter: args.delimiter,
            },
        }
    }

    /// Parse input specification with validation.
    fn parse_input(
        kind: &'static str,
        stream: StreamKind,
        format: Option<InputFormat>,
    ) -> InputSpec {
        match (stream, format) {
            (StreamKind::StdIo, Some(format)) => match format {
                InputFormat::Shp | InputFormat::Kml | InputFormat::Kmz => {
                    Self::exit(
                        ErrorKind::ArgumentConflict,
                        format!("File-only format '{:?}' cannot read from stdin", format),
                    );
                }
                InputFormat::JsonStream => InputSpec::Streamable {
                    format: StreamableFormat::JsonStream,
                    stream: StreamKind::StdIo,
                },
                InputFormat::Ndjson => InputSpec::Streamable {
                    format: StreamableFormat::Ndjson,
                    stream: StreamKind::StdIo,
                },
                InputFormat::Csv => InputSpec::Streamable {
                    format: StreamableFormat::Csv,
                    stream: StreamKind::StdIo,
                },
            },
            (StreamKind::StdIo, None) => {
                Self::exit(
                    ErrorKind::ArgumentConflict,
                    "When using stdin, you must provide the input format using the -i flag",
                );
            }
            (StreamKind::File(path), None) => match InputFormat::try_from_path(&path) {
                Ok(format) => Self::build_input_spec(StreamKind::File(path), format),
                Err(_) => Self::exit(
                    ErrorKind::InvalidValue,
                    format!(
                        "File format for {} has a missing or invalid file extension",
                        kind
                    ),
                ),
            },
            (StreamKind::File(path), Some(format)) => {
                // Validate format matches file extension
                match InputFormat::try_from_path(&path) {
                    Ok(path_format) if path_format == format => {
                        Self::build_input_spec(StreamKind::File(path), format)
                    }
                    _ => Self::exit(
                        ErrorKind::ArgumentConflict,
                        format!(
                            "Format in {} filename '{}' doesn't match '{:?}' which was specified using the -i flag", 
                            kind, path.to_string_lossy(), format
                        ),
                    ),
                }
            }
            #[cfg(test)]
            _ => unreachable!(),
        }
    }

    /// Build InputSpec from InputFormat.
    fn build_input_spec(stream: StreamKind, format: InputFormat) -> InputSpec {
        match format {
            InputFormat::JsonStream => InputSpec::Streamable {
                format: StreamableFormat::JsonStream,
                stream,
            },
            InputFormat::Ndjson => InputSpec::Streamable {
                format: StreamableFormat::Ndjson,
                stream,
            },
            InputFormat::Csv => InputSpec::Streamable {
                format: StreamableFormat::Csv,
                stream,
            },
            InputFormat::Shp => InputSpec::FileOnly {
                format: FileOnlyFormat::Shp,
                path: match stream {
                    StreamKind::File(path) => path,
                    _ => unreachable!("File-only formats require file path"),
                },
            },
            InputFormat::Kml => InputSpec::FileOnly {
                format: FileOnlyFormat::Kml,
                path: match stream {
                    StreamKind::File(path) => path,
                    _ => unreachable!("File-only formats require file path"),
                },
            },
            InputFormat::Kmz => InputSpec::FileOnly {
                format: FileOnlyFormat::Kmz,
                path: match stream {
                    StreamKind::File(path) => path,
                    _ => unreachable!("File-only formats require file path"),
                },
            },
        }
    }

    /// Parse output specification with validation.
    fn parse_output(
        kind: &'static str,
        stream: StreamKind,
        format: Option<OutputFormat>,
    ) -> OutputSpec {
        match (stream, format) {
            (stream, Some(format)) => {
                let streamable_format = match format {
                    OutputFormat::JsonStream => StreamableFormat::JsonStream,
                    OutputFormat::Ndjson => StreamableFormat::Ndjson,
                    OutputFormat::Csv => StreamableFormat::Csv,
                };
                OutputSpec::new(streamable_format, stream)
            }
            (StreamKind::StdIo, None) => {
                Self::exit(
                    ErrorKind::ArgumentConflict,
                    "When using stdout, you must provide the output format using the -o flag",
                );
            }
            (StreamKind::File(path), None) => match OutputFormat::try_from_path(&path) {
                Ok(format) => {
                    let streamable_format = match format {
                        OutputFormat::JsonStream => StreamableFormat::JsonStream,
                        OutputFormat::Ndjson => StreamableFormat::Ndjson,
                        OutputFormat::Csv => StreamableFormat::Csv,
                    };
                    OutputSpec::new(streamable_format, StreamKind::File(path))
                }
                Err(err) => Self::exit(
                    ErrorKind::InvalidValue,
                    format!("File format for {}: {}", kind, err),
                ),
            },
            #[cfg(test)]
            _ => unreachable!(),
        }
    }

    fn exit(kind: ErrorKind, msg: impl std::fmt::Display) -> ! {
        Args::command().error(kind, msg).exit()
    }
}

/// Command line utility to convert between GIS encodings.
#[derive(Parser, Debug)]
struct Args {
    /// Input file path.
    #[arg(value_parser, default_value = "-")]
    input: StreamKind,

    /// Output file path.
    #[arg(value_parser, default_value = "-")]
    output: StreamKind,

    /// Select the type of the input format.
    #[arg(long = "input", short)]
    input_format: Option<InputFormat>,

    /// Select the type of the output format.
    #[arg(long = "output", short)]
    output_format: Option<OutputFormat>,

    /// Only output shapes, do not process any metadata.
    #[arg(long, short, conflicts_with = "meta")]
    shapes: bool,

    /// Only output metadata, do not process shapes.
    #[arg(long, short, conflicts_with = "shapes")]
    meta: bool,

    /// Override the delimiter for csv processing. Must be single ASCII character.
    #[arg(short, long, default_value = ",", value_parser = Self::parse_delimiter)]
    delimiter: u8,

    /// Determine the type and name of the geometry input or output columns for CSV.
    /// TODO: Document this
    #[arg(long, short = 'g', default_value = "wkt")]
    csv_geom: CsvGeom,

    /// Run in quiet mode. No errors or messages will be emitted to stdout.
    #[arg(
        short,
        action = ArgAction::Count,
        value_parser = clap::value_parser!(u8).range(0..=2)
    )]
    quiet: u8,
}

impl Args {
    fn parse_delimiter(s: &str) -> Result<u8, String> {
        if s.len() != 1 {
            return Err("Delimiter must be a single character".to_string());
        }
        Ok(s.as_bytes()[0])
    }

    fn mode(&self) -> ContentMode {
        match (self.shapes, self.meta) {
            (false, false) => ContentMode::Full,
            (true, false) => ContentMode::Geometry,
            (false, true) => ContentMode::Properties,
            (true, true) => unreachable!("Clap enforces mutual exclusivity"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum QuietLevel {
    Normal,
    NoErrors,
    NoMessages,
}

impl From<u8> for QuietLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => QuietLevel::Normal,
            1 => QuietLevel::NoErrors,
            2 => QuietLevel::NoMessages,
            _ => unreachable!("Clap restricts this to 0–2"),
        }
    }
}

/// Input formats available for conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InputFormat {
    /// JSON.
    ///
    /// Uses geojson's permissive, streaming parser and therefore only works on FeatureCollections or arrays of Features
    /// at the top level.
    JsonStream,

    /// Newline delimited JSON.
    ///
    /// Streamable, with individual features separated by `\n` (input also supports \r\n`). Each underlying feature must
    /// be a valid geojson Feature, we do not support FeatureCollections or GeometryCollections for simplicity and
    /// compatibility.
    Ndjson,

    /// CSV.
    ///
    /// Streamable, with features mapping to individual rows in the csv. The csv format has further configuration
    /// options that are shared between inputs and outputs (we don't anticipate input and output formats being the
    /// same!). These are captured in the delimiter and csv-geom fields.
    ///
    /// When reading, input csv files/streams must have a header row to enable property keys - we do not support
    /// anonymous keys.
    Csv,

    /// Shapefile.
    ///
    /// Streamable, reading and writing can be done by feature. .dbf file contents are read into a common properties
    /// value format based on JSON, so some type fidelity will be lost, but fields will be converted to their nearest
    /// valid JSON type.
    Shp,

    /// KML, uncompressed.
    ///
    /// Not streamable, requires buffering and parsing the whole file in memory for input. KML is heirarchical, with
    /// geometries able to be at multiple levels, which we flatten in our processing.
    ///
    /// We attempt to extract property data from KML as follows, with all properties exracted as strings:
    ///
    /// - `name` and `description` elements in `Placemark` objects are extracted into properties.
    /// - Other element in `Placemark` objects are extracted with theit element name as the key and the content as the
    ///   value. We do not recurse into child-elements.
    /// - Nested `Folder` objects have their `name` and `description` recursively captured and added to an array. Each
    ///   geometry inside a folder has this array pushed onto its properties with deeper descendants at the top.
    Kml,

    /// KMZ, compressed KML.
    ///
    /// Not streamable, requires buffering and parsing the whole file in memory for input or output. See KML for futher
    /// notes on parsing.
    Kmz,
}

impl InputFormat {
    pub fn try_from_path(path: &PathBuf) -> Result<Self, &'static str> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") | Some("geojson") => Ok(InputFormat::JsonStream),
            Some("ndjson") => Ok(InputFormat::Ndjson),
            Some("csv") => Ok(InputFormat::Csv),
            Some("shp") => Ok(InputFormat::Shp),
            Some("kml") => Ok(InputFormat::Kml),
            Some("kmz") => Ok(InputFormat::Kmz),
            _ => Err("Unknown or missing file extension"),
        }
    }
}

/// Output formats available for conversion (streamable formats only)
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// JSON.
    ///
    /// Uses geojson's permissive, streaming parser and therefore only works on FeatureCollections or arrays of Features
    /// at the top level.
    JsonStream,

    /// Newline delimited JSON.
    ///
    /// Streamable, with individual features separated by `\n` (input also supports \r\n`). Each underlying feature must
    /// be a valid geojson Feature, we do not support FeatureCollections or GeometryCollections for simplicity and
    /// compatibility.
    Ndjson,

    /// CSV.
    ///
    /// Streamable, with features mapping to individual rows in the csv. The csv format has further configuration
    /// options that are shared between inputs and outputs (we don't anticipate input and output formats being the
    /// same!). These are captured in the delimiter and csv-geom fields.
    ///
    /// When writing, the transformer is permissive, with headers defined by the fields in the first row. If an
    /// output field is missing in the properties it is serialized as an empty string. Extra ouput fields not present
    /// in the headers are ignored.
    Csv,
}

impl OutputFormat {
    pub fn try_from_path(path: &PathBuf) -> Result<Self, &'static str> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") | Some("geojson") => Ok(OutputFormat::JsonStream),
            Some("ndjson") => Ok(OutputFormat::Ndjson),
            Some("csv") => Ok(OutputFormat::Csv),
            Some("shp") | Some("kml") | Some("kmz") => {
                Err("File-only format not supported for output")
            }
            _ => Err("Unknown or missing file extension"),
        }
    }
}
