use std::path::PathBuf;

use clap::{error::ErrorKind, ArgAction, CommandFactory, Parser, ValueEnum};

use crate::{
    format::{
        csv::{CsvGeom, CsvSettings},
        *,
    },
    stream::StreamKind,
    QuietLevel,
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
                delimiter: args.csv_delimiter,
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
                format => Self::build_input_spec(StreamKind::StdIo, format),
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
                    // Special affordance for the two json variants
                    // TODO: Should we either abandon this entirely, or set a default
                    Ok(path_format) if path_format.is_json() && format.is_json() => {
                        Self::build_input_spec(StreamKind::File(path), format)
                    }
                    _ => Self::exit(
                        ErrorKind::ArgumentConflict,
                        format!(
                            "Format in {} filename '{}' doesn't match '{:?}' which was specified using the -I flag", 
                            kind, path.to_string_lossy(), format
                        ),
                    ),
                }
            }
            #[cfg(test)]
            _ => unreachable!(),
        }
    }

    fn build_input_spec(stream: StreamKind, format: InputFormat) -> InputSpec {
        match format {
            InputFormat::Json => InputSpec::Streamable {
                format: StreamableFormat::JsonStream,
                stream,
            },
            InputFormat::JsonString => InputSpec::Streamable {
                format: StreamableFormat::JsonString,
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
                    OutputFormat::Json => StreamableFormat::JsonStream,
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
                        OutputFormat::Json => StreamableFormat::JsonStream,
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
    /// Input file path, will use stdin when not provided.
    #[arg(long, short, value_parser, default_value = "-")]
    input: StreamKind,

    /// Output file path, will use stdout when not provided.
    #[arg(long, short, value_parser, default_value = "-")]
    output: StreamKind,

    /// Select the type of the input format.
    #[arg(long, short = 'I')]
    input_format: Option<InputFormat>,

    /// Select the type of the output format.
    #[arg(long, short = 'O')]
    output_format: Option<OutputFormat>,

    /// Only output shapes, do not process any properties.
    #[arg(long, conflicts_with = "properties")]
    geometries: bool,

    /// Only output metadata, do not process shapes.
    #[arg(long, alias = "props", conflicts_with = "geometries")]
    properties: bool,

    /// Override the delimiter for csv processing. Must be single ASCII character.
    #[arg(long, default_value = ",", value_parser = Self::parse_delimiter)]
    csv_delimiter: u8,

    /// Determine the type and name of the geometry input or output columns for CSV.
    ///
    /// By default, this assumes that the csv contains a column labelled 'geom' that contains WKT encoded geometries.
    ///
    /// To override this, first pass the name of the format. Supported formats are: 'wkt', 'wkb', 'json', and 'pt'. The
    /// first three expect a single column in the appropriate format, with a column name that defaults to 'geom'. To
    /// override the column name, pass an alternative separated by a comma, e.g., 'json,my_field'. The 'pt' option
    /// expects two columns each contains numbers in decimal degrees. The default lolmn names are 'lng' and 'lat'. The
    /// override, pass both separated by commas, e.g., 'pt,x,y'. The x-value name must be first.
    #[arg(long, default_value = "wkt")]
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
        match (self.geometries, self.properties) {
            (false, false) => ContentMode::Full,
            (true, false) => ContentMode::Geometry,
            (false, true) => ContentMode::Properties,
            (true, true) => unreachable!("Clap enforces mutual exclusivity"),
        }
    }
}

/// Input formats available for conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InputFormat {
    /// JSON Stream (streamed, file or pipe).
    ///
    /// Uses geojson's permissive, streaming parser and therefore only works on FeatureCollections or arrays of Features
    /// at the top level.
    Json,

    /// JSON String (fully loaded, file or pipe).
    ///
    /// Loads and parses the entire file or stream as a string, requiring additional memory and overhead, but enforces
    /// proper geojson and works for inputs other than FeatureCollection.
    JsonString,

    /// Newline delimited JSON (streamed, file or pipe).
    ///
    /// Streamable, with individual features separated by `\n` (input also supports \r\n`). Each underlying feature must
    /// be a valid geojson Feature, we do not support FeatureCollections or GeometryCollections for simplicity and
    /// compatibility.
    Ndjson,

    /// CSV (streamed, file or pipe).
    ///
    /// Streamable, with features mapping to individual rows in the csv. The csv format has further configuration
    /// options that are shared between inputs and outputs (we don't anticipate input and output formats being the
    /// same!). These are captured in the delimiter and csv-geom fields.
    ///
    /// When reading, input csv files/streams must have a header row to enable property keys - we do not support
    /// anonymous keys.
    Csv,

    /// Shapefile (streamed, file only).
    ///
    /// Streamable, reading and writing can be done by feature. .dbf file contents are read into a common properties
    /// value format based on JSON, so some type fidelity will be lost, but fields will be converted to their nearest
    /// valid JSON type.
    Shp,

    /// KML, uncompressed (fully loaded, file only).
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

    /// KMZ, compressed KML (fully loaded, file only).
    ///
    /// Not streamable, requires buffering and parsing the whole file in memory for input or output. See KML for futher
    /// notes on parsing.
    Kmz,
}

impl InputFormat {
    pub fn try_from_path(path: &PathBuf) -> Result<Self, &'static str> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") | Some("geojson") => Ok(InputFormat::Json),
            Some("ndjson") => Ok(InputFormat::Ndjson),
            Some("csv") => Ok(InputFormat::Csv),
            Some("shp") => Ok(InputFormat::Shp),
            Some("kml") => Ok(InputFormat::Kml),
            Some("kmz") => Ok(InputFormat::Kmz),
            _ => Err("Unknown or missing file extension"),
        }
    }

    fn is_json(&self) -> bool {
        match self {
            Self::Json | Self::JsonString => true,
            _ => false,
        }
    }
}

/// Output formats available for conversion (streamable formats only)
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// JSON.
    ///
    /// Streams out as bytes. Wraps the output stream of Features in a FeatureCollection.
    Json,

    /// Newline delimited JSON.
    ///
    /// Streams individual Features separated by `\n` (input also supports \r\n`). Each underlying feature is a valid
    /// geojson Feature.
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
            Some("json") | Some("geojson") => Ok(OutputFormat::Json),
            Some("ndjson") => Ok(OutputFormat::Ndjson),
            Some("csv") => Ok(OutputFormat::Csv),
            Some("shp") | Some("kml") | Some("kmz") => {
                Err("File-only format not supported for output")
            }
            _ => Err("Unknown or missing file extension"),
        }
    }
}
