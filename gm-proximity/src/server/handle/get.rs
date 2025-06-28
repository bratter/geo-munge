use crate::{message::prelude::*, server::geo_store::GeoRecord};

use super::Context;

pub fn get(context: Context, get_req: GetReq) {
    let store = context.store.load();

    // TODO: Batching here - cap out on size only should be fine
    // TODO: Error enum rather than string sent back to client
    match (get_req.meta_only, get_req.keys) {
        (false, KeySet::Uid(keys)) => {
            let mut items = Vec::with_capacity(keys.len());
            for key in keys {
                let item = store
                    .get(&key)
                    .ok_or_else(|| format!("Record with key {} not found", key))
                    .map(record_to_feature);
                items.push(item);
            }

            context.send(Response::FeatureData(items));
        }
        (false, KeySet::Custom(keys)) => {
            let mut items = Vec::with_capacity(keys.len());
            for key in keys {
                let item = store
                    .get_with_custom_key(&key)
                    .ok_or_else(|| format!("Record with key {:x} not found", key))
                    .map(record_to_feature);
                items.push(item);
            }

            context.send(Response::FeatureData(items));
        }
        (true, KeySet::Uid(keys)) => {
            let mut items = Vec::with_capacity(keys.len());
            for key in keys {
                let item = store
                    .get(&key)
                    .ok_or_else(|| format!("Record with key {} not found", key))
                    .map(record_to_meta);
                items.push(item);
            }

            context.send(Response::MetaData(items));
        }
        (true, KeySet::Custom(keys)) => {
            let mut items = Vec::with_capacity(keys.len());
            for key in keys {
                let item = store
                    .get_with_custom_key(&key)
                    .ok_or_else(|| format!("Record with key {:x} not found", key))
                    .map(record_to_meta);
                items.push(item);
            }

            context.send(Response::MetaData(items));
        }
    };

    context.send(Response::Done(1));
}

#[inline(always)]
fn record_to_feature(record: GeoRecord) -> Feature {
    geojson::Feature::from(&*record).into()
}

fn record_to_meta(record: GeoRecord) -> (NodeId, JsonValue) {
    (
        record.id,
        geojson::JsonValue::from(record.metadata.clone()).into(),
    )
}
