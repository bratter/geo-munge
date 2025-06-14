use std::sync::atomic::Ordering;

use crate::message::prelude::*;

use super::{handle::KeyGenerator, Context};

// TODO: The quadtree has to keep track of last insert id, which is not the count if we allow deletes.
// TODO: Potentially handle batching, especially if the actual insertion work gets handed off to a worker pool.
pub fn insert(handler: Context, insert: DataStream) {
    let mut insert_count: usize = 0;
    let mut error_count: usize = 0;
    let geo_store = handler.store.load();

    for geom in &insert {
        let id = match &**handler.key_gen.load() {
            KeyGenerator::AutoIncrement(id_gen) => id_gen.fetch_add(1, Ordering::Relaxed),
            // TODO: This has to extract from the metadata - which needs to be added to the DS along with the id
            KeyGenerator::CustomU32(_) => todo!(),
            KeyGenerator::MetaPointer(id_gen, _) => id_gen.fetch_add(1, Ordering::Relaxed),
        };

        // We are using the error by counting it
        let _ = geom
            // TODO: When we have optional metadata we need to use a different insert method... or just one that takes
            // an option...
            .and_then(|g| Ok(geo_store.insert(id, g)?))
            .inspect(|_| insert_count += 1)
            .inspect_err(|_| error_count += 1);
    }

    // TODO: Consider adding failure reasons
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
}
