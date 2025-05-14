use std::sync::{Arc, RwLock};

use anyhow::Result;
use geolib::qt::{
    datum::{BaseData, Datum},
    Quadtree,
};

use crate::message::prelude::*;

// TODO: The quadtree has to keep track of last insert id, which is not the count if we allow deletes.
// TODO: Potentially handle batching, especially if the actual insertion work gets handed off to a worker pool.
// At the moment this will lock the RWLock for all the inserts
pub fn insert(qt: &Arc<RwLock<Quadtree>>, insert: DataStream) -> Result<Response> {
    // NOTE: Ok to propagate the panic with unwrap as the only error is for a poisoned RwLock
    let mut qt = qt.write().unwrap();
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
    Ok(Response::InsertResult {
        success: insert_count,
        fail: error_count,
    })
}

#[cfg(test)]
mod test {
    use crate::server::run::build_qt;

    use super::*;

    use std::path::PathBuf;

    #[test]
    fn inserts_ndjson_requests() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../data/sample_geojson/sample.ndjson");
        let insert_val = DataStream::from(std::fs::read(path).unwrap().to_vec());
        let qt = Arc::new(RwLock::new(build_qt(Reset::default())));

        let res = insert(&qt, insert_val).unwrap();

        if let Response::InsertResult { success, fail } = res {
            assert_eq!(success, 3);
            assert_eq!(fail, 0);
        } else {
            panic!("Not a Success(Some(_))");
        }
    }
}
