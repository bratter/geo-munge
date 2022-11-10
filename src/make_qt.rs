use std::iter::once;

use geo::Point;
use quadtree::*;
use shapefile::{dbase::Record, Reader, Shape};

use crate::QtData;

pub trait Searchable<D>
where
    D: Datum<f64>,
{
    fn find(&self, cmp: &Point, r: Option<f64>) -> Result<(&D, f64), Error>;

    fn knn(&self, cmp: &Point, k: usize, r: Option<f64>) -> Result<Vec<(&D, f64)>, Error>;
}

impl Searchable<IndexedDatum<Geometry<f64>>> for PointQuadTree<IndexedDatum<Geometry<f64>>, f64> {
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

    let mut qt = Box::new(BoundsQuadTree::new(bounds, depth, mc));

    for shp in shp
        .iter_shapes_and_records()
        .map(add_record)
        .flat_map(convert_shapes)
    {
        if let Ok((shape, i)) = shp {
            if opts.is_bounds {
                if qt.insert(IndexedDatum(shape, i)).is_err() {
                    eprintln!("Cannot insert into qt.")
                }
            } else {
                if matches!(Geometry::Point::<f64>, _) {
                    if qt.insert(IndexedDatum(shape, i)).is_err() {
                        eprintln!("Cannot insert into qt.")
                    }
                } else {
                    eprintln!(
                        "Invalid shape. Can only add Points unless the bounds option is provided."
                    )
                }
            }
        } else {
            eprintln!("Could not read shape.")
        }
    }

    (qt, records)
}

// TODO: Can we eliminate the clone? Maybe have to use a box, or return a ref?
#[derive(Debug)]
pub struct IndexedDatum<G>(pub G, pub usize);

impl Datum<f64> for IndexedDatum<Geometry<f64>> {
    fn geometry(&self) -> Geometry<f64> {
        self.0.clone()
    }
}

// Hacky solution to get make_qt working for both point and bounds variants
// Point shoud fail for non-point entities before it hits here, hence use of
// unreachable.
impl PointDatum<f64> for IndexedDatum<Geometry<f64>> {
    fn point(&self) -> Point<f64> {
        match self.0 {
            Geometry::Point(p) => p.to_owned(),
            _ => unreachable!(),
        }
    }
}

impl Datum<f64> for IndexedDatum<Point> {
    fn geometry(&self) -> Geometry<f64> {
        Geometry::Point(self.0)
    }
}

impl PointDatum<f64> for IndexedDatum<Point> {
    fn point(&self) -> Point<f64> {
        self.0
    }
}

fn point_to_iter<S>(
    shape: S,
    i: usize,
) -> Box<dyn Iterator<Item = Result<(Geometry<f64>, usize), ()>>>
where
    S: Into<geo::Point>,
{
    let mut p: geo::Point = shape.into();
    p.to_radians_in_place();
    Box::new(once(Ok((Geometry::Point(p), i))))
}

fn mls_to_iter<S>(
    shape: S,
    i: usize,
) -> Box<dyn Iterator<Item = Result<(Geometry<f64>, usize), ()>>>
where
    S: Into<geo::MultiLineString>,
{
    let mls: geo::MultiLineString = shape.into();
    Box::new(mls.into_iter().map(move |mut item| {
        item.to_radians_in_place();
        Ok((Geometry::LineString(item), i))
    }))
}

fn mp_to_iter<S>(shape: S, i: usize) -> Box<dyn Iterator<Item = Result<(Geometry<f64>, usize), ()>>>
where
    S: Into<geo::MultiPoint>,
{
    let mp: geo::MultiPoint = shape.into();
    Box::new(mp.into_iter().map(move |mut item| {
        item.to_radians_in_place();
        Ok((Geometry::Point(item), i))
    }))
}

fn mpoly_to_iter<S>(
    shape: S,
    i: usize,
) -> Box<dyn Iterator<Item = Result<(Geometry<f64>, usize), ()>>>
where
    S: Into<geo::MultiPolygon>,
{
    let mp: geo::MultiPolygon = shape.into();
    Box::new(mp.into_iter().map(move |mut item| {
        item.to_radians_in_place();
        Ok((Geometry::Polygon(item), i))
    }))
}

/// Convert shapefile shapes to their geo-type equivalents. This will only
/// convert those types that are valid in quadtrees.
fn convert_shapes(
    shape_index: Result<(Shape, usize), ()>,
) -> Box<dyn Iterator<Item = Result<(Geometry<f64>, usize), ()>>> {
    match shape_index {
        Ok((shape, i)) => {
            match shape {
                Shape::Point(p) => point_to_iter(p, i),
                Shape::PointM(p) => point_to_iter(p, i),
                Shape::PointZ(p) => point_to_iter(p, i),
                Shape::Polyline(p) => mls_to_iter(p, i),
                Shape::PolylineM(p) => mls_to_iter(p, i),
                Shape::PolylineZ(p) => mls_to_iter(p, i),
                Shape::Multipoint(p) => mp_to_iter(p, i),
                Shape::MultipointM(p) => mp_to_iter(p, i),
                Shape::MultipointZ(p) => mp_to_iter(p, i),
                Shape::Polygon(p) => mpoly_to_iter(p, i),
                Shape::PolygonM(p) => mpoly_to_iter(p, i),
                Shape::PolygonZ(p) => mpoly_to_iter(p, i),
                // NullShape and MultiPatch are not covered
                _ => Box::new(once(Err(()))),
            }
        }
        Err(_) => Box::new(once(Err(()))),
    }
}
