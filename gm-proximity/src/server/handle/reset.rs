use std::sync::{Arc, RwLock};

use anyhow::Result;
use geolib::qt::Quadtree;

use crate::{
    message::{Reset, Response},
    server::run::build_qt,
};

/// Resets the quadtree.
///
/// Drops all memory associated with the original quadree, replacing it with a fresh one. It will not cancel any other
/// in-progress operations.
pub fn reset(qt: Arc<RwLock<Quadtree>>, reset: Reset) -> Result<Response> {
    // NOTE: Ok to propagate panic with unwrap as the only error is for a poisoned RwLock
    let qt = &mut *qt.write().unwrap();

    // Best way to drop the quadtree is to memory replace with a new one, ensuring a complete reset
    let _ = std::mem::replace(qt, build_qt(reset));

    Ok(Response::Success(None))
}
