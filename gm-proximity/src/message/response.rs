//! Reponses.

use bincode::{Decode, Encode};

use super::MessageStream;

#[derive(Debug, Encode, Decode)]
#[non_exhaustive]
pub enum Response {
    /// Success response.
    ///
    /// General response indicating that the previous request was successful, but the request type had no specific data
    /// that it needed to return.
    ///
    /// TODO: Make an inner type here that captures different types of success messages
    Success(Option<String>),

    /// Response to Stats request.
    ///
    /// Includes both config settings such as bounding box and primary key, and quadtree statistics such as size and
    /// number of layers.
    Stats(usize),

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

// Use default read/write impls.
impl MessageStream for Response {}
