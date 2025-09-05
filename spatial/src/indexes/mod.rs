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
pub trait SpatialIndex<T>
where
    T: Deref,
    T::Target: AsRef<Geometry>,
{
    fn insert(&self, record: T) -> Result<()>;

    fn remove<K>(&self, id: &K) -> Option<T>
    where
        T::Target: PartialEq<K>;

    fn contains<K>(&self, id: &K) -> bool
    where
        T::Target: PartialEq<K>;
}

/// A convenience trait representing an item that can be assigned a unique id.
///
/// This is required by the spatial indexes primarily for debugging and instrumentation purposes as it gives the index
/// some way of reporting to the user what item it was processing without having a concrete type. We force it to provide
/// a u32 rather than being a generic for ease of use, so when an identifier is not that space it would have to be cast
/// somehow.
///
/// If the caller doesn't want any form of identification but the trait bounds require it, suggest implementing just be
/// returning 0 in all cases.
///
/// TODO: Consider making this a generic, and/or wrapping in an Option with a default impl of None
pub trait Identified {
    fn uid(&self) -> u32;
}

/// Proximity-based search functions for spatial indexes.
///
/// While this trait doesn't require the structure to also implement [`SpatialIndex`] it usually will to provide the
/// means to insert or delete geometries.
///
/// Implementors of the trait should ensure the following rules hold:
/// - If the cmp geometry overlaps or touches the geometry it is being tested against, the returned distance must be 0.
/// - Retrieval should be permissive in that shapes that cannot be measured should just be skipped - filtering invalid
///   shapes should be done on insertion if required at all. This behavior is useful in cases where some other traits
///   have less stringent requirements on the contained geometries.
/// - Implementors do not need to be identity aware, and therefore will not filter out any results. This means that both
///   (a) users should take this into account, and filter accordingly on the output results, and (b) implementors should
///   pay attention to the search algorithm to ensure efficiency in producing an arbitrary number of results.
/// - There is no need for the comparison geometry to be contained by the indexes' bounding box.
/// - Results are returned in distance order (closest first).
///
/// TODO: Use GeoFloat generic instead of f64 concrete type?
pub trait ProximitySearch<T> {
    /// Find all geometries within the provided radius, returning results in distance order.
    /// This is the core method that all other methods build upon.
    fn within_radius(&self, cmp: &Geometry, radius: f64) -> impl Iterator<Item = (T, f64)>;

    /// Find all neighbors without radius limit, returning results in distance order.
    fn neighbors(&self, cmp: &Geometry) -> impl Iterator<Item = (T, f64)> {
        self.within_radius(cmp, std::f64::INFINITY)
    }

    /// Find the `k` nearest neighbors.
    fn nearest(&self, cmp: &Geometry, k: usize) -> impl Iterator<Item = (T, f64)> {
        self.neighbors(cmp).take(k)
    }

    /// Find the single closest geometry.
    fn closest(&self, cmp: &Geometry) -> Option<(T, f64)> {
        self.neighbors(cmp).next()
    }

    /// Find the single closest geometry within the provided radius.
    fn closest_within(&self, cmp: &Geometry, radius: f64) -> Option<(T, f64)> {
        self.within_radius(cmp, radius).next()
    }
}

/// Region-based query functions for spatial indexes.
///
/// While this trait doesn't require the structure to also implement [`SpatialIndex`] it usually will.
pub trait RegionQuery<T> {
    /// Find all geometries that are completely contained within the provided bounding box.
    fn contained_by(&self, bbox: &Rect) -> impl Iterator<Item = T>;

    /// Find all geometries that intersect with the provided bounding box.
    /// This includes geometries that touch, overlap with, or are contained by the bbox.
    fn intersecting(&self, bbox: &Rect) -> impl Iterator<Item = T>;
}
