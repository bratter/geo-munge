pub mod datum;

use geo::{Point, Rect};
use quadtree::*;
use shapefile::{dbase::Record, Reader, Shape};

use crate::shp::convert_shapes;
use datum::*;

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

pub fn make_qt<T>(
    shp: &mut Reader<T>,
    opts: QtData,
) -> (
    Box<dyn Searchable<IndexedDatum<Geometry<f64>>>>,
    Vec<Record>,
)
where
    T: std::io::Read + std::io::Seek,
{
    let (bounds, depth, mc) = (opts.bounds, opts.depth, opts.max_children);

    // Store records in a vector so we only have to allocate for them once
    // Then create a closure to add to the vector as we iterate over the shapefile
    // This vector needs to be returned with the qt for later reference
    let mut records = Vec::new();
    let add_record = |res| -> Result<(Shape, usize), ()> {
        match res {
            Ok((s, r)) => {
                let i = records.len();
                records.push(r);

                Ok((s, i))
            }
            Err(_) => Err(()),
        }
    };

    let mut qt: Box<dyn Searchable<_>> = if opts.is_bounds {
        Box::new(BoundsQuadTree::new(bounds, depth, mc))
    } else {
        Box::new(PointQuadTree::new(bounds, depth, mc))
    };

    for shp in shp
        .iter_shapes_and_records()
        .map(add_record)
        .flat_map(convert_shapes)
    {
        if let Ok((shape, i)) = shp {
            if opts.is_bounds {
                if qt.insert(IndexedDatum(shape, i)).is_err() {
                    eprintln!("Cannot insert datum at index {i} into qt")
                }
            } else {
                if matches!(shape, Geometry::Point::<f64>(_)) {
                    if qt.insert(IndexedDatum(shape, i)).is_err() {
                        eprintln!("Cannot insert datum at index {i} into qt")
                    }
                } else {
                    eprintln!(
                        "Invalid shape at index {i}. Can only add Points unless the bounds option is provided"
                    )
                }
            }
        } else {
            eprintln!("Could not read shape.")
        }
    }

    (qt, records)
}
