use std::iter::once;

use quadtree::*;
use shapefile::Shape;

/// Convert shapefile shapes to their geo-type equivalents. This will only
/// convert those types that are valid in quadtrees.
pub fn convert_shapes(
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
