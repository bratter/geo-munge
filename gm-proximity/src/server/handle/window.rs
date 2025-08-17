use geo::{Rect, ToRadians};
use spatial::RegionQuery;

use crate::{message::prelude::*, server::geo_store::GeoRecord};

use super::Context;

/// Run a "window" bounding box query using the provided bounding box.
///
/// We convert the incoming degree-based bounding box into a radian-based [`Rect`] in this handler.
pub fn window(handler: Context, req: WindowReq) {
    let mut rect = Rect::from(req.bbox);
    rect.to_radians_in_place();

    let store = handler.store.load();

    // TODO: Wten we chunk we want the blocking send to be in the iterator to make it lazy
    let response: Vec<_> = match req.join {
        JoinType::Intersects => store.intersecting(&rect).map(record_to_response).collect(),
        JoinType::Contains => store.contained_by(&rect).map(record_to_response).collect(),
    };

    // TODO: KnnData with distance 0 is indeed appropriate, but is there a better name, or should we split the types?
    // Especially as we don't need the index either
    handler.send(Response::KnnData(response));
    // TODO: We are not chunking yet, but we should, so we need to have a done
    handler.send(Response::Done(1));
}

// TODO: Assuming we simplify the Knn result returning or use a different return type, we can drop the result
#[inline]
fn record_to_response(record: GeoRecord) -> Result<KnnItem, String> {
    Ok(KnnItem {
        index: 0,
        uid: record.id,
        distance: 0.0,
    })
}
