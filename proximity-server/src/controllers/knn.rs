use std::sync::Arc;

use geo::Geometry;
use protocol::prelude::*;
use spatial::{ProximitySearch, EARTH_RADIUS_METERS};

use crate::geo::{GeoStore, ParsedFeature, Record};

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
    // Convert from meters input to radians
    let r = req.r.map(|r| r / EARTH_RADIUS_METERS);

    match req.data {
        FindData::Features(feats) => {
            process_geoms(&context, req.content_mode, req.k, r, req.start_index, feats)
        }
        FindData::Keys(keys) => {
            process_keys(&context, req.content_mode, req.k, r, req.start_index, &keys)
        }
    }
}

fn process_geoms(
    context: &Context,
    content_mode: ContentMode,
    k: usize,
    r: Option<f64>,
    start_index: usize,
    geoms: Vec<JsonFeature>,
) {
    let store = context.store.load();
    let batch_size = match content_mode {
        ContentMode::None => MAX_ID_BATCH_SIZE,
        _ => MAX_GEOM_BATCH_SIZE,
    } as usize;
    let mut items = Vec::with_capacity(batch_size);
    let mut response_count = 0;

    for (i, json) in geoms.into_iter().enumerate() {
        // NOTE: ParsedFeature does radians conversion
        match ParsedFeature::try_from(json.0).map(|f| f.geometry) {
            Ok(geom) => {
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
    let mut process_record = |record: &Record, i: usize, input_uid: Uid| {
        let geom = &record.data.geometry;
        for neighbor in exec_neighbor_search(&store, content_mode, i, Some(input_uid), r, geom)
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
                if let Some(record) = store.get_with_custom_key(key) {
                    process_record(&record, start_index + i, record.data.id);
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
    input_uid: Option<Uid>,
    r: Option<f64>,
    geom: &Geometry,
) -> impl Iterator<Item = ProximityResult> {
    store
        .within_radius(geom, r.unwrap_or(std::f64::MAX))
        .map(move |(record, distance)| {
            let content = record.data.generate_content(content_mode);

            ProximityResult {
                input_index,
                input_uid,
                id: record.data.id,
                distance_meters: distance * EARTH_RADIUS_METERS,
                content,
            }
        })
}
