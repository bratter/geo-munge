use geo::{Rect, ToRadians};
use spatial::RegionQuery;

use crate::message::prelude::*;

use super::{record_to_basic_result, Context, MAX_GEOM_BATCH_SIZE, MAX_ID_BATCH_SIZE};

/// Run a "window" bounding box query using the provided bounding box.
///
/// We convert the incoming degree-based bounding box into a radian-based [`Rect`] in this handler.
pub fn window(context: Context, req: WindowReq) {
    let mut rect = Rect::from(req.bbox);
    rect.to_radians_in_place();

    let store = context.store.load();
    let batch_size = match req.content_mode {
        ContentMode::None => MAX_ID_BATCH_SIZE,
        _ => MAX_GEOM_BATCH_SIZE,
    } as usize;
    let mut items = Vec::with_capacity(batch_size);
    let mut response_count = 0;

    match req.join {
        JoinType::Intersects => {
            let iter = store
                .intersecting(&rect)
                .map(|record| Ok(record_to_basic_result(req.content_mode, &record)));

            for item in iter {
                if items.len() >= batch_size {
                    let batch = std::mem::replace(&mut items, Vec::with_capacity(batch_size));
                    context.send(Response::BasicResults(batch));
                    response_count += 1;
                }
                items.push(item);
            }
        }
        JoinType::Contains => {
            let iter = store
                .contained_by(&rect)
                .map(|record| Ok(record_to_basic_result(req.content_mode, &record)));

            for item in iter {
                if items.len() >= batch_size {
                    let batch = std::mem::replace(&mut items, Vec::with_capacity(batch_size));
                    context.send(Response::BasicResults(batch));
                    response_count += 1;
                }
                items.push(item);
            }
        }
    };

    // Final flush
    if !items.is_empty() {
        context.send(Response::BasicResults(items));
        response_count += 1;
    }
    context.send(Response::Done(response_count));
}
