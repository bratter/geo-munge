use std::sync::Arc;

use crate::{message::prelude::*, server::geo_store::GeoStore};

use super::Context;

/// Resets the [`GeoStore`].
pub fn reset(context: Context, reset: ResetReq) {
    let new_store = match reset.key_mode {
        KeyMode::MetaPointer(ref ptr) => GeoStore::with_custom_key(ptr.clone()),
        _ => GeoStore::new(),
    };

    context.key_gen.store(Arc::new(reset.key_mode.into()));
    context.store.store(Arc::new(new_store));

    context.send(Response::Success(None));
}
