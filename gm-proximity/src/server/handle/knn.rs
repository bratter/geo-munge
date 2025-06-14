use std::sync::Arc;

use anyhow::Result;
use geo::Geometry;

use crate::{
    message::prelude::*,
    server::geo_store::{GeoStore, Knn as KnnT},
};

use super::Context;

// TODO: FIx the trait name vs the data name - change the data name
pub fn knn(handler: Context, knn: KnnReq) {
    // TODO: Handle other types of incoming find data formats
    let response = match &knn.data {
        FindData::Geom(shapes) => process_geom_stream(&handler, knn.k, knn.r, shapes.into_iter()),
        // TODO: Here we need to pull the shape from the Map storage and pass it to the quadtree
        FindData::Keys(_keys) => todo!(),
    };

    handler.send(Response::KnnData(response));
    // TODO: Here we are not chunking, but because knn might return multiple responses, just testing done
    handler.send(Response::Done(1));
}

// TODO: If there are more settings, bundle them into a QtSettings struct
// TODO: We need to think what metadata to return from the matched geom. At least the id, but possibly the rest as a setting.
// Same applies to the stored shape - don't want the overhead of returning it unless required by the client as the
// client should already know or could query afterwards
// TODO: Need better response type overall - a find result struct. Probably want the k-index as a member of the response too
// TODO: Option as to whether to return row-level errors or skip them
// TODO: Push the results directly into the output message and send (probably one result per knn input row)
// TODO: Do we want to return some form of error code rather than a string to keep the size down?
fn process_geom_stream(
    handler: &Context,
    k: usize,
    r: Option<f64>,
    geoms: impl Iterator<Item = Result<Geometry>>,
) -> Vec<Result<(u32, f64), String>> {
    geoms
        .flat_map(|geom| match geom {
            Ok(g) => exec_knn(&handler.store.load(), k, r, g),
            Err(err) => vec![Err(err.to_string())],
        })
        .collect()
}

// TODO: Knn should only return the usize id and the distance - needs to be mapped here
fn exec_knn<'a>(
    qt: &'a Arc<GeoStore>,
    k: usize,
    r: Option<f64>,
    item: Geometry,
) -> Vec<Result<(u32, f64), String>> {
    qt.knn_r(&item, k, r.unwrap_or(std::f64::INFINITY))
        .map(|res| Ok((res.0.id, res.1)))
        .collect()
}
