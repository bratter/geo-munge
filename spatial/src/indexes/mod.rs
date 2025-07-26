pub mod basic_quadtree;

use std::ops::Deref;

use anyhow::Result;
use geo::{Geometry, Rect};

/// Common functions for a concurrent spatial index.
///
/// All functions require only &self and therefore must manage concurrency internally.
///
/// NOTE: The Deref bound ensures that the SpatialIndex can work with any smart pointer wrapped inner type without
/// requiring another newtype on top of it, helping the ergonomics of callers.
/// The PartialEq requires that the datum type can be directly compared with the corresponding key type.
/// An alternative scheme using a SpatialDatum trait would also work, but requires more effort on behalf of the caller.
/// TODO:A more general approach to any matching might just be able to put a constraint directly on the remove method of
/// PartialEq with the Datum type - this means that a datum or an id would work if partialeq is set up correctly by the
/// caller
pub trait SpatialIndex<T>
where
    T: Deref,
    T::Target: AsRef<Geometry>,
{
    fn insert(&self, record: T) -> Result<()>;

    // TODO: How to handle ids and removal
    // Start with a naive impl just iterating over all the children
    fn remove<K>(&self, id: &K) -> Option<T>
    where
        T::Target: PartialEq<K>;
}

// TODO: GeoNum or similar for the numeric type?
// TODO: What about the generic? Likely fine as-is
// TODO: How to handle errors (if any) in the output? e.g., run into shapes that it can't process
// TODO: Filter out same key responses

/// Nearest-neighbor-based search functions for spatial indexes.
///
/// While this doesn't require the structure to also implement [`SpatialIndex`] it usually will.
pub trait Knn<T> {
    fn knn_r(&self, cmp: &Geometry, k: usize, r: f64) -> impl Iterator<Item = (T, f64)>;

    fn knn(&self, cmp: &Geometry, k: usize) -> impl Iterator<Item = (T, f64)> {
        self.knn_r(cmp, k, std::f64::INFINITY)
    }

    fn find_nearest_r(&self, cmp: &Geometry, r: f64) -> Option<(T, f64)> {
        self.knn_r(cmp, 1, r).next()
    }

    fn find_nearest(&self, cmp: &Geometry) -> Option<(T, f64)> {
        self.knn(cmp, 1).next()
    }
}

/// Bounding-box-based search functions for spatial indexes.
///
/// While this doesn't require the structure to also implement [`SpatialIndex`] it usually will.
pub trait BboxSearch<'a, T: 'a> {
    /// Query spatial index by bounding box. Returns an iterator.
    fn get_bbox(&'a self, bbox: &Rect) -> impl Iterator<Item = &'a T>;
}
