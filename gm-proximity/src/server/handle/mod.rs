//! Request Handlers.

mod bench;
mod delete;
mod get;
mod handle;
mod insert;
mod knn;
mod reset;
mod stats;

pub mod handlers {
    pub use super::bench::bench;
    pub use super::delete::delete;
    pub use super::get::get;
    pub use super::insert::insert;
    pub use super::knn::knn;
    pub use super::reset::reset;
    pub use super::stats::stats;
}

pub use handle::{Context, Handler};
