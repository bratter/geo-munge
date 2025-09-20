//! Module for main concurrent access data structures.

use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use anyhow::{anyhow, bail, Result};
use dashmap::DashMap;
use fxhash::FxBuildHasher;
use geo::{Geometry, Rect};
use protocol::{request::KeyMode, CustomKey, Properties, Uid};
use spatial::{earth_bbox, BasicQuadTree, Identified, ProximitySearch, RegionQuery, SpatialIndex};

use super::{Feature, ParsedFeature};

#[derive(Debug)]
enum UidManager {
    AutoIncrement(AtomicU32),
    ProvidedNumeric,
}

impl UidManager {
    fn new(mode: &KeyMode) -> Self {
        match mode {
            KeyMode::AutoIncrement | KeyMode::CustomBytes(_) => {
                Self::AutoIncrement(AtomicU32::new(0))
            }
            KeyMode::ProvidedNumeric => Self::ProvidedNumeric,
        }
    }

    fn generate_id(&self, provided_key: Option<Uid>) -> Result<Uid> {
        match (self, provided_key) {
            (UidManager::AutoIncrement(counter), None) => {
                Ok(counter.fetch_add(1, Ordering::Relaxed))
            }
            (UidManager::AutoIncrement(_), Some(_)) => {
                bail!("Cannot provide key in AutoIncrement mode")
            }
            (UidManager::ProvidedNumeric, Some(key)) => Ok(key),
            (UidManager::ProvidedNumeric, None) => {
                bail!("Must provide key in ProvidedNumeric mode")
            }
        }
    }
}

/// Storage wrapper around Feature with server-specific metadata.
pub struct RecordInner {
    pub data: Feature,
    is_deleted: AtomicBool,
}

/// Shared record type used throughout the server.
pub type Record = Arc<RecordInner>;

impl From<&RecordInner> for Feature {
    fn from(record: &RecordInner) -> Self {
        record.data.clone()
    }
}

impl From<Feature> for RecordInner {
    fn from(data: Feature) -> Self {
        Self {
            data,
            is_deleted: AtomicBool::new(false),
        }
    }
}

impl Identified for RecordInner {
    fn uid(&self) -> u32 {
        self.data.id
    }
}

impl AsRef<Geometry<f64>> for RecordInner {
    fn as_ref(&self) -> &Geometry<f64> {
        &self.data.geometry
    }
}

impl PartialEq<Uid> for RecordInner {
    fn eq(&self, other: &Uid) -> bool {
        &self.data.id == other
    }
}

impl RecordInner {
    pub fn new(data: Feature) -> Self {
        Self {
            data,
            is_deleted: AtomicBool::new(false),
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.is_deleted.load(Ordering::Relaxed)
    }

    pub fn mark_deleted(&self) {
        self.is_deleted.store(true, Ordering::Relaxed);
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
    id_index: DashMap<Uid, Record, FxBuildHasher>,
    custom_key: DashMap<CustomKey, Record, FxBuildHasher>,
    spatial_index: BasicQuadTree<Record>,
    uid_manager: UidManager,
    custom_key_pointer: Option<String>,
}

impl GeoStore {
    /// Create a new [`GeoStore`].
    ///
    /// Will generate the appropriate key management strategy based on the provided key_mode. If the [`KeyMode`] is
    /// custom bytes, it is extracted from each record's metadata using JSON Pointer passed with this call. This means
    /// that all entries using a custom key must have metadata. See https://datatracker.ietf.org/doc/html/rfc6901 for
    /// details on JSON pointer syntax.
    pub fn new(bbox: Rect, key_mode: KeyMode) -> Self {
        GeoStore {
            id_index: DashMap::with_hasher(FxBuildHasher::new()),
            custom_key: DashMap::with_hasher(FxBuildHasher::new()),
            spatial_index: BasicQuadTree::new(bbox),
            uid_manager: UidManager::new(&key_mode),
            custom_key_pointer: if let KeyMode::CustomBytes(ptr) = key_mode {
                Some(ptr)
            } else {
                None
            },
        }
    }

    pub fn key_mode(&self) -> KeyMode {
        if let Some(ptr) = &self.custom_key_pointer {
            KeyMode::CustomBytes(ptr.clone())
        } else {
            match self.uid_manager {
                UidManager::AutoIncrement(_) => KeyMode::AutoIncrement,
                UidManager::ProvidedNumeric => KeyMode::ProvidedNumeric,
            }
        }
    }

    pub fn len(&self) -> usize {
        self.id_index.len()
    }

    pub fn bbox(&self) -> Rect {
        self.spatial_index.bbox()
    }

    /// Insert a new feature, generating the appropriate ID based on the store's key strategy.
    ///
    /// If using an additional custom key, metadata must be provided or the insert will fail.
    ///
    /// Will error if the key already exists or if there's a mismatch between the key strategy
    /// and the provided_key field.
    ///
    /// WARN: Revisit prevention of race conditions on inserts - id_index should be primary, and everything else synced,
    /// but need to work through how to synchronize? We should be able to assume that the primary key will always be
    /// correct - impose that condition on the caller, and should also be the first check that happens, so if the caller
    /// ensures this we don't need anything, if we want to be defensive, just need to manage time-of-check, time-of-use
    /// on the id_index insert. Will also need to be able to rollback if a later insert fails.
    pub fn insert(&self, parsed_feature: ParsedFeature) -> Result<()> {
        // Generate the primary key based on the strategy
        let id = self.uid_manager.generate_id(parsed_feature.provided_key)?;

        // Create the feature with the assigned ID
        let feature = Feature {
            id,
            geometry: parsed_feature.geometry,
            properties: parsed_feature.properties,
        };
        let record = Arc::new(RecordInner::new(feature));

        // Do this after the record to avoid a double conditional
        // Best to do before the other inserts as this can easily fail
        // No need to rollback here on failure as this is the first insert
        if self.has_custom_key() {
            match &record.data.properties {
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
            self.delete(&record.data.id);
            bail!(err);
        }

        match self.id_index.entry(record.data.id) {
            dashmap::Entry::Occupied(_) => {
                self.delete(&record.data.id);
                bail!("Duplicate key {}", record.data.id);
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
        I: IntoIterator<Item = ParsedFeature>,
    {
        let mut insert_count: usize = 0;
        let mut error_count: usize = 0;

        for feature in records {
            match self.insert(feature) {
                Ok(_) => insert_count += 1,
                Err(_) => error_count += 1,
            }
        }

        (insert_count, error_count)
    }

    /// Soft-delete a record by ID.
    ///
    /// Returns true if something was freshly deleted, false otherwise.
    pub fn delete(&self, id: &Uid) -> bool {
        if let Some((_, record)) = self.id_index.remove(id) {
            let is_deleted = record.is_deleted.fetch_or(true, Ordering::Release);
            self.spatial_index.remove(id);

            // TODO: Is there a cleaner way of doing this, also that doesn't use an unwrap? This is a reason why we
            // might want the key on the Item, but also this will happen rarely; note that the unwrap should be fine as
            // it needed to get in there, but could also use an if let or an and_then
            if self.has_custom_key() {
                let props = record.data.properties.as_ref().unwrap();
                if let Ok(custom_key) = self.extract_custom_key(props) {
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
            self.id_index.remove(&record.data.id);
            self.spatial_index.remove(&record.data.id);

            !is_deleted
        } else {
            false
        }
    }

    /// Retrieve a record by ID.
    ///
    /// This does not check the deletion status, which introduces a small race condition, but is still eventually
    /// consistent.
    pub fn get(&self, id: &Uid) -> Option<Record> {
        self.id_index.get(&id).map(|r| Arc::clone(&r))
    }

    /// Retrieve a record using a custom key.
    ///
    /// Will return [`None`] if the key doesn't exist or there is no custom key on the store. Similar to get, this
    /// does not check deletion status.
    pub fn get_with_custom_key(&self, custom_key: &CustomKey) -> Option<Record> {
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
    fn extract_custom_key(&self, metadata: &Properties) -> Result<CustomKey> {
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
        Self::new(earth_bbox(), KeyMode::default())
    }
}

impl ProximitySearch<Record> for GeoStore {
    fn within_radius(&self, cmp: &Geometry<f64>, r: f64) -> impl Iterator<Item = (Record, f64)> {
        self.spatial_index
            .within_radius(cmp, r)
            .filter(|r| !r.0.is_deleted.load(Ordering::Acquire))
    }
}

impl RegionQuery<Record> for GeoStore {
    fn contained_by(&self, bbox: &Rect) -> impl Iterator<Item = Record> {
        self.spatial_index
            .contained_by(bbox)
            .filter(|r| !r.is_deleted.load(Ordering::Acquire))
    }

    fn intersecting(&self, bbox: &Rect) -> impl Iterator<Item = Record> {
        self.spatial_index
            .intersecting(bbox)
            .filter(|r| !r.is_deleted.load(Ordering::Acquire))
    }
}
