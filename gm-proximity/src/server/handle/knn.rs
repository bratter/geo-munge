use std::sync::Arc;

use anyhow::Result;
use geo::{Geometry, ToRadians};
use spatial::Knn as KnnTrait;

use crate::{message::prelude::*, server::geo_store::GeoStore};

use super::Context;

/// Run a knn with the provided [`KnnReq`].
///
/// When the incoming data type is geojson Features we do a radians conversion before passing to knn. This therefore
/// assumes that incoming geojson is in decimal degrees as it should be according to the spec.
///
/// FIX: The output data type here needs to include some notion of what the input data was
/// FIX: The index impls need to have fully lazy iterators to do filtering or it won't work as the filtering needs to
/// take k and can't allocate a Vec in the knn method
/// TODO: This is where filtering should be implemented as a first pass, but could push down to the geostore on a
/// specific method if we have a filter
pub fn knn(handler: Context, knn: KnnReq) {
    // TODO: Handle custom key format for the find
    let response = match knn.data {
        FindData::Features(shapes) => {
            process_geom_stream(&handler, knn.k, knn.r, shapes.into_iter())
        }
        // TODO: Here we need to pull the shape from the Map storage and pass it to the quadtree
        FindData::Keys(keys) => process_key_stream(&handler, knn.k, knn.r, &keys),
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
    geoms: impl Iterator<Item = Feature>,
) -> Vec<Result<KnnItem, String>> {
    geoms
        .enumerate()
        .flat_map(|(i, geom)| exec_knn_on_feature(&handler.store.load(), i, k, r, geom))
        .collect()
}

// TODO: Need to eliminate as much intermediate collection as we can here and in key stream - Impl Iter return?
#[inline(always)]
fn exec_knn_on_feature<'a>(
    store: &'a Arc<GeoStore>,
    i: usize,
    k: usize,
    r: Option<f64>,
    feature: Feature,
) -> Vec<Result<KnnItem, String>> {
    match Geometry::try_from(feature.0) {
        Ok(mut geom) => {
            // NOTE: Convert incoming feature geometries to radians
            geom.to_radians_in_place();
            exec_knn(store, i, k, r, &geom).collect()
        }
        Err(err) => vec![Err(err.to_string())],
    }
}

#[inline(always)]
fn exec_knn<'a>(
    store: &'a Arc<GeoStore>,
    i: usize,
    k: usize,
    r: Option<f64>,
    geom: &Geometry,
) -> impl Iterator<Item = Result<KnnItem, String>> {
    store
        .knn_r(geom, k, r.unwrap_or(std::f64::INFINITY))
        .map(move |res| {
            Ok(KnnItem {
                index: i,
                uid: res.0.id,
                distance: res.1,
            })
        })
}

fn process_key_stream(
    handler: &Context,
    k: usize,
    r: Option<f64>,
    keys: &KeySet,
) -> Vec<Result<KnnItem, String>> {
    let store = handler.store.load();
    let mut results = Vec::new();

    // TODO: Revist self-exclusion logic when the knn method is fixed, likely just eliminate the +1
    match keys {
        KeySet::Uid(keys) => {
            for (i, key) in keys.iter().enumerate() {
                if let Some(gr) = store.get(key) {
                    results.extend(
                        exec_knn(&store, i, k + 1, r, &gr.geometry)
                            .filter(|res| res.as_ref().map(|item| &item.uid != key).unwrap_or(true))
                            .take(k),
                    );
                }
            }
        }
        KeySet::Custom(keys) => {
            for (i, key) in keys.iter().enumerate() {
                if let Some(gr) = store.get_with_custom_key(key) {
                    results.extend(
                        exec_knn(&store, i, k + 1, r, &gr.geometry)
                            .filter(|res| {
                                res.as_ref().map(|item| item.uid != gr.id).unwrap_or(true)
                            })
                            .take(k),
                    );
                }
            }
        }
    }

    results
}
