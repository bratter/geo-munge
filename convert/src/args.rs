use clap::{error::ErrorKind, ArgAction, CommandFactory, Parser};
use geolib::{
    csv::{CsvGeom, CsvSettings},
    format::{ContentMode, Format},
};

use crate::{io::IO, stream::StreamKind};

/// Main CLI argument parser.
///
/// Is not a clap parser itself, but calls clap and layers additional parsing on top.
#[derive(Debug)]
pub struct Cli {
    pub input: IO,
    pub output: IO,
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
            input: Self::parse_io("input", args.input, args.input_format),
            output: Self::parse_io("output", args.output, args.output_format),
            quiet: args.quiet.into(),
            csv_settings: CsvSettings {
                geom: args.csv_geom,
                delimiter: args.delimiter,
            },
        }
    }

    /// Conduct argument validation that can't be done inside the clap instance.
    fn parse_io(kind: &'static str, stream: StreamKind, format: Option<Format>) -> IO {
        match (stream, format) {
            (stream @ StreamKind::StdIo, Some(format)) => IO::new(stream, format),
            (StreamKind::StdIo, None) => {
                let io = if kind == "input" { "StdIn" } else { "StdOut" };
                Self::exit(
                    ErrorKind::ArgumentConflict,
                    format!(
                        "When using {}, you must provide the {} format using the -i flag",
                        io, kind
                    ),
                );
            }
            (StreamKind::File(file), None) => {
                if let Ok(format) = Format::try_from(&file) {
                    IO::new(StreamKind::File(file), format)
                } else {
                    Self::exit(
                        ErrorKind::InvalidValue,
                        format!(
                            "File format for {} has a missing or invalid file extension",
                            kind
                        ),
                    );
                }
            }
            (StreamKind::File(file), Some(format)) => {
                // Type erase the Error that doesn't impl PartialEq
                if Format::try_from(&file).ok() == Some(format) {
                    IO::new(StreamKind::File(file), format)
                } else {
                    Self::exit(
                        ErrorKind::ArgumentConflict,
                        format!(
                            "Format in {} filename '{}' doesn't match '{}' which was specified using the -i flag", 
                            kind, file.to_string_lossy(), format
                        ),
                    );
                }
            } // Wildcard covers testing permutations
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
    input_format: Option<Format>,

    /// Select the type of the output format.
    #[arg(long = "output", short)]
    output_format: Option<Format>,

    /// Only output shapes, do not process any metadata.
    #[arg(long, short, conflicts_with = "meta")]
    shapes: bool,

    /// Only output metadata, do not process shapes.
    #[arg(long, short, conflicts_with = "shapes")]
    meta: bool,

    /// Override the delimiter for csv output. Must be single ASCII character.
    #[arg(short, long, default_value = ",", value_parser = Self::parse_delimiter)]
    delimiter: u8,

    /// Determine the type and name of the geometry input or output columns for CSV.
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
