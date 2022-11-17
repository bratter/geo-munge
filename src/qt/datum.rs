use geo::Point;

use quadtree::*;

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
    fn point(&self) -> Point {
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
    fn point(&self) -> Point {
        self.0
    }
}
