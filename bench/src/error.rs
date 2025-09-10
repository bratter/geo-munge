use std::{fmt, path::PathBuf};

/// Custom error enum for emitting on failure.
///
/// Includes custom display, debug traits to produce human-readable error messages.
/// TODO: Remove this when revisiting bench crate
#[non_exhaustive]
pub enum Error {
    FileIOError(std::io::Error),
    CannotReadFile(PathBuf),
    CsvWriteError(csv::Error),
    ShapeFileWriteError(shapefile::Error),
    FailedToDeserialize(PathBuf, serde_json::Error),
    CannotFindCommand,
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileIOError(err) => {
                write!(f, "File IO error encountered; {}", err)
            }
            Self::CannotReadFile(path) => {
                write!(f, "Cannot read file at {}", path.to_string_lossy())
            }
            Self::CsvWriteError(err) => write!(f, "Error writing csv output: {}", err),
            Self::ShapeFileWriteError(err) => write!(f, "Error writing to shapefile: {}", err),
            Self::FailedToDeserialize(path, err) => write!(
                f,
                "Deserialization failed for file {}, error provided: {}",
                path.to_string_lossy(),
                err
            ),
            Self::CannotFindCommand => {
                write!(f, "Could not locate the proximity command for execution")
            }
        }
    }
}

// Custom debug implementation that delegates to Display
// This is then written on termination by the default Termination
// implementation
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::FileIOError(value)
    }
}
