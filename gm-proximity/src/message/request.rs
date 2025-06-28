//! Requests.

use std::{fmt::Debug, num::ParseIntError, str::FromStr};

use anyhow::{bail, Error, Result};
use bincode::{Decode, Encode};
use geo::{Point, Rect};

use super::{encode::IoCodec, CustomKey, Feature, NodeId};

#[derive(Debug, Encode, Decode)]
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
    /// TODO: Support grouping for close together items
    Window,

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
            Request::Window => false,
            Request::Get(_) => false,
            Request::Bench(_) => false,
        }
    }
}

// Use default encode and decode impls
impl IoCodec for Request {}

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
    pub data: FindData,
    // TODO: Add filters
}

#[derive(Debug, Encode, Decode)]
pub enum FindData {
    /// Run the find for the stream of passed features.
    Features(Vec<Feature>),

    /// Run the find for a set of primary keys already in the quadtree.
    Keys(KeySet),
}

#[derive(Debug, Encode, Decode)]
pub struct GetReq {
    pub keys: KeySet,
    pub meta_only: bool,
}

#[derive(Debug, Encode, Decode)]
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
        let ks = s
            .split(',')
            .map(|b| CustomKey::try_from(b.as_bytes()))
            .collect::<Result<Vec<CustomKey>>>()?
            .into();

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
