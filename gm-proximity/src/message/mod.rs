//! Message logic for GM-Proximity.
//!
//! Module for message handling between client and server. Includes message types, serialization/deserialization, and
//! structure of inner message types.

mod encode;
mod request;
mod response;

pub mod prelude {
    pub use super::encode::IoCodec;
    pub use super::request::*;
    pub use super::response::*;
}
