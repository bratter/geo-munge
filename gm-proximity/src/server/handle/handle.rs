use std::sync::{Arc, RwLock};

use geolib::qt::Quadtree;

use crate::message::prelude::*;

use super::{insert, knn, reset, stats};

pub fn handle_request(req: Request, qt: &Arc<RwLock<Quadtree>>) -> Response {
    let res = match req {
        Request::Stats => stats(qt),
        Request::Reset(r) => reset(qt, r),
        Request::KeyType(_) => Ok(Response::Error("KeyType resettingk not implemented".into())),
        Request::Bbox(_) => Ok(Response::Error("BBox resetting not implemented".into())),
        Request::Insert(i) => insert(qt, i),
        Request::Delete => Ok(Response::Error("Delete not implemented".into())),
        Request::Knn(knn_data) => knn(qt, knn_data),
        // TODO: Delete
        _ => todo!("Delete this"),
    };

    // Convert errors to an error response - handle fails shouldn't break out of the handling loop, but should just
    // return failure ideication to the client
    // TODO: Do we want to do some form of error logging?
    match res {
        Ok(res) => res,
        Err(err) => err.into(),
    }
}
