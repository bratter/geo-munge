mod bench;
mod delete;
mod get;
mod knn;
mod load;
mod repl;
mod reset;
mod window;

pub use bench::bench;
pub use delete::delete;
pub use get::get;
pub use knn::knn;
pub use load::load;
pub use repl::repl;
pub use reset::reset;
pub use window::window;

use anyhow::Result;

// Pull in some convenient symbols
pub use super::handler::{CommandHandler, OutputFormat, OutputOptions, Res, ResponseHandler};

/// The maximum number of data bytes available for use in a single batch.
///
/// This is currently the size of a socket buffer less some padding, but can be tuned to balance performance.
/// Note that 4kB of padding is generous, but will have minimal impact on overall efficiency.
///
/// We use this as a heuruistic in absence of (currently) an easy way of building the batches post-serialization.
const MAX_BATCH_BYTES: usize = (64 - 4) * 1024;

/// The maximum number of items possible in a single batch.
///
/// Because we gather [`geojson::Feature`] structs for sending, the maximum number we could possibly send in a single
/// batch is the size of the smallest possible feature text string, which should be ~66 bytes. Note that this doesn't
/// represent the size when serialized, only the size when read as a string to align with the [`MAX_BATCH_BYTES`].
///
/// This should only be used for pre-allocating vectors to hold buffers being built.
const MAX_FEATURE_COUNT: usize = MAX_BATCH_BYTES / 66;

/// The maximum number of items for a single batch of ids.
///
/// This should be well inside the available 64kb because the maximum size of a Custom Key is 16b -> max 4096.
const MAX_ID_BATCH_SIZE: u32 = 2048;

/// Callback to be passed to filter_map to eprintln!() a parse error then pass on the non-error items. For use only in
/// client batching controllers to publish then not batch errors.
fn print_and_filter_err<T, E: std::fmt::Display>(item_result: Result<T, E>) -> Option<T> {
    match item_result {
        Ok(item) => Some(item),
        Err(err) => {
            eprintln!("Warning: Could not parse item: {}", err);
            None
        }
    }
}
