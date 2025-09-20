use std::sync::Arc;

use protocol::prelude::*;

use super::{Context, GeoStore};

/// Resets the [`GeoStore`].
pub fn reset(context: Context, reset: ResetReq) {
    let bbox = reset.bbox.unwrap_or_default().into();
    let new_store = GeoStore::new(bbox, reset.key_mode);

    context.store.store(Arc::new(new_store));

    context.send(Response::Success(None));
}
