use crate::message::prelude::*;

use super::{record_to_basic_result, Context};

pub fn get(context: Context, req: GetReq) {
    let store = context.store.load();

    // TODO: Is there an efficienct way to merge these two branches
    // TODO: Batching here - cap out on size only should be fine
    // TODO: Error enum rather than string sent back to client
    match req.keys {
        KeySet::Uid(keys) => {
            let mut items = Vec::with_capacity(keys.len());
            for key in keys {
                let item = store
                    .get(&key)
                    .ok_or_else(|| format!("Record with key {} not found", key))
                    .map(|record| record_to_basic_result(req.content_mode, &record));
                items.push(item);
            }

            context.send(Response::BasicResults(items));
        }
        KeySet::Custom(keys) => {
            let mut items = Vec::with_capacity(keys.len());
            for key in keys {
                let item = store
                    .get_with_custom_key(&key)
                    .ok_or_else(|| format!("Record with key {:x} not found", key))
                    .map(|record| record_to_basic_result(req.content_mode, &record));
                items.push(item);
            }

            context.send(Response::BasicResults(items));
        }
    };

    context.send(Response::Done(1));
}
