use std::sync::{atomic::Ordering, Arc};

use anyhow::{anyhow, Result};
use arc_swap::Guard;
use geo::{Geometry, ToRadians};
use geojson::JsonValue;

use crate::message::prelude::*;

use super::{handle::KeyGenerator, Context};

/// Insert records from the incoming data stream into the store.
///
/// Generates an iterator for insertion based on incoming features, leaving it up to the store to batch as appropriate.
///
/// This insert method will convert all incoming geometries to radians. Therefore all incoming [`Feature`]'s must be in
/// decimal degrees (which they should be if they are valid geojson).
///
/// TODO: Is this the best place for radian conversion? Should it be done with types?
pub fn insert(handler: Context, insert: Vec<Feature>) {
    let mut error_count: usize = 0;

    let insert_iter = insert.into_iter().filter_map(|feature| {
        match prepare_insert(handler.key_gen.load(), feature) {
            Ok(value) => Some(value),
            Err(_) => {
                error_count += 1;
                None
            }
        }
    });
    let (insert_count, insert_errors) = handler.store.load().bulk_insert(insert_iter);

    handler.send(Response::ResultCounts {
        success: insert_count,
        fail: error_count + insert_errors,
    })
}

fn prepare_insert(
    key_type: Guard<Arc<KeyGenerator>>,
    feature: Feature,
) -> Result<(NodeId, Geometry, Option<JsonValue>)> {
    let mut feature = feature.0;
    let meta = std::mem::take(&mut feature.properties).map(JsonValue::Object);

    let uid = match &**key_type {
        KeyGenerator::AutoIncrement(id_gen) => id_gen.fetch_add(1, Ordering::Relaxed),
        KeyGenerator::CustomU32(ptr) => meta
            .as_ref()
            .and_then(|json| json.pointer(ptr.as_str()))
            .and_then(|v| v.as_u64())
            .and_then(|v| v.try_into().ok())
            .ok_or(anyhow!("Insert failed: Could not generate CustomU32 id"))?,
        KeyGenerator::MetaPointer(id_gen, _) => id_gen.fetch_add(1, Ordering::Relaxed),
    };

    // NOTE: We ensure conversion to radians on insert
    let mut geom = Geometry::<f64>::try_from(feature)?;
    geom.to_radians_in_place();

    Ok((uid, geom, meta))
}

#[cfg(test)]
mod test {
    use std::{path::PathBuf, time::Duration};

    use super::*;

    use crate::{connection::MsgToken, input_io::Input};

    #[test]
    fn inserts_ndjson_requests() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../data/sample_geojson/sample.ndjson");
        let insert_val = Input::try_new(Some(path))
            .unwrap()
            .into_feature_iter()
            .filter_map(|res| if let Ok((_, f)) = res { Some(f) } else { None })
            .collect();
        let ctx = Context::make_store();
        let (rx, handler) = Context::test_new(&ctx, MsgToken::new(0, 0));

        insert(handler, insert_val);

        match rx.recv_timeout(Duration::from_millis(0)) {
            Ok((_, Response::ResultCounts { success, fail })) => {
                assert_eq!(success, 3);
                assert_eq!(fail, 0);
            }
            Ok(res) => panic!("Wrong response type: {:?}", res),
            Err(err) => panic!("Response failed: {}", err),
        }
    }

    #[test]
    fn insert_with_custom_id() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../data/sample_geojson/sample_with_id.ndjson");
        let insert_val = Input::try_new(Some(path))
            .unwrap()
            .into_feature_iter()
            .filter_map(|res| if let Ok((_, f)) = res { Some(f) } else { None })
            .collect();
        let ctx = Context::make_store();
        let (rx, handler) = Context::test_new(&ctx, MsgToken::new(0, 0));

        insert(handler, insert_val);

        match rx.recv_timeout(Duration::from_millis(0)) {
            Ok((_, Response::ResultCounts { success, fail })) => {
                assert_eq!(success, 3);
                assert_eq!(fail, 0);
            }
            Ok(res) => panic!("Wrong response type: {:?}", res),
            Err(err) => panic!("Response failed: {}", err),
        }
    }
}
