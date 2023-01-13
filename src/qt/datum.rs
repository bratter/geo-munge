use geo::Point;

use quadtree::*;

// TODO: Can we eliminate the clone? Maybe have to use a box, or return a ref?
#[derive(Debug)]
pub struct IndexedDatum<M = ()> {
    pub geom: Geometry<f64>,
    pub index: usize,
    pub meta: Option<M>,
}

impl<M> IndexedDatum<M> {
    pub fn new(geom: Geometry<f64>, index: usize, meta: M) -> Self {
        Self {
            geom,
            index,
            meta: Some(meta),
        }
    }

    pub fn without_meta(geom: Geometry<f64>, index: usize) -> Self {
        Self {
            geom,
            index,
            meta: None,
        }
    }
}

impl<M> Datum<f64> for IndexedDatum<M> {
    fn geometry(&self) -> Geometry<f64> {
        self.geom.clone()
    }
}

// Hacky solution to get make_qt working for both point and bounds variants
// Point shoud fail for non-point entities before it hits here, hence use of
// unreachable.
impl<M> PointDatum<f64> for IndexedDatum<M> {
    fn point(&self) -> Point {
        match self.geom {
            Geometry::Point(p) => p.to_owned(),
            _ => unreachable!(),
        }
    }
}
