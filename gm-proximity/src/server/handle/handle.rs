use std::sync::{Arc, RwLock};

use anyhow::Result;
use geolib::qt::Quadtree;

use crate::message::{Request, Response};

use super::{insert, reset, stats};

pub fn handle_request(req: Request, qt: Arc<RwLock<Quadtree>>) -> Result<Response> {
    match req {
        Request::Stats => stats(qt),
        Request::Reset(r) => reset(qt, r),
        Request::Insert(i) => insert(qt, i),
        // TODO: Delete
        _ => todo!("Delete this"),
    }
}
