//! Responses.

use std::fmt::Debug;

use anyhow::Result;
use bincode::{Decode, Encode};
use geojson::JsonValue;

use super::{encode::IoCodec, request::KeyMode, Feature, NodeId, Properties};

#[derive(Encode, Decode)]
#[non_exhaustive]
pub enum Response {
    /// Success response.
    ///
    /// General response indicating that the previous request was successful, but the request type had no specific data
    /// that it needed to return.
    Success(Option<String>),

    /// Done response.
    ///
    /// Indicate that this is the last response for the operation when the request returned multiple individual
    /// responses. This will usually be attached to a request id in the message and contains the number of individual
    /// responses EXCLUDING this one that were returned.
    Done(u32),

    /// Response to Stats request.
    ///
    /// Includes both config settings such as bounding box and primary key, and quadtree statistics such as size and
    /// number of layers.
    Stats(Stats),

    /// Response type that captures a count of successes and failures.
    /// TODO: Upgrade to contain failure details?
    ResultCounts { success: usize, fail: usize },

    /// Basic query results without distance information.
    ///
    /// Used for Get and Window commands that don't involve proximity calculations.
    BasicResults(Vec<Result<BasicResult, String>>),

    /// Proximity query results with distance information.
    ///
    /// Used for KNN commands that involve distance calculations.
    ProximityResults(Vec<Result<ProximityResult, String>>),

    /// An error response.
    ///
    /// Indicates that the previous request was invalid or could not be processed.
    ///
    /// TODO: This should probably contain more information than just a message.
    Error(String),

    /// A benchmarking request to the server.
    Bench(BenchRes),
}

// Use default encode and decode impls
impl IoCodec for Response {}

impl From<anyhow::Error> for Response {
    fn from(err: anyhow::Error) -> Self {
        Response::Error(err.to_string().into())
    }
}

impl Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success(msg) => f.debug_tuple("Success").field(msg).finish(),
            Self::Done(count) => f.debug_tuple("Done").field(count).finish(),
            Self::Stats(stats) => f.debug_tuple("Stats").field(stats).finish(),
            Self::ResultCounts { success, fail } => f
                .debug_struct("ResultCounts")
                .field("success", success)
                .field("fail", fail)
                .finish(),
            Self::BasicResults(results) => f
                .debug_tuple("BasicResults")
                .field(&format_args!(
                    "Vec<Result<BasicResult, String>>({})",
                    results.len()
                ))
                .finish(),
            Self::ProximityResults(results) => f
                .debug_tuple("ProximityResults")
                .field(&format_args!(
                    "Vec<Result<ProximityResult, String>>({})",
                    results.len()
                ))
                .finish(),
            Self::Error(msg) => f.debug_tuple("Error").field(msg).finish(),
            Self::Bench(bench_res) => f.debug_tuple("Bench").field(bench_res).finish(),
        }
    }
}

#[derive(Debug, Encode, Decode)]
pub struct Stats {
    pub key_mode: KeyMode,
    pub qt_size: usize,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
}

/// Content type for query results - determines what additional data is returned with the ID.
/// TODO: Move these common things out into a different file
#[derive(Debug, Encode, Decode)]
pub enum ContentType {
    /// Full GeoJSON feature with properties and geometry.
    FullFeature(Feature),

    /// GeoJSON geometry only, without properties.
    GeometryOnly(Feature),

    /// Properties only as JSON value.
    PropertiesOnly(Properties),

    /// No additional content, ID only.
    None,
}

impl ContentType {
    /// Set a property on the content type.
    ///
    /// When the content type is None, this will change the content type to properties only, enabling the addition of
    /// properties in a mutable manner downstream from origination. This lets us inject results data in the response.
    pub fn set_property(&mut self, key: impl Into<String>, value: impl Into<JsonValue>) {
        match self {
            ContentType::FullFeature(feature) => feature.0.set_property(key, value),
            ContentType::GeometryOnly(feature) => feature.0.set_property(key, value),
            ContentType::PropertiesOnly(properties) => properties.set_property(key, value),
            ContentType::None => {
                let mut new_properties = Properties::default();
                new_properties.set_property(key, value);
                let _ = std::mem::replace(self, ContentType::PropertiesOnly(new_properties));
            }
        }
    }
}

/// Basic query result without distance information.
#[derive(Debug, Encode, Decode)]
pub struct BasicResult {
    /// The unique identifier of the node.
    pub id: NodeId,

    /// The content to return with this result.
    pub content: ContentType,
}

/// Proximity query result with distance information.
#[derive(Debug, Encode, Decode)]
pub struct ProximityResult {
    /// The input index from the original query request.
    pub input_index: usize,

    /// When the query was a reference to an id or a filter, return the uid of the retrieved input item.
    pub input_uid: Option<NodeId>,

    /// The unique identifier of the retrieved node.
    pub id: NodeId,

    /// The distance from the query geometry.
    pub distance: f64,

    /// The content to return with this result.
    pub content: ContentType,
}

#[derive(Encode, Decode)]
pub struct BenchRes {
    /// Dummy data to reflect data sent with a response.
    pub data: Vec<u8>,
}

impl BenchRes {
    pub fn with_len(len: usize) -> Self {
        let mut data = Vec::with_capacity(len);
        data.resize(len, 0);

        Self { data }
    }
}

impl Debug for BenchRes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BenchRes").finish_non_exhaustive()
    }
}
