mod bench;
mod delete;
mod get;
mod insert;
mod knn;
mod reset;
mod stats;
mod window;

pub use bench::bench;
pub use delete::delete;
pub use get::get;
pub use insert::insert;
pub use knn::knn;
pub use reset::reset;
pub use stats::stats;
pub use window::window;

// Pull in some useful imports that the controllers use
use super::{
    geo::GeoStore,
    handler::{feature_to_basic_result, Context},
};

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
