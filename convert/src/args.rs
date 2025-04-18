use std::{path::PathBuf, str::FromStr};

use clap::{error::ErrorKind, CommandFactory, Parser, ValueEnum};

use crate::Format;

/// Main CLI argument parser.
///
/// IS not a clap parser itself, but calls clap and layers additional parsing on top.
#[derive(Debug)]
pub struct Cli {
    pub input: IO,
    pub output: IO,
    pub meta: Meta,
}

impl Cli {
    /// Parse from os args, exiting on error.
    pub fn parse() -> Self {
        let args = Args::parse();

        Self {
            input: Self::parse_io("input", args.input, args.input_format),
            output: Self::parse_io("output", args.output, args.output_format),
            meta: args.meta.into(),
        }
    }

    /// Conduct argument validation that can't be done inside the clap instance.
    fn parse_io(kind: &'static str, stream: Stream, format: Option<Format>) -> IO {
        match (stream, format) {
            (stream @ Stream::StdIo, Some(format)) => IO { stream, format },
            (Stream::StdIo, None) => {
                let io = if kind == "input" { "StdIn" } else { "StdOut" };
                Self::exit(
                    ErrorKind::ArgumentConflict,
                    format!(
                        "When using {}, you must provide the {} format using the -i flag",
                        io, kind
                    ),
                );
            }
            (Stream::File(file), None) => {
                if let Ok(format) = Format::try_from(&file) {
                    let stream = Stream::File(file);
                    IO { stream, format }
                } else {
                    Self::exit(
                        ErrorKind::InvalidValue,
                        format!(
                            "File format for {} has a missing or invalid file exension",
                            kind
                        ),
                    );
                }
            }
            (Stream::File(file), Some(format)) => {
                if Format::try_from(&file) == Ok(format) {
                    let stream = Stream::File(file);
                    IO { stream, format }
                } else {
                    Self::exit(
                        ErrorKind::ArgumentConflict,
                        format!(
                            "Format in {} filename '{}' doesn't match '{}' which was specified using the -i flag", 
                            kind, file.to_string_lossy(), format
                        ),
                    );
                }
            }
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
    pub input: Stream,

    /// Output file path.
    #[arg(value_parser, default_value = "-")]
    pub output: Stream,

    /// Select the type of the input format.
    #[arg(long = "input", short)]
    pub input_format: Option<Format>,

    /// Select the type of the output format.
    #[arg(long = "output", short)]
    pub output_format: Option<Format>,

    /// Preserve the metadata that comes with the input.
    #[arg(long, short)]
    pub meta: bool,
}

#[derive(Debug, Clone)]
pub struct IO {
    pub stream: Stream,
    pub format: Format,
}

#[derive(Debug, Clone)]
pub enum Stream {
    StdIo,
    File(PathBuf),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Meta {
    Discard,
    Preserve,
}

impl From<bool> for Meta {
    fn from(value: bool) -> Self {
        if value {
            Meta::Preserve
        } else {
            Meta::Discard
        }
    }
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
