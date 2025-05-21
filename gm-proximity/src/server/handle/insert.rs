use geolib::qt::datum::{BaseData, Datum};

use crate::message::prelude::*;

use super::Context;

// TODO: The quadtree has to keep track of last insert id, which is not the count if we allow deletes.
// TODO: Potentially handle batching, especially if the actual insertion work gets handed off to a worker pool.
// At the moment this will lock the RWLock for all the inserts
pub fn insert(handler: Context, insert: DataStream) {
    let mut qt = handler.write_qt();
    let mut insert_count: usize = 0;
    let mut error_count: usize = 0;

    for geom in &insert {
        // We are using the error by counting it
        let _ = geom
            .and_then(|f| Ok(Datum::new(f, BaseData::None, 0)))
            .and_then(|d| Ok(qt.insert(d)?))
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
        let (rx, handler) = Context::test_new(MsgToken::new(0, 0));

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
