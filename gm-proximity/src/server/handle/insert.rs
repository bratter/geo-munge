use std::sync::{Arc, RwLock};

use anyhow::Result;
use geolib::qt::{
    datum::{BaseData, Datum},
    Geometry, Quadtree, ToRadians,
};

use crate::message::{Insert, Response};

// TODO: The quadtree has to keep track of last insert id, which is not the count if we allow deletes.
// TODO: Potentially handle batching, especially if the actual insertion work gets handed off to a worker pool.
// At the moment this will lock the RWLock for all the inserts
pub fn insert(qt: Arc<RwLock<Quadtree>>, insert: Insert) -> Result<Response> {
    // NOTE: Ok to propagate the panic with unwrap as the only error is for a poisoned RwLock
    let mut qt = qt.write().unwrap();
    let mut insert_count: usize = 0;
    let mut error_count: usize = 0;

    for feature in &insert {
        // We are using the error by counting it
        let _ = feature
            .and_then(|f| {
                let mut f: Geometry<f64> = geo::Geometry::try_from(f)?.try_into()?;
                f.to_radians_in_place();
                Ok(f)
            })
            .and_then(|f| Ok(Datum::new(f, BaseData::None, 0)))
            .and_then(|d| Ok(qt.insert(d)?))
            .inspect(|_| insert_count += 1)
            .inspect_err(|_| error_count += 1);
    }

    // TODO: Respond with the error reasons collecting errors from the results
    Ok(Response::Success(Some(format!(
        "{} inserted, {} errors",
        insert_count, error_count
    ))))
}

#[cfg(test)]
mod test {
    use crate::{message::Reset, server::run::build_qt};

    use super::*;

    use std::path::PathBuf;

    #[test]
    fn inserts_ndjson_requests() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../data/sample_geojson/sample.ndjson");
        let insert_val = Insert::from(std::fs::read(path).unwrap().to_vec());
        let qt = Arc::new(RwLock::new(build_qt(Reset::default())));

        let res = insert(qt, insert_val).unwrap();

        // TODO: We should be able to pull out of the response the number of successful inserts directly
        if let Response::Success(Some(s)) = res {
            assert_eq!(s.chars().nth(0).unwrap(), '3');
        } else {
            panic!("Not a Success(Some(_))");
        }
    }
}
