use std::sync::Arc;

use anyhow::Result;
use geo::Geometry;
use geojson::Feature;

use crate::{
    message::prelude::*,
    server::geo_store::{GeoStore, Knn as KnnTrait, NodeId},
};

use super::Context;

pub fn knn(handler: Context, knn: KnnReq) {
    // TODO: Handle custom key format for the find
    let response = match &knn.data {
        FindData::Geom(shapes) => process_geom_stream(&handler, knn.k, knn.r, shapes.into_iter()),
        // TODO: Here we need to pull the shape from the Map storage and pass it to the quadtree
        FindData::Keys(keys) => process_key_stream(&handler, knn.k, knn.r, keys.as_slice()),
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
    geoms: impl Iterator<Item = Result<Feature>>,
) -> Vec<Result<(u32, f64), String>> {
    geoms
        .flat_map(|geom| match geom {
            Ok(g) => exec_knn_on_feature(&handler.store.load(), k, r, g),
            Err(err) => vec![Err(err.to_string())],
        })
        .collect()
}

// TODO: Knn should only return the usize id and the distance - needs to be mapped here
// TODO: Need to eliminate as much intermediate collection as we can here and in key stream - Impl Iter return?
#[inline(always)]
fn exec_knn_on_feature<'a>(
    qt: &'a Arc<GeoStore>,
    k: usize,
    r: Option<f64>,
    feature: Feature,
) -> Vec<Result<(u32, f64), String>> {
    match Geometry::try_from(feature) {
        Ok(geom) => exec_knn(qt, k, r, &geom).collect(),
        Err(err) => vec![Err(err.to_string())],
    }
}

#[inline(always)]
fn exec_knn<'a>(
    qt: &'a Arc<GeoStore>,
    k: usize,
    r: Option<f64>,
    geom: &Geometry,
) -> impl Iterator<Item = Result<(u32, f64), String>> {
    qt.knn_r(geom, k, r.unwrap_or(std::f64::INFINITY))
        .map(|res| Ok((res.0.id, res.1)))
}

fn process_key_stream(
    handler: &Context,
    k: usize,
    r: Option<f64>,
    keys: &[NodeId],
) -> Vec<Result<(u32, f64), String>> {
    let store = handler.store.load();
    let mut results = Vec::new();

    for key in keys {
        if let Some(gr) = store.get(*key) {
            results.extend(exec_knn(&store, k, r, &gr.geometry));
        } else {
            todo!()
        }
    }

    results
}
