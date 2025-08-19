use std::sync::Arc;

use anyhow::Result;
use geo::{Geometry, ToRadians};
use spatial::ProximitySearch;

use crate::{message::prelude::*, server::geo_store::GeoStore};

use super::Context;

/// Run a knn with the provided [`KnnReq`].
///
/// When the incoming data type is geojson Features we do a radians conversion before passing to knn. This therefore
/// assumes that incoming geojson is in decimal degrees as it should be according to the spec.
///
/// TODO: This is where filtering should be implemented as a first pass, but could push down to the geostore on a
/// specific method if we have a filter
pub fn knn(handler: Context, req: KnnReq) {
    // TODO: Handle custom key format for the find
    let response = match req.data {
        FindData::Features(shapes) => {
            process_geom_stream(&handler, req.content_mode, req.k, req.r, shapes.into_iter())
        }
        // TODO: Here we need to pull the shape from the Map storage and pass it to the quadtree
        FindData::Keys(keys) => process_key_stream(&handler, req.content_mode, req.k, req.r, &keys),
    };

    handler.send(Response::ProximityResults(response));
    // TODO: Here we are not chunking, but because knn might return multiple responses, just testing done
    handler.send(Response::Done(1));
}

// TODO: If there are more settings, bundle them into a QtSettings struct
// TODO: Push the results directly into the output message and send (probably one result per knn input row)
// TODO: Do we want to return some form of error code rather than a string to keep the size down?
fn process_geom_stream(
    handler: &Context,
    content_mode: ContentMode,
    k: usize,
    r: Option<f64>,
    geoms: impl Iterator<Item = Feature>,
) -> Vec<Result<ProximityResult, String>> {
    geoms
        .enumerate()
        .flat_map(|(i, geom)| {
            exec_knn_on_feature(&handler.store.load(), content_mode, i, k, r, geom)
        })
        .collect()
}

// TODO: Need to eliminate as much intermediate collection as we can here and in key stream - Impl Iter return?
#[inline(always)]
fn exec_knn_on_feature<'a>(
    store: &'a Arc<GeoStore>,
    content_mode: ContentMode,
    i: usize,
    k: usize,
    r: Option<f64>,
    feature: Feature,
) -> Vec<Result<ProximityResult, String>> {
    match Geometry::try_from(feature.0) {
        Ok(mut geom) => {
            // NOTE: Convert incoming feature geometries to radians
            geom.to_radians_in_place();
            exec_neighbor_search(store, content_mode, i, r, &geom)
                .take(k)
                .collect()
        }
        Err(err) => vec![Err(err.to_string())],
    }
}

// TODO: This doesn't have to be a result as we are not able to fail here
#[inline(always)]
fn exec_neighbor_search<'a>(
    store: &'a Arc<GeoStore>,
    content_mode: ContentMode,
    i: usize,
    r: Option<f64>,
    geom: &Geometry,
) -> impl Iterator<Item = Result<ProximityResult, String>> {
    store
        .within_radius(geom, r.unwrap_or(std::f64::INFINITY))
        .map(move |(record, distance)| {
            let content = content_mode.with_record(&record);

            Ok(ProximityResult {
                input_index: i,
                id: record.id,
                distance,
                content,
            })
        })
}

fn process_key_stream(
    handler: &Context,
    content_mode: ContentMode,
    k: usize,
    r: Option<f64>,
    keys: &KeySet,
) -> Vec<Result<ProximityResult, String>> {
    let store = handler.store.load();
    let mut results = Vec::new();

    match keys {
        KeySet::Uid(keys) => {
            for (i, key) in keys.iter().enumerate() {
                if let Some(gr) = store.get(key) {
                    results.extend(
                        exec_neighbor_search(&store, content_mode, i, r, &gr.geometry)
                            .filter(|res| res.as_ref().map(|item| &item.id != key).unwrap_or(true))
                            .take(k),
                    );
                }
            }
        }
        KeySet::Custom(keys) => {
            for (i, key) in keys.iter().enumerate() {
                if let Some(gr) = store.get_with_custom_key(key) {
                    results.extend(
                        exec_neighbor_search(&store, content_mode, i, r, &gr.geometry)
                            .filter(|res| res.as_ref().map(|item| item.id != gr.id).unwrap_or(true))
                            .take(k),
                    );
                }
            }
        }
    }

    results
}
