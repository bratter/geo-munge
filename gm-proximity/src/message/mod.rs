//! Message logic for GM-Proximity.
//!
//! Module for message handling between client and server. Includes message types, serialization/deserialization, and
//! structure of inner message types.

mod request;
mod response;
mod stream;

pub mod prelude {
    pub use super::request::*;
    pub use super::response::Response;
    pub use super::stream::MessageStream;
}
