use std::sync::atomic::Ordering;

use geo::Geometry;
use geojson::JsonValue;

use crate::message::prelude::*;

use super::{handle::KeyGenerator, Context};

// TODO: Potentially handle batching, especially if the actual insertion work gets handed off to a worker pool.
pub fn insert(handler: Context, insert: DataStream) {
    let mut insert_count: usize = 0;
    let mut error_count: usize = 0;
    let geo_store = handler.store.load();

    for feature_result in &insert {
        if let Ok(mut feature) = feature_result {
            let meta = std::mem::take(&mut feature.properties).map(JsonValue::Object);

            let uid = match &**handler.key_gen.load() {
                KeyGenerator::AutoIncrement(id_gen) => id_gen.fetch_add(1, Ordering::Relaxed),
                KeyGenerator::CustomU32(ptr) => {
                    let uid_opt: Option<u32> = meta
                        .as_ref()
                        .and_then(|json| json.pointer(ptr.as_str()))
                        .and_then(|v| v.as_u64())
                        .and_then(|v| v.try_into().ok());

                    if let Some(uid) = uid_opt {
                        uid
                    } else {
                        error_count += 1;
                        tracing::trace!("Insert failed: Could not generate CustomU32 id");
                        continue;
                    }
                }
                KeyGenerator::MetaPointer(id_gen, _) => id_gen.fetch_add(1, Ordering::Relaxed),
            };

            // We are using the error by counting it
            if let Ok(geom) = Geometry::<f64>::try_from(feature) {
                let _ = geo_store
                    .insert(uid, geom, meta)
                    .inspect(|_| {
                        insert_count += 1;
                        tracing::trace!("Inserted uid {}", uid);
                    })
                    .inspect_err(|err| {
                        error_count += 1;
                        tracing::trace!("Insert failed: {}", err);
                    });
            } else {
                error_count += 1;
                tracing::trace!("Insert failed: Could not convert geometry")
            }
        } else {
            error_count += 1;
            tracing::trace!("Insert failed: Error in input datastream");
        }
    }

    // TODO: Consider adding failure reasons and/or ids instead of just a count
    handler.send(Response::InsertResult {
        success: insert_count,
        fail: error_count,
    })
}

#[cfg(test)]
mod test {
    use std::{path::PathBuf, time::Duration};

    use super::*;

    use crate::connection::MsgToken;

    #[test]
    fn inserts_ndjson_requests() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../data/sample_geojson/sample.ndjson");
        let insert_val = DataStream::from(std::fs::read(path).unwrap().to_vec());
        let ctx = Context::make_store();
        let (rx, handler) = Context::test_new(&ctx, MsgToken::new(0, 0));

        insert(handler, insert_val);

        match rx.recv_timeout(Duration::from_millis(0)) {
            Ok((_, Response::InsertResult { success, fail })) => {
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
        let insert_val = DataStream::from(std::fs::read(path).unwrap().to_vec());
        let ctx = Context::make_store();
        let (rx, handler) = Context::test_new(&ctx, MsgToken::new(0, 0));

        insert(handler, insert_val);

        match rx.recv_timeout(Duration::from_millis(0)) {
            Ok((_, Response::InsertResult { success, fail })) => {
                assert_eq!(success, 3);
                assert_eq!(fail, 0);
            }
            Ok(res) => panic!("Wrong response type: {:?}", res),
            Err(err) => panic!("Response failed: {}", err),
        }
    }
}
