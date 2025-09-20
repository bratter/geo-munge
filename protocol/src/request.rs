//! Requests.

use std::{
    fmt::{Debug, Display},
    num::ParseIntError,
    str::FromStr,
};

use anyhow::{anyhow, bail, Error, Ok, Result};
use bincode::{Decode, Encode};
use geo::{Point, Rect, ToDegrees, ToRadians};

use crate::{content::ContentMode, feature::JsonFeature, CustomKey, Uid};

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
    Insert(Vec<JsonFeature>),

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
    pub bbox: Option<DegreeBbox>,
}

impl ResetReq {
    pub fn new(key_mode: KeyMode, bbox: Option<DegreeBbox>) -> Self {
        Self { key_mode, bbox }
    }
}

/// Key type setting request data.
///
/// Used in [`Request::Reset`] and for stats responses.
#[derive(Debug, Default, Clone, Encode, Decode)]
pub enum KeyMode {
    #[default]
    AutoIncrement,
    ProvidedNumeric,
    CustomBytes(String),
}

/// Bounding box for client use in degrees.
///
/// This struct is designed to be parsed as lng_min, lat_min, lnhg_max, lat_max in decimal degrees.
///
/// Producing a [`Rect`] from this bounding box will automatically convert to Radians.
#[derive(Debug, Clone, Encode, Decode)]
pub struct DegreeBbox {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl Default for DegreeBbox {
    fn default() -> Self {
        Self {
            x1: -180.0,
            y1: -90.0,
            x2: 180.0,
            y2: 90.0,
        }
    }
}

impl From<Rect> for DegreeBbox {
    fn from(mut value: Rect) -> Self {
        value.to_degrees_in_place();
        let min = value.min();
        let max = value.max();

        DegreeBbox {
            x1: min.x,
            y1: min.y,
            x2: max.x,
            y2: max.y,
        }
    }
}

impl From<DegreeBbox> for Rect {
    fn from(value: DegreeBbox) -> Self {
        let mut rect = Rect::new(
            Point::new(value.x1, value.y1),
            Point::new(value.x2, value.y2),
        );
        rect.to_radians_in_place();

        rect
    }
}

// TODO: Better error messages for floats on let entries, and better bounds checking
impl FromStr for DegreeBbox {
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

        Ok(DegreeBbox { x1, y1, x2, y2 })
    }
}

impl Display for DegreeBbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{},{},{},{}]", self.x1, self.y1, self.x2, self.y2)
    }
}

#[derive(Debug, Encode, Decode)]
pub struct KnnReq {
    pub k: usize,
    pub r: Option<f64>,
    pub content_mode: ContentMode,
    /// The input_index of the first item in this request.
    pub start_index: usize,
    pub data: FindData,
    // TODO: Add filters
}

#[derive(Debug, Encode, Decode)]
pub struct WindowReq {
    pub bbox: DegreeBbox,
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
    Features(Vec<JsonFeature>),

    /// Run the find for a set of primary keys already in the quadtree.
    Keys(KeySet),
}

impl FindData {
    pub fn len(&self) -> usize {
        match self {
            FindData::Features(features) => features.len(),
            FindData::Keys(KeySet::Uid(keys)) => keys.len(),
            FindData::Keys(KeySet::Custom(keys)) => keys.len(),
        }
    }
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
    Uid(Vec<Uid>),
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
        let ks = match s
            .parse::<geojson::JsonValue>()
            .map_err(|err| anyhow!("{} - this must be a valid JSON array or value when attempting to parse a custom key from a string", err))?
        {
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

impl From<Vec<Uid>> for KeySet {
    fn from(value: Vec<Uid>) -> Self {
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
