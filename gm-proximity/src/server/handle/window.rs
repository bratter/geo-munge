use geo::{Rect, ToRadians};
use spatial::RegionQuery;

use crate::message::prelude::*;

use super::{record_to_basic_result, Context};

/// Run a "window" bounding box query using the provided bounding box.
///
/// We convert the incoming degree-based bounding box into a radian-based [`Rect`] in this handler.
pub fn window(handler: Context, req: WindowReq) {
    let mut rect = Rect::from(req.bbox);
    rect.to_radians_in_place();

    let store = handler.store.load();

    // TODO: When we chunk we want the blocking send to be in the iterator to make it lazy
    let response: Vec<_> = match req.join {
        JoinType::Intersects => store
            .intersecting(&rect)
            .map(|record| Ok(record_to_basic_result(req.content_mode, &record)))
            .collect(),
        JoinType::Contains => store
            .contained_by(&rect)
            .map(|record| Ok(record_to_basic_result(req.content_mode, &record)))
            .collect(),
    };

    handler.send(Response::BasicResults(response));
    // TODO: We are not chunking yet, but we should, so we need to have a done
    handler.send(Response::Done(1));
}
