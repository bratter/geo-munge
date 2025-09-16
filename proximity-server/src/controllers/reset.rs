use std::sync::Arc;

use protocol::prelude::*;

use super::{Context, GeoStore};

/// Resets the [`GeoStore`].
pub fn reset(context: Context, reset: ResetReq) {
    let bbox = reset.bbox.unwrap_or_default().into();

    let new_store = match reset.key_mode {
        KeyMode::MetaPointer(ref ptr) => GeoStore::with_custom_key(bbox, ptr.clone()),
        _ => GeoStore::new(bbox),
    };

    context.key_gen.store(Arc::new(reset.key_mode.into()));
    context.store.store(Arc::new(new_store));

    context.send(Response::Success(None));
}
