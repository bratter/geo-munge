use crate::message::{dispatch_counted_batches, prelude::*};

use super::{record_to_basic_result, Context, MAX_GEOM_BATCH_SIZE, MAX_ID_BATCH_SIZE};

pub fn get(context: Context, req: GetReq) {
    let store = context.store.load();
    let batch_size = match req.content_mode {
        ContentMode::None => MAX_ID_BATCH_SIZE,
        _ => MAX_GEOM_BATCH_SIZE,
    };
    let mut response_count = 0;

    match req.keys {
        KeySet::Uid(keys) => {
            let iter = keys.iter().map(|key| {
                store
                    .get(&key)
                    .ok_or_else(|| format!("Record with key {} not found", key))
                    .map(|record| record_to_basic_result(req.content_mode, &record))
            });
            response_count += dispatch_counted_batches(iter, batch_size, |batch| {
                context.send(Response::BasicResults(batch));
                Ok(())
            })
            .expect("infallible");
        }
        KeySet::Custom(keys) => {
            let iter = keys.iter().map(|key| {
                store
                    .get_with_custom_key(&key)
                    .ok_or_else(|| format!("Record with key {:x} not found", key))
                    .map(|record| record_to_basic_result(req.content_mode, &record))
            });
            response_count += dispatch_counted_batches(iter, batch_size, |batch| {
                context.send(Response::BasicResults(batch));
                Ok(())
            })
            .expect("infallible");
        }
    };

    // Notify completion
    context.send(Response::Done(response_count));
}
