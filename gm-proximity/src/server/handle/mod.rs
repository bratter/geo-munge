//! Request Handlers.

mod bench;
mod handle;
mod insert;
mod knn;
mod reset;
mod stats;

use bench::bench;
use insert::insert;
use knn::knn;
use reset::reset;
use stats::stats;

pub use handle::{Context, Handler};
