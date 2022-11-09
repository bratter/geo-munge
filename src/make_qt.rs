use std::iter::once;

use geo::Point;
use quadtree::*;
use shapefile::{Reader, Shape};

use crate::QtData;

pub trait Searchable<D>
where
    D: Datum<f64>,
{
    fn find(&self, cmp: &Point) -> Result<(&D, f64), Error>;

    fn knn(&self, cmp: &Point, k: usize) -> Result<Vec<(&D, f64)>, Error>;
}

impl Searchable<IndexedDatum<Point>> for PointQuadTree<IndexedDatum<Point>, f64> {
    fn find(&self, cmp: &Point) -> Result<(&IndexedDatum<Point>, f64), Error> {
        QuadTreeSearch::find(self, &sphere(*cmp))
    }

    fn knn(&self, cmp: &Point, k: usize) -> Result<Vec<(&IndexedDatum<Point>, f64)>, Error> {
        QuadTreeSearch::knn(self, &sphere(*cmp), k)
    }
}

impl Searchable<Point> for BoundsQuadTree<Point, f64> {
    fn find(&self, cmp: &Point) -> Result<(&Point, f64), Error> {
        QuadTreeSearch::find(self, &sphere(*cmp))
    }

    fn knn(&self, cmp: &Point, k: usize) -> Result<Vec<(&Point, f64)>, Error> {
        QuadTreeSearch::knn(self, &sphere(*cmp), k)
    }
}

pub fn make_qt<T>(shp: &mut Reader<T>, opts: QtData) -> Box<dyn Searchable<IndexedDatum<Point>>>
where
    T: std::io::Read + std::io::Seek,
{
    let (bounds, depth, mc) = (opts.bounds, opts.depth, opts.max_children);

    if opts.is_bounds {
        // TODO: Do this and work through errors
        // let qt = Box::new(BoundsQuadTree::new(bounds, depth, mc));

        // qt
        todo!()
    } else {
        let mut qt = Box::new(PointQuadTree::new(bounds, depth, mc));

        // Store records in a vector so we only have to allocate for them once
        // Then create a closure to add to the vector as we iterate over the shapefile
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

        for shp in shp
            .iter_shapes_and_records()
            .map(add_record)
            .flat_map(convert_shapes)
        {
            if let Ok((shape, i)) = shp {
                if let Geometry::Point(p) = shape {
                    if qt.insert(IndexedDatum(p, i)).is_err() {
                        eprintln!("Cannot insert into qt.")
                    }
                } else {
                    eprintln!("Invalid shape.")
                }
            } else {
                eprintln!("Could not read shape.")
            }
        }

        qt
    }
}

// TODO: Can we eliminate the clone?
#[derive(Debug)]
pub struct IndexedDatum<G>(G, usize);

impl Datum<f64> for IndexedDatum<Geometry<f64>> {
    fn geometry(&self) -> Geometry<f64> {
        self.0.clone()
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
