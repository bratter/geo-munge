use anyhow::Result;
use geolib::qt::{Geometry, Quadtree};

use crate::message::prelude::*;

use super::Context;

// TODO: Just fix this to see what the problem is
pub fn knn(handler: Context, knn: Knn) {
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
// TODO: When this evolves into a processing thread, we can send results back to the output queue over a channel
// however we want, but for the time being we just collect ans send as data.
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
    geoms: impl Iterator<Item = Result<Geometry<f64>>>,
) -> Vec<Result<(usize, f64), String>> {
    let qt = &*handler.read_qt();

    geoms
        .flat_map(|geom| match geom {
            Ok(g) => exec_knn(&qt, k, r, g),
            Err(err) => vec![Err(err.to_string())],
        })
        .collect()
}

fn exec_knn<'a>(
    qt: &'a Quadtree,
    k: usize,
    r: Option<f64>,
    item: Geometry<f64>,
) -> Vec<Result<(usize, f64), String>> {
    match qt.knn_from_geom(item, k, r) {
        Ok(result) => result
            .into_iter()
            // TODO: The quadtree itself should return a better response
            .map(|(datum, distance)| Ok((datum.index(), distance)))
            .collect(),
        Err(err) => vec![Err(err.to_string())],
    }
}
