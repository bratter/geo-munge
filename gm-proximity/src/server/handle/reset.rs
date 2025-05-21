use crate::{message::prelude::*, server::run::build_qt};

use super::Context;

/// Resets the quadtree.
///
/// Drops all memory associated with the original quadree, replacing it with a fresh one. It will not cancel any other
/// in-progress operations.
pub fn reset(handler: Context, reset: Reset) {
    // Best way to drop the quadtree is to memory replace with a new one, ensuring a complete reset
    let _ = std::mem::replace(&mut *handler.write_qt(), build_qt(reset));

    handler.send(Response::Success(None));
}
