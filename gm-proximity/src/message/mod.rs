//! Message logic for GM-Proximity.
//!
//! Module for message handling between client and server. Includes message types, serialization/deserialization, and
//! structure of inner message types.

mod request;
mod response;
mod stream;

pub use request::*;
pub use response::Response;
pub use stream::MessageStream;
