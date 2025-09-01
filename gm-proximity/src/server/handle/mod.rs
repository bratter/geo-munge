//! Request Handlers.

mod bench;
mod delete;
mod get;
mod handle;
mod insert;
mod knn;
mod reset;
mod stats;
mod window;

pub mod handlers {
    pub use super::bench::bench;
    pub use super::delete::delete;
    pub use super::get::get;
    pub use super::insert::insert;
    pub use super::knn::knn;
    pub use super::reset::reset;
    pub use super::stats::stats;
    pub use super::window::window;
}

pub use handle::{record_to_basic_result, Context, Handler};

/// The maximum number of items for a single batch of results.
///
/// This number is a hedge between arbitrary geometries while reducing overhead for points. This should be tweaked in
/// practice, and potentially changed to something far more accurate if the transmission encoding is changed.
const MAX_GEOM_BATCH_SIZE: u32 = 200;

/// The maximum number of ids to send in a single batch of results.
///
/// Because id only content is much smaller than geometry/properties, we set a spearate limit for these to add some
/// additional efficiency.
const MAX_ID_BATCH_SIZE: u32 = 2048;
