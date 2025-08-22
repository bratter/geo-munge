//! Requests.

use std::{fmt::Debug, num::ParseIntError, str::FromStr};

use anyhow::{anyhow, bail, Error, Ok, Result};
use bincode::{Decode, Encode};
use geo::{Point, Rect};

use crate::server::geo_store::GeoRecord;

use super::{encode::IoCodec, response::ContentType, CustomKey, Feature, NodeId};

#[derive(Encode, Decode)]
#[non_exhaustive]
pub enum Request {
    /// Request basic quadtree statistics from the current server.
    ///
    /// Includes both config settings such as bounding box and primary key, and quadtree statistics such as siae and
    /// number of layers.
    Stats,

    /// Reset request.
    ///
    /// Truncate the quadtree. Options to reset the primary key and bounding box, otherwise will reset these to the
    /// defaults.
    ///
    /// Will default to the whole Earth bounding box of `[-180, -90, 180, 90]`. Anything outside the bounding box will
    /// be rejected. The bounding box can be grown after initialization, but it cannot be shrunk.
    ///
    /// The underlying quadtree implementation reserves the right to not set the bounding box exactly if it would be
    /// more efficient to choose a large bounding box. It will never be smaller.
    Reset(ResetReq),

    /// Insert request.
    ///
    /// This request makes no promises that the included data is well-formed JSON (so that clients don't have to validate
    /// when the server really should anyway). Therefore items extracted from the insert may error on the server during
    /// parsing. This SHOULD NOT error the whole insert, only the individual features.
    ///
    /// If possible, clients SHOULD batch insertion requests to improve efficiency. Batches should be sized small enough to
    /// avoid over-using memory, but can be larger than 1 to make inserts more efficient. The server MAY choose to
    /// arbitrarily chunk large batches, but will not batch across requests.
    Insert(Vec<Feature>),

    /// Get items using either the uid or a custom key.
    Get(GetReq),

    /// Delete items using either the uid or a custom key.
    Delete(KeySet),

    /// Conduct a KNN search on the stored data.
    ///
    /// Can take a max count, radius or bounding box constraints, and metadata filters.
    /// TODO: Also do find, also do filters
    Knn(KnnReq),

    /// Filter for all items inside a bounding box.
    ///
    /// Can take a set of metadata filters.
    ///
    /// TODO: Also do filters like Knn
    Window(WindowReq),

    /// A benchmarking request to the server.
    Bench(BenchReq),
}

impl Request {
    /// Indicates whether the server produces a single response message for the current request type.
    ///
    /// When true, the server will only ever produce a single result for the request, so clients can retire any request
    /// tracking after a single response. When false, the server will send at least two responses, the last of which
    /// will be done that will contain the number of messages excluding the done.
    /// TODO: Could signal done with the Done or Success response types instead
    /// TODO: Do we still need this?
    pub fn is_oneshot(&self) -> bool {
        match self {
            Request::Stats => true,
            Request::Reset(_) => true,
            Request::Insert(_) => true,
            Request::Delete(_) => true,
            Request::Knn(_) => false,
            Request::Window(_) => false,
            Request::Get(_) => false,
            Request::Bench(_) => false,
        }
    }
}

// Use default encode and decode impls
impl IoCodec for Request {}

impl Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stats => write!(f, "Stats"),
            Self::Reset(r) => f.debug_tuple("Reset").field(r).finish(),
            Self::Insert(features) => f
                .debug_tuple("Insert")
                .field(&format_args!("Vec<Feature>({})", features.len()))
                .finish(),
            Self::Get(r) => f.debug_tuple("Get").field(r).finish(),
            Self::Delete(r) => f.debug_tuple("Delete").field(r).finish(),
            Self::Knn(r) => f.debug_tuple("Knn").field(r).finish(),
            Self::Window(r) => f.debug_tuple("Window").field(r).finish(),
            Self::Bench(r) => f.debug_tuple("Bench").field(r).finish(),
        }
    }
}

/// Reset request type.
///
/// Contains an optional key type and bounding box to provide settings in a single request.
#[derive(Debug, Default, Encode, Decode)]
pub struct ResetReq {
    pub key_mode: KeyMode,
    pub bbox: Option<Bbox>,
}

impl ResetReq {
    pub fn new(key_mode: KeyMode, bbox: Option<Bbox>) -> Self {
        Self { key_mode, bbox }
    }
}

/// Key type setting request data.
///
/// Likely to be used in [`Request::Reset`].
#[derive(Debug, Default, Clone, Encode, Decode)]
pub enum KeyMode {
    #[default]
    AutoIncrement,
    CustomU32(String),
    MetaPointer(String),
}

/// Bounding box request data.
///
/// Can be used in a [`Request::Bbox`], but more likely to be used in [`Request::Reset`].
///
/// FIX: Needs to be radians aware, need to harmonize with the get_earth_bbox function in math, probably needs to move
#[derive(Debug, Clone, Encode, Decode)]
pub struct Bbox {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl Default for Bbox {
    fn default() -> Self {
        Self {
            x1: -180.0,
            y1: -90.0,
            x2: 180.0,
            y2: 90.0,
        }
    }
}

impl From<Rect> for Bbox {
    fn from(value: Rect) -> Self {
        let min = value.min();
        let max = value.max();

        Bbox {
            x1: min.x,
            y1: min.y,
            x2: max.x,
            y2: max.y,
        }
    }
}

impl From<Bbox> for Rect {
    fn from(value: Bbox) -> Self {
        Rect::new(
            Point::new(value.x1, value.y1),
            Point::new(value.x2, value.y2),
        )
    }
}

// TODO: Better error messages for floats on let entries, and better bounds checking
impl FromStr for Bbox {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let entries: Result<Vec<_>, _> = s.split(',').map(|s| s.parse::<f64>()).collect();
        let entries = entries?;

        if entries.len() != 4 {
            bail!(
                "A bounding box requires 4 comma separated floats, {} provided",
                entries.len()
            );
        }

        // Now we know there are exactly 4 entries
        let x1 = entries[0];
        let y1 = entries[1];
        let x2 = entries[2];
        let y2 = entries[3];

        if x1 >= x2 || y1 >= y2 {
            bail!("A bounding box must be four floats x1,y1,x2,y2 with 1 being the top left and 2 being the bottom right");
        }

        Ok(Bbox { x1, y1, x2, y2 })
    }
}

#[derive(Debug, Encode, Decode)]
pub struct KnnReq {
    pub k: usize,
    pub r: Option<f64>,
    pub content_mode: ContentMode,
    pub data: FindData,
    // TODO: Add filters
}

#[derive(Debug, Encode, Decode)]
pub struct WindowReq {
    pub bbox: Bbox,
    pub join: JoinType,
    pub content_mode: ContentMode,
}

#[derive(Debug, Encode, Decode)]
pub enum JoinType {
    Intersects,
    Contains,
}

#[derive(Encode, Decode)]
pub enum FindData {
    /// Run the find for the stream of passed features.
    Features(Vec<Feature>),

    /// Run the find for a set of primary keys already in the quadtree.
    Keys(KeySet),
}

impl Debug for FindData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Features(features) => f
                .debug_tuple("Features")
                .field(&format_args!("Vec<Feature>({})", features.len()))
                .finish(),
            Self::Keys(keys) => f.debug_tuple("Keys").field(keys).finish(),
        }
    }
}

#[derive(Debug, Encode, Decode)]
pub struct GetReq {
    pub keys: KeySet,
    pub content_mode: ContentMode,
}

#[derive(Encode, Decode)]
pub enum KeySet {
    Uid(Vec<NodeId>),
    Custom(Vec<CustomKey>),
}

impl KeySet {
    pub fn parse_with_type(s: &str, key_bytes: bool) -> Result<Self> {
        if key_bytes {
            Self::custom_from_str(s)
        } else {
            Self::uid_from_str(s)
        }
    }

    pub fn uid_from_str(s: &str) -> Result<Self> {
        let ks = s
            .split(',')
            .map(|b| b.parse())
            .collect::<Result<Vec<u32>, ParseIntError>>()?
            .into();

        Ok(ks)
    }

    pub fn custom_from_str(s: &str) -> Result<Self> {
        let ks = match s.parse::<geojson::JsonValue>()? {
            geojson::JsonValue::Array(arr) => arr
                .iter()
                .map(CustomKey::try_from)
                .collect::<Result<Vec<CustomKey>>>()?
                .into(),
            v => vec![CustomKey::try_from(&v)?].into(),
        };

        Ok(ks)
    }
}

impl From<Vec<NodeId>> for KeySet {
    fn from(value: Vec<NodeId>) -> Self {
        KeySet::Uid(value)
    }
}

impl From<Vec<CustomKey>> for KeySet {
    fn from(value: Vec<CustomKey>) -> Self {
        KeySet::Custom(value)
    }
}

impl Debug for KeySet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uid(uids) => f
                .debug_tuple("Uid")
                .field(&format_args!("Vec<NodeId>({})", uids.len()))
                .finish(),
            Self::Custom(keys) => f
                .debug_tuple("Custom")
                .field(&format_args!("Vec<CustomKey>({})", keys.len()))
                .finish(),
        }
    }
}

#[derive(Encode, Decode)]
pub struct BenchReq {
    /// Millisecond processing delay to simulate computation time.
    pub delay: Option<u64>,

    /// Bytes to send the response to simulate response size.
    pub size: u32,

    /// Ratio of responses to requests.
    pub ratio: u32,

    /// Dummy data to reflect data sent with a request.
    pub data: Vec<u8>,
}

impl Debug for BenchReq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BenchReq")
            .field("delay", &self.delay)
            .field("size", &self.size)
            .field("ratio", &self.ratio)
            .finish_non_exhaustive()
    }
}

/// Content mode for query response output.
#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub enum ContentMode {
    /// Return no additional content, IDs only.
    #[default]
    None,

    /// Return full GeoJSON features with properties and geometry.
    Full,

    /// Return GeoJSON geometry only, without properties.
    Geometry,

    /// Return properties only as JSON.
    Properties,
}

impl ContentMode {
    /// Convert a [`GeoRecord`] to the correct [`ContentType`] for responses based on this mode.
    pub fn with_record(&self, record: &GeoRecord) -> ContentType {
        match self {
            Self::None => ContentType::None,
            Self::Full => ContentType::FullFeature(geojson::Feature::from(record.as_ref()).into()),
            Self::Geometry => {
                let geom: geojson::Feature = geojson::Geometry::from(record.as_ref()).into();
                ContentType::GeometryOnly(geom.into())
            }
            Self::Properties => {
                ContentType::PropertiesOnly(geojson::JsonValue::from(record.as_ref()).into())
            }
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::None => "None",
            Self::Full => "Full Feature",
            Self::Geometry => "Geometry",
            Self::Properties => "Properties",
        }
    }

    pub fn list() -> [&'static str; 4] {
        [
            Self::None.as_str(),
            Self::Full.as_str(),
            Self::Geometry.as_str(),
            Self::Properties.as_str(),
        ]
    }
}

impl FromStr for ContentMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "none" | "id" | "ids" => Ok(ContentMode::None),
            "full" | "feature" => Ok(ContentMode::Full),
            "geometry" | "geom" => Ok(ContentMode::Geometry),
            "properties" | "props" | "meta" => Ok(ContentMode::Properties),
            _ => Err(anyhow!(
                "Invalid content mode '{}'. Valid options: full, geometry, properties, none",
                s
            )),
        }
    }
}

// TODO: Should these be moved into a newtype in the repl module, or should the setting types all be moved into a common mod
impl TryFrom<usize> for ContentMode {
    type Error = Error;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Full),
            2 => Ok(Self::Geometry),
            3 => Ok(Self::Properties),
            _ => bail!("Invalid index for ContentMode"),
        }
    }
}
