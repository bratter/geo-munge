use crate::message::prelude::*;

use super::Context;

/// Insert records from the incoming data stream into the store.
///
/// Generates an iterator for insertion based on incoming features, leaving it up to the store to batch as appropriate.
///
/// This insert method will convert all incoming geometries to radians. Therefore all incoming [`JsonFeature`]'s must be in
/// decimal degrees (which they should be if they are valid geojson).
pub fn insert(handler: Context, insert: Vec<JsonFeature>) {
    let mut error_count: usize = 0;

    let key_gen = &**handler.key_gen.load();
    let insert_iter = insert.into_iter().filter_map(|json| {
        let feature = ParsedFeature::try_from(json.0).and_then(|f| f.with_key_generator(key_gen));

        match feature {
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
