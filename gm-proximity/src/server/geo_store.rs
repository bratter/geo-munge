//! Module for main concurrent access data structures.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{anyhow, bail, Result};
use dashmap::DashMap;
use fxhash::FxBuildHasher;
use geo::{Geometry, Rect};
use geojson::JsonValue;
use spatial::{BasicQuadTree, BboxSearch, Knn, SpatialIndex};

// TODO: Move Bbox?
use crate::message::{prelude::Bbox, CustomKey, NodeId};

/// Base record containing the actual data.
pub struct GeoRecordInner {
    pub id: NodeId,
    pub geometry: Geometry<f64>,
    // TODO: Metadata is just JSON values, use JSON pointer syntax for extraction
    // https://datatracker.ietf.org/doc/html/rfc6901
    pub metadata: Option<JsonValue>,
    is_deleted: AtomicBool,
}

/// Shared record type used throughout the system.
pub type GeoRecord = Arc<GeoRecordInner>;

impl From<&GeoRecordInner> for geojson::Feature {
    fn from(value: &GeoRecordInner) -> Self {
        let mut feature: geojson::Feature = geojson::Geometry::from(&value.geometry).into();
        feature.id = Some(geojson::feature::Id::Number(value.id.into()));
        feature.properties = match &value.metadata {
            Some(JsonValue::Object(meta)) => Some(meta.clone()),
            None => None,
            _ => unreachable!(),
        };

        feature
    }
}

impl AsRef<Geometry> for GeoRecordInner {
    fn as_ref(&self) -> &Geometry {
        &self.geometry
    }
}

impl PartialEq<NodeId> for GeoRecordInner {
    fn eq(&self, other: &NodeId) -> bool {
        &self.id == other
    }
}

/// Main data store struct holding indexes and records.
///
/// The store works entirely on shared references so can be passed safely between threads without locking the outer
/// structure.
///
/// TODO: Hash function? Consider AHash
/// TODO: Other options for custom key, or make it generic to save space when not used
pub struct GeoStore {
    id_index: DashMap<NodeId, GeoRecord, FxBuildHasher>,
    custom_key: DashMap<CustomKey, GeoRecord, FxBuildHasher>,
    spatial_index: BasicQuadTree<GeoRecord>,
    custom_key_pointer: Option<String>,
}

// TODO: Should the bbox struct be pulled into this module instead?
impl GeoStore {
    pub fn new(bbox: geo::Rect) -> Self {
        GeoStore {
            id_index: DashMap::with_hasher(FxBuildHasher::new()),
            custom_key: DashMap::with_hasher(FxBuildHasher::new()),
            spatial_index: BasicQuadTree::new(bbox),
            custom_key_pointer: None,
        }
    }

    /// Create a new [`GeoStore`] that also stores a custom index.
    ///
    /// The custom index is extracted from each record's metadata using JSON Pointer passed with this call. This means
    /// that all entries using a custom key must have metadata. See https://datatracker.ietf.org/doc/html/rfc6901 for
    /// details on JSON pointer syntax.
    pub fn with_custom_key(bbox: Rect, key_ptr: String) -> Self {
        let mut store = Self::new(bbox);
        store.custom_key_pointer = Some(key_ptr);
        store
    }

    pub fn size(&self) -> usize {
        self.id_index.len()
    }

    /// Insert a new record with metadata.
    ///
    /// If using an additional custom key, metadata must be provided or the insert will fail.
    ///
    /// Will error if the key already exists.
    ///
    /// WARN: Revisit prevention of race conditions on inserts - id_index should be primary, and everything else synced,
    /// but need to work through how to synchronize? We should be able to assume that the primary key will always be
    /// correct - impose that condition on the caller, and should also be the first check that happens, so if the caller
    /// ensures this we don't need anything, if we want to be defensive, just need to manage time-of-check, time-of-use
    /// on the id_index insert. Will also need to be able to rollback if a later insert fails.
    pub fn insert(
        &self,
        id: NodeId,
        geometry: Geometry<f64>,
        metadata: Option<JsonValue>,
    ) -> Result<()> {
        let record = Arc::new(GeoRecordInner {
            id,
            geometry,
            metadata,
            is_deleted: AtomicBool::new(false),
        });

        // Do this after the record to avoid a double conditional
        // Best to do before the other inserts as this can easily fail
        // No need to rollback here on failure as this is the first insert
        if self.has_custom_key() {
            match &record.metadata {
                Some(value) => {
                    let custom_key = self.extract_custom_key(value)?;
                    match self.custom_key.entry(custom_key) {
                        dashmap::Entry::Occupied(_) => {
                            bail!("Duplicate key {:x} for custom key", custom_key);
                        }
                        dashmap::Entry::Vacant(vacant) => vacant.insert(Arc::clone(&record)),
                    }
                }
                None => bail!("Custom key set, but no meta available"),
            };
        }

        if let Err(err) = self.spatial_index.insert(Arc::clone(&record)) {
            self.delete(&id);
            bail!(err);
        }

        match self.id_index.entry(id) {
            dashmap::Entry::Occupied(_) => {
                self.delete(&id);
                bail!("Duplicate key {}", id);
            }
            dashmap::Entry::Vacant(vacant) => vacant.insert(record),
        };

        Ok(())
    }

    /// Bulk insert multiple records.
    /// TODO: Work on parallelizing and SIMD here
    /// TODO: Consider adding failure reasons and/or ids instead of just a count
    pub fn bulk_insert<I>(&self, records: I) -> (usize, usize)
    where
        I: IntoIterator<Item = (NodeId, Geometry<f64>, Option<JsonValue>)>,
    {
        let mut insert_count: usize = 0;
        let mut error_count: usize = 0;

        for (id, geometry, metadata) in records {
            match self.insert(id, geometry, metadata) {
                Ok(_) => insert_count += 1,
                Err(_) => error_count += 1,
            }
        }

        (insert_count, error_count)
    }

    /// Soft-delete a record by ID.
    ///
    /// Returns true if something was freshly deleted, false otherwise.
    pub fn delete(&self, id: &NodeId) -> bool {
        if let Some((_, record)) = self.id_index.remove(id) {
            let is_deleted = record.is_deleted.fetch_or(true, Ordering::Release);
            self.spatial_index.remove(id);

            // TODO: Is there a cleaner way of doing this, also that doesn't use an unwrap? This is a reason why we
            // might want the key on the Item, but also this will happen rarely; note that the unwrap should be fine as
            // it needed to get in there, but could also use an if let or an and_then
            if self.has_custom_key() {
                if let Ok(custom_key) = self.extract_custom_key(&record.metadata.as_ref().unwrap())
                {
                    self.custom_key.remove(&custom_key);
                }
            }
            !is_deleted
        } else {
            false
        }
    }

    /// Soft-delete a record with a custom key.
    ///
    /// Returns true if something is freshly deleted, false otherwise.
    pub fn delete_with_custom_key(&self, custom_key: &CustomKey) -> bool {
        if let Some((_, record)) = self.custom_key.remove(&custom_key) {
            let is_deleted = record.is_deleted.fetch_or(true, Ordering::Release);
            self.id_index.remove(&record.id);
            self.spatial_index.remove(&record.id);

            !is_deleted
        } else {
            false
        }
    }

    /// Retrieve a record by ID.
    ///
    /// This does not check the deletion status, which introduces a small race condition, but is still eventually
    /// consistent.
    pub fn get(&self, id: &NodeId) -> Option<GeoRecord> {
        self.id_index.get(&id).map(|r| Arc::clone(&r))
    }

    /// Retrieve a record using a custom key.
    ///
    /// Will return [`None`] if the key doesn't exist or there is no custom key on the store. Similar to get, this
    /// does not check deletion status.
    pub fn get_with_custom_key(&self, custom_key: &CustomKey) -> Option<GeoRecord> {
        if self.has_custom_key() {
            self.custom_key.get(custom_key).map(|r| Arc::clone(&r))
        } else {
            None
        }
    }

    fn has_custom_key(&self) -> bool {
        self.custom_key_pointer.is_some()
    }

    /// Using the stored JSON Pointer, extract the custom primary key for the passed metadata.
    /// This supports both string and i64 keys
    /// TODO: Expose this so that incoming items can determine the custom key before getting/deleting?
    fn extract_custom_key(&self, metadata: &JsonValue) -> Result<CustomKey> {
        let ptr = self
            .custom_key_pointer
            .as_ref()
            .ok_or(anyhow!("No custom key pointer"))?
            .as_str();

        metadata
            .pointer(ptr)
            .ok_or(anyhow!("Unable to locate field at {}", ptr))?
            .try_into()
    }
}

impl Default for GeoStore {
    fn default() -> Self {
        Self::new(Bbox::default().into())
    }
}

impl Knn<GeoRecord> for GeoStore {
    fn knn_r(&self, cmp: &Geometry, k: usize, r: f64) -> impl Iterator<Item = (GeoRecord, f64)> {
        self.spatial_index.knn_r(cmp, k, r)
    }
}

impl BboxSearch<'_, GeoRecord> for GeoStore {
    fn get_bbox(&self, bbox: &Rect) -> impl Iterator<Item = &GeoRecord> {
        self.spatial_index
            .get_bbox(bbox)
            .filter(|r| !r.is_deleted.load(Ordering::Acquire))
    }
}
