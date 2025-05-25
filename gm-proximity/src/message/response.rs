//! Responses.

use anyhow::Result;
use bincode::{Decode, Encode};

use super::encode::IoEncode;

#[derive(Debug, Encode, Decode)]
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
    Done(usize),

    /// Response to Stats request.
    ///
    /// Includes both config settings such as bounding box and primary key, and quadtree statistics such as size and
    /// number of layers.
    Stats(Stats),

    /// Insertion result response.
    ///
    /// Returns the number of successes and failures
    /// TODO: Upgrade to contain failure details?
    InsertResult { success: usize, fail: usize },

    /// Knn result data.
    ///
    /// Contains a vector of results from a Knn calculation, wrapped in a result for failed rows.
    /// TODO: Response type without errors, better response type overall
    KnnData(Vec<Result<(usize, f64), String>>),

    /// Data response.
    ///
    /// Any response that requires data to be returned.
    Data,

    /// An error response.
    ///
    /// Indicates that the previous request was invalid or could not be processed.
    ///
    /// TODO: This should probably contain more information than just a message.
    Error(String),
}

// Use default encode and decode impls
impl IoEncode for Response {}

impl From<anyhow::Error> for Response {
    fn from(err: anyhow::Error) -> Self {
        Response::Error(err.to_string().into())
    }
}

#[derive(Debug, Encode, Decode)]
pub struct Stats {
    pub qt_size: usize,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
}
