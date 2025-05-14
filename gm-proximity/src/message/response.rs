//! Reponses.

use std::borrow::Cow;

use bincode::{Decode, Encode};

use super::stream::MessageStream;

#[derive(Debug, Encode, Decode)]
#[non_exhaustive]
pub enum Response<'a> {
    /// Success response.
    ///
    /// General response indicating that the previous request was successful, but the request type had no specific data
    /// that it needed to return.
    Success(Option<Cow<'a, str>>),

    /// Response to Stats request.
    ///
    /// Includes both config settings such as bounding box and primary key, and quadtree statistics such as size and
    /// number of layers.
    Stats(usize),

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
    Error(Cow<'a, str>),
}

// Use default read/write impls.
impl MessageStream for Response<'_> {}

impl From<anyhow::Error> for Response<'_> {
    fn from(err: anyhow::Error) -> Self {
        Response::Error(err.to_string().into())
    }
}
