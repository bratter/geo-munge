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

    fn remove<K>(&self, id: &K) -> Option<T>
    where
        T::Target: PartialEq<K>;
}

/// Nearest-neighbor-based search functions for spatial indexes.
///
/// While this trait doesn't require the structure to also implement [`SpatialIndex`] it usually will to provide the
/// means to insert or delete geometries.
///
/// Implementors of the trait should ensure the following rules hold:
/// - Methods that take a `k` must return at most `k` values. If multiple geometries in the index are at exactly the
///   same index and returning all of these instances will exceed `k`, then the implementation can arbitrarily choose
///   which to  return.
/// - If an `r` value is provided, the method must return all geometries where the closest point is less than
/// - If the cmp geometry overlaps or touches the geometry it is being tested against, the returned distance must be 0.
/// - Retrieval should be permissive in that shapes that cannot be measured should just be skipped - filtering invalid
///   shapes should be done on insertion if required at all. This behavior is useful in cases where some other traits
///   have less stringent requirements on the contained geometries.
/// - Implementors do not need to be identity aware, and therefore will not filter out any results. This means that both
///   (a) users should take this into account, and filter accordingly on the output results, and (b) implementors should
///   pay attention to the knn algorithm to ensure efficiency in producing an arbitrary number of results.
/// - There is no need for the comparison geometry to be contained by the indexes' bounding box.
///
/// TODO: Use GeoFloat generic instead of f64 concrete type?
/// TODO: Add an iter method that just iterates through all hits
pub trait Knn<T> {
    /// Find the `k` nearest neighbors constrained within the provided radius.
    fn knn_r(&self, cmp: &Geometry, k: usize, r: f64) -> impl Iterator<Item = (T, f64)>;

    /// Find the `k` nearest neighbors.
    fn knn(&self, cmp: &Geometry, k: usize) -> impl Iterator<Item = (T, f64)> {
        self.knn_r(cmp, k, std::f64::INFINITY)
    }

    /// Find the single nearest neighbor within the provided radius.
    fn find_nearest_r(&self, cmp: &Geometry, r: f64) -> Option<(T, f64)> {
        self.knn_r(cmp, 1, r).next()
    }

    /// Find the single nearest neighbor.
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
