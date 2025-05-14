//! Request Handlers.

mod handle;
mod insert;
mod knn;
mod reset;
mod stats;

use insert::insert;
use knn::knn;
use reset::reset;
use stats::stats;

pub use handle::handle_request;
