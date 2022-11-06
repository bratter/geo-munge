use std::fmt::Display;
use std::fs::File;
use std::io::{BufRead, BufReader};

use geo::Point;
use quadtree::*;
use shapefile::ShapeReader;

use crate::QtData;

pub fn make_point_qt<D>(
    shp: &mut ShapeReader<BufReader<File>>,
    opts: QtData,
) -> impl QuadTreeSearch<D, f64> + QuadTree<D, f64> + Display
where
    D: Datum<f64> + PointDatum<f64>,
{
    if opts.is_bounds == true {
        // Bounds qt
        let qt = BoundsQuadTree::new(opts.bounds, opts.depth, opts.max_children);

        for shape in shp.iter_shapes() {
            // In a bounds qt, we can try adding a variety of shapes
            match shape {
                _ => todo!(),
            }
        }
        return qt;
    } else {
        // Point qt
        let qt: PointQuadTree<D, f64> =
            PointQuadTree::new(opts.bounds, opts.depth, opts.max_children);
        // return qt;
        todo!();
    }
}

pub trait Test<D>
where
    D: Datum<f64>,
{
    fn f2(&self, cmp: Point) -> Result<(&D, f64), Error>;
}
impl Test<Point> for PointQuadTree<Point, f64> {
    fn f2(&self, cmp: Point) -> Result<(&Point, f64), Error> {
        self.find(&sphere(cmp))
    }
}
impl Test<Point> for BoundsQuadTree<Point, f64> {
    fn f2(&self, cmp: Point) -> Result<(&Point, f64), Error> {
        self.find(&sphere(cmp))
    }
}
pub fn make_qt(opts: QtData) -> Box<dyn Test<Point>> {
    if opts.is_bounds {
        Box::new(BoundsQuadTree::new(opts.bounds, opts.depth, opts.max_children))
    } else {
        Box::new(PointQuadTree::new(opts.bounds, opts.depth, opts.max_children))
    }
}
// pub fn make_bounds_qt<D>(
//     shp: &mut ShapeReader<BufReader<File>>,
//     opts: QtData,
// ) -> dyn QuadTreeSearch<D, f64>
// where
//     D: Datum<f64> + PointDatum<f64>,
// {
//     // Point qt
//     let qt: PointQuadTree<D, f64> = PointQuadTree::new(opts.bounds, opts.depth, opts.max_children);
//     return qt;
// }
