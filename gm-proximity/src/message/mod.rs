//! Message logic for GM-Proximity.
//!
//! Module for message handling between client and server. Includes message types, serialization/deserialization, and
//! structure of inner message types.

mod batch;
mod feature;

pub use batch::dispatch_counted_batches;
pub use feature::{Feature, KeyGenerator, ParsedFeature};
