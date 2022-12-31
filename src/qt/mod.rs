pub mod datum;
mod shapefile;

use std::path::PathBuf;

use geo::{Point, Rect};
use quadtree::*;

use crate::error::FiberError;
use datum::*;

use self::shapefile::shp_build;

pub struct QtData {
    pub is_bounds: bool,
    pub bounds: Rect<f64>,
    pub depth: u8,
    pub max_children: usize,
}

impl QtData {
    pub fn new(
        is_bounds: bool,
        bounds: Rect,
        depth: Option<u8>,
        max_children: Option<usize>,
    ) -> Self {
        Self {
            is_bounds,
            bounds,
            depth: depth.unwrap_or(10),
            max_children: max_children.unwrap_or(10),
        }
    }
}

// All quadtrees should implement Display
pub trait Searchable<D>: std::fmt::Display
where
    D: Datum<f64>,
{
    fn size(&self) -> usize;

    fn insert(&mut self, datum: D) -> Result<(), Error>;

    fn find(&self, cmp: &Point, r: Option<f64>) -> Result<(&D, f64), Error>;

    fn knn(&self, cmp: &Point, k: usize, r: Option<f64>) -> Result<Vec<(&D, f64)>, Error>;
}

impl Searchable<IndexedDatum<Geometry<f64>>> for PointQuadTree<IndexedDatum<Geometry<f64>>, f64> {
    fn size(&self) -> usize {
        QuadTree::size(self)
    }

    fn insert(&mut self, datum: IndexedDatum<Geometry<f64>>) -> Result<(), Error> {
        <PointQuadTree<IndexedDatum<Geometry<f64>>, f64> as QuadTree<
            IndexedDatum<Geometry<f64>>,
            f64,
        >>::insert(self, datum)
    }

    fn find(
        &self,
        cmp: &Point,
        r: Option<f64>,
    ) -> Result<(&IndexedDatum<Geometry<f64>>, f64), Error> {
        QuadTreeSearch::find_r(self, &sphere(*cmp), r.unwrap_or(f64::INFINITY))
    }

    fn knn(
        &self,
        cmp: &Point,
        k: usize,
        r: Option<f64>,
    ) -> Result<Vec<(&IndexedDatum<Geometry<f64>>, f64)>, Error> {
        QuadTreeSearch::knn_r(self, &sphere(*cmp), k, r.unwrap_or(f64::INFINITY))
    }
}

impl Searchable<IndexedDatum<Geometry<f64>>> for BoundsQuadTree<IndexedDatum<Geometry<f64>>, f64> {
    fn size(&self) -> usize {
        QuadTree::size(self)
    }

    fn insert(&mut self, datum: IndexedDatum<Geometry<f64>>) -> Result<(), Error> {
        <BoundsQuadTree<IndexedDatum<Geometry<f64>>, f64> as QuadTree<
            IndexedDatum<Geometry<f64>>,
            f64,
        >>::insert(self, datum)
    }

    fn find(
        &self,
        cmp: &Point,
        r: Option<f64>,
    ) -> Result<(&IndexedDatum<Geometry<f64>>, f64), Error> {
        QuadTreeSearch::find_r(self, &sphere(*cmp), r.unwrap_or(f64::INFINITY))
    }

    fn knn(
        &self,
        cmp: &Point,
        k: usize,
        r: Option<f64>,
    ) -> Result<Vec<(&IndexedDatum<Geometry<f64>>, f64)>, Error> {
        QuadTreeSearch::knn_r(self, &sphere(*cmp), k, r.unwrap_or(f64::INFINITY))
    }
}

/// Result type including the original datum, extracted metdata, and the distance.
pub struct SearchResult<'a> {
    pub datum: &'a IndexedDatum<Geometry<f64>>,
    // TODO: This type will need to be fixed
    //       Can it be &'a dyn instead of box?
    pub meta: Box<dyn Iterator<Item = String> + 'a>,
    pub distance: f64,
}

/// Trait representing the allowable file types for searching and extracting metadata. Used as each
/// file type requires a different way of managing the meta.
///
///  In the meta key of each implementation, implementors should automatically insert the native
///  `id` key to ensure that the match id is always transfered to the results. The location of this
///  key is necessarily implementation specific.
pub trait SearchableWithMeta: std::fmt::Display {
    fn size(&self) -> usize;

    fn find<'a>(
        &'a self,
        cmp: &Point,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<SearchResult<'a>, Error>;

    fn knn<'a>(
        &'a self,
        cmp: &Point,
        k: usize,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<Vec<SearchResult<'a>>, Error>;
}

/// Create the correct quadtree wrapper based on the input options and provided filetype.
pub fn make_qt_from_path<'a>(
    path: PathBuf,
    opts: QtData,
) -> Result<Box<dyn SearchableWithMeta>, FiberError> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or(FiberError::IO("Cannot parse file extension"))?
    {
        "shp" => shp_build(path, opts),
        _ => Err(FiberError::IO("Unsupported file type")),
    }
}

pub fn make_dyn_qt(opts: &QtData) -> Box<dyn Searchable<IndexedDatum<Geometry<f64>>>> {
    let (bounds, depth, mc) = (opts.bounds, opts.depth, opts.max_children);

    if opts.is_bounds {
        Box::new(BoundsQuadTree::new(bounds, depth, mc))
    } else {
        Box::new(PointQuadTree::new(bounds, depth, mc))
    }
}
