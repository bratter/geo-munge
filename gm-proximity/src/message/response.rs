//! Reponses.

use anyhow::Result;
use bincode::{Decode, Encode};

use super::stream::MessageStream;

#[derive(Debug, Encode, Decode)]
#[non_exhaustive]
pub enum Response {
    /// Success response.
    ///
    /// General response indicating that the previous request was successful, but the request type had no specific data
    /// that it needed to return.
    Success(Option<String>),

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
    Error(String),
}

impl Response {
    pub fn decode(buf: &[u8]) -> Result<Self> {
        let config = bincode::config::standard();
        let (res, _) = bincode::decode_from_slice::<Self, _>(&buf, config)?;

        Ok(res)
    }

    // TODO: I don't think this is any better if it consumes the self, but check if there is a better way
    // TODO: Do we want to send the name of the request with the error if it fails to encode
    pub fn encode(&self) -> Result<Vec<u8>> {
        let config = bincode::config::standard();
        let bytes = bincode::encode_to_vec(self, config)?;

        Ok(bytes)
    }
}

// Use default read/write impls.
// TODO: Likely remove
impl MessageStream for Response {}

impl From<anyhow::Error> for Response {
    fn from(err: anyhow::Error) -> Self {
        Response::Error(err.to_string().into())
    }
}
