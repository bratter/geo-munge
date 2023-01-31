use std::{fmt, path::PathBuf};

/// Custom error enum for emitting on failure.
///
/// Includes custom display, debug traits to produce human-readable error messages.
#[non_exhaustive]
pub enum Error {
    FileIOError(std::io::Error),
    CannotReadFile(PathBuf),
    CannotParseFile(PathBuf),
    CannotParseFileExtension(PathBuf),
    UnsupportedFileType,
    UnexpectedEndOfInput,
    InvalidDelimiter,
    InvalidBoundingBox,
    MissingBoundingBox,
    TypeDoesNotContainMetadata,
    CsvParseError(csv::Error),
    CsvWriteError(csv::Error),
    ShapefileParseError(shapefile::Error),
    ShapeFileWriteError(shapefile::Error),
    MissingLatLngField,
    CannotParseRecord(usize, ParseType),
    UnsupportedGeometry(UnsupportedGeoType),
    InsertFailed(usize, quadtree::Error),
    InsertFailedRequiresPoint(usize),
    FindError(usize, quadtree::Error),
    FailedToDeserialize(PathBuf, serde_json::Error),
    ExecPipelineFailed(std::io::Error),
    CannotFindCommand,
}

pub enum ParseType {
    Lng,
    Lat,
    GeoJson,
    Shapefile,
    Csv,
    MissingGeometry,
}

pub enum UnsupportedGeoType {
    NestedKmlMulti,
    KmlElement,
    UnknownKml,
    NullShp,
    MultipatchShp,
}

impl fmt::Display for UnsupportedGeoType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_str = match self {
            UnsupportedGeoType::NestedKmlMulti => "Nested KML Multigeometry",
            UnsupportedGeoType::KmlElement => "KML Element",
            UnsupportedGeoType::UnknownKml => "Unknown KML type",
            UnsupportedGeoType::NullShp => "Shapefile Null",
            UnsupportedGeoType::MultipatchShp => "Shapefile Multipatch",
        };
        write!(f, "{}", type_str)
    }
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileIOError(err) => {
                write!(f, "File IO error encountered; {}", err)
            },
            Self::CannotReadFile(path) => {
                write!(f, "Cannot read file at {}", path.to_string_lossy())
            }
            Self::CannotParseFile(path) => {
                write!(f, "Cannot parse file at {}", path.to_string_lossy())
            }
            Self::CannotParseFileExtension(path) => write!(
                f,
                "Cannot parse file extension for file {}",
                path.to_string_lossy()
            ),
            Self::UnsupportedFileType => write!(f, "Unsupported file type"),
            Self::UnexpectedEndOfInput => write!(f, "Unexpected end of input"),
            Self::InvalidDelimiter => write!(f, "Invalid delimiter provided"),
            Self::InvalidBoundingBox => write!(f, "The bounding box provided in the source file or on the command line is not valid"),
            Self::MissingBoundingBox => write!(f, "A bounding box was expected but not found"),
            Self::TypeDoesNotContainMetadata => write!(f, "The provided type was expected to contain metadata, but it does not"),
            Self::CsvParseError(err) => write!(f, "Error parsing csv input: {}", err),
            Self::CsvWriteError(err) => write!(f, "Error writing csv output: {}", err),
            Self::ShapefileParseError(err) => write!(f, "Error parsing shapefile input: {}", err),
            Self::ShapeFileWriteError(err) => write!(f, "Error writing to shapefile: {}", err),
            Self::MissingLatLngField => write!(f, "The test points are missing a lng or lat field"),
            Self::CannotParseRecord(i, parse_type) => {
                let type_str = match parse_type {
                    ParseType::Lng => "Lng parsing failed",
                    ParseType::Lat => "Lat parsing failed",
                    ParseType::GeoJson => "GeoJson feature parsing failed",
                    ParseType::Shapefile => "Shapefile parsing failed",
                    ParseType::Csv => "CSV parsing failed",
                    ParseType::MissingGeometry => "Missing geometry",
                };
                write!(f, "Failed to parse record at index {}: {}", i, type_str)
            }
            Self::UnsupportedGeometry(geo_type) => write!(f, "Unsupported geometry type encountered: {}", geo_type),
            Self::InsertFailed(i, err) => write!(f, "Insert failed for geometry at index {}: {}", i, display_qt_err(err)),
            Self::InsertFailedRequiresPoint(i) => write!(f, "Cannot insert non-point geometry into point quadtree at index {}, to enable bounds mode, create the quadtree without the -p flag", i),
            Self::FindError(i, err) => write!(f, "Match for input record at index {}, failed: {}", i, display_qt_err(err)),
            Self::FailedToDeserialize(path, err) => write!(f, "Deserialization failed for file {}, error provided: {}", path.to_string_lossy(), err),
            Self::ExecPipelineFailed(err) => write!(f, "Run execution failure: {}", err) ,
            Self::CannotFindCommand => write!(f, "Could not locate the proximity command for execution")
        }
    }
}

fn display_qt_err(err: &quadtree::Error) -> &'static str {
    match err {
        quadtree::Error::Empty => "QuadTree is empty",
        quadtree::Error::OutOfBounds => "Input point is out of bounds",
        quadtree::Error::NoneInRadius => "No datum available within search radius",
        quadtree::Error::CannotMakeBbox => "Cannot make the bounding box",
        quadtree::Error::InvalidDistance => "Attempt to calculate distances on invalid shapes",
        quadtree::Error::CannotFindSubNode => "Cannot find sub node",
        quadtree::Error::CannotCastInfinity => "Cannot cast infinity",
        _ => "Unknown Quadtree error",
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
