use std::sync::Arc;

use geo::{Geometry, ToRadians};
use spatial::ProximitySearch;

use crate::{
    message::prelude::*,
    server::geo_store::{GeoRecord, GeoStore},
};

use super::{Context, MAX_GEOM_BATCH_SIZE, MAX_ID_BATCH_SIZE};

/// Run a knn with the provided [`KnnReq`].
///
/// When the incoming data type is geojson Features we do a radians conversion before passing to knn. This therefore
/// assumes that incoming geojson is in decimal degrees as it should be according to the spec.
///
/// TODO: This is where filtering should be implemented as a first pass, but could push down to the geostore on a
/// specific method if we have a filter, note that filtering will also be required on all other collections, so should
/// be built in a common location and used everywhere
pub fn knn(context: Context, req: KnnReq) {
    match req.data {
        FindData::Features(feats) => process_geoms(
            &context,
            req.content_mode,
            req.k,
            req.r,
            req.start_index,
            feats,
        ),
        FindData::Keys(keys) => process_keys(
            &context,
            req.content_mode,
            req.k,
            req.r,
            req.start_index,
            &keys,
        ),
    }
}

fn process_geoms(
    context: &Context,
    content_mode: ContentMode,
    k: usize,
    r: Option<f64>,
    start_index: usize,
    geoms: Vec<Feature>,
) {
    let store = context.store.load();
    let batch_size = match content_mode {
        ContentMode::None => MAX_ID_BATCH_SIZE,
        _ => MAX_GEOM_BATCH_SIZE,
    } as usize;
    let mut items = Vec::with_capacity(batch_size);
    let mut response_count = 0;

    for (i, feature) in geoms.into_iter().enumerate() {
        match Geometry::try_from(feature.0) {
            Ok(mut geom) => {
                // NOTE: Convert incoming feature geometries to radians
                geom.to_radians_in_place();

                for neighbor in
                    exec_neighbor_search(&store, content_mode, start_index + i, None, r, &geom)
                        .map(Ok)
                        .take(k)
                {
                    if items.len() >= batch_size {
                        let batch = std::mem::replace(&mut items, Vec::with_capacity(batch_size));
                        context.send(Response::ProximityResults(batch));
                        response_count += 1;
                    }
                    items.push(neighbor);
                }
            }
            Err(err) => {
                if items.len() >= batch_size {
                    let batch = std::mem::replace(&mut items, Vec::with_capacity(batch_size));
                    context.send(Response::ProximityResults(batch));
                    response_count += 1;
                }
                items.push(Err(err.to_string()));
            }
        }
    }

    // Final flush
    if !items.is_empty() {
        context.send(Response::ProximityResults(items));
        response_count += 1;
    }
    context.send(Response::Done(response_count));
}

fn process_keys(
    context: &Context,
    content_mode: ContentMode,
    k: usize,
    r: Option<f64>,
    start_index: usize,
    keys: &KeySet,
) {
    let store = context.store.load();
    let batch_size = match content_mode {
        ContentMode::None => MAX_ID_BATCH_SIZE,
        _ => MAX_GEOM_BATCH_SIZE,
    } as usize;
    let mut items = Vec::with_capacity(batch_size);
    let mut response_count = 0;

    // NOTE: We have to manually batch here rather than using the helping in message/batch.rs as the nested iterator
    // structure cannot be flattened due to lifetime issues, preventing us from passing a flat iterator to the batch
    // Instead we set up a helper closure here to manage the additional complexity
    let mut process_record = |gr: &GeoRecord, i: usize, input_uid: NodeId| {
        for neighbor in
            exec_neighbor_search(&store, content_mode, i, Some(input_uid), r, &gr.geometry)
                .filter(|item| item.id != input_uid)
                .map(Ok)
                .take(k)
        {
            if items.len() >= batch_size {
                let batch = std::mem::replace(&mut items, Vec::with_capacity(batch_size));
                context.send(Response::ProximityResults(batch));
                response_count += 1;
            }
            items.push(neighbor);
        }
    };

    match keys {
        KeySet::Uid(keys) => {
            for (i, key) in keys.iter().enumerate() {
                if let Some(gr) = store.get(key) {
                    process_record(&gr, start_index + i, *key);
                }
            }
        }
        KeySet::Custom(keys) => {
            for (i, key) in keys.iter().enumerate() {
                if let Some(gr) = store.get_with_custom_key(key) {
                    process_record(&gr, start_index + i, gr.id);
                }
            }
        }
    }

    // Final flush
    if !items.is_empty() {
        context.send(Response::ProximityResults(items));
        response_count += 1;
    }
    context.send(Response::Done(response_count));
}

#[inline]
fn exec_neighbor_search<'a>(
    store: &'a Arc<GeoStore>,
    content_mode: ContentMode,
    input_index: usize,
    input_uid: Option<NodeId>,
    r: Option<f64>,
    geom: &Geometry,
) -> impl Iterator<Item = ProximityResult> {
    store
        .within_radius(geom, r.unwrap_or(std::f64::MAX))
        .map(move |(record, distance)| {
            let content = content_mode.with_record(&record);

            ProximityResult {
                input_index,
                input_uid,
                id: record.id,
                distance,
                content,
            }
        })
}
