use std::sync::{Arc, RwLock};

use anyhow::Result;
use geolib::qt::Quadtree;

use crate::message::Response;

// TODO: An actual implementation
pub fn stats(qt: Arc<RwLock<Quadtree>>) -> Result<Response> {
    // NOTE: Ok to propagate panic with unwrap as the only error is for a poisoned RwLock
    let qt = qt.read().unwrap();

    Ok(Response::Stats(qt.size()))
}
