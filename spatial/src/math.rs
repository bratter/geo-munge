//! Geometry helper functions.

use std::f64::consts::{FRAC_PI_2, PI};

use geo::{
    coord, BoundingRect, GeoNum, Geometry, GeometryCollection, Intersects, Line, LineString,
    MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect, Triangle,
};

/// Helper macro for making points
#[macro_export]
macro_rules! p {
    ($x:expr, $y:expr) => {
        geo::Point::new($x, $y)
    };
}

/// Helper macro to make a line
#[macro_export]
macro_rules! l {
    ($x1:expr, $y1:expr, $x2:expr, $y2:expr) => {
        Line::new(
            geo::coord! { x: $x1, y: $y1 },
            geo::coord! { x: $x2, y: $y2 },
        )
    };
}

/// Determine whether a [`Point`] in contained within or sits on the boundary of
/// a [`Rect`].
///
/// We cannot use Rect::contains for this purpose because the
/// [DE-9IM semantics](https://en.wikipedia.org/wiki/DE-9IM) that geo-rust uses
/// does not return true when the `Point` site on the boundary of the `Rect`.
/// However this i still valid for most QuadTree operations.
///
/// Note that even 0-sized `Rect` shapes on the boundary of a quadtree will be
/// contained by another `Rect`, so this is not required for bounds-bounds
/// calculations.
pub fn pt_in_rect<T: GeoNum>(rect: &Rect<T>, pt: &Point<T>) -> bool {
    let (x, y) = pt.x_y();

    let (x1, y1) = rect.min().x_y();
    let (x2, y2) = rect.max().x_y();

    x >= x1 && x <= x2 && y >= y1 && y <= y2
}

pub enum PtRectPostion {
    Contained,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

// Determine the cardinal direction in "quadtrants" of a point from a rectangle.
//
// If the point is within the bounds of the rectangle in any direction it will have the
pub fn pt_rect_postion<T: GeoNum>(rect: &Rect<T>, pt: &Point<T>) -> PtRectPostion {
    let (x, y) = pt.x_y();
    let north = y > rect.max().y;
    let east = x > rect.max().x;
    let south = y < rect.min().y;
    let west = x < rect.min().x;

    match (north, east, south, west) {
        (false, false, false, false) => PtRectPostion::Contained,
        (true, false, false, false) => PtRectPostion::North,
        (false, true, false, false) => PtRectPostion::East,
        (false, false, true, false) => PtRectPostion::South,
        (false, false, false, true) => PtRectPostion::West,
        (true, true, false, false) => PtRectPostion::NorthEast,
        (false, true, true, false) => PtRectPostion::SouthEast,
        (false, false, true, true) => PtRectPostion::SouthWest,
        (true, false, false, true) => PtRectPostion::NorthWest,
        _ => unreachable!(),
    }
}

/// Determine whether the first rectangle `r1` contains or has on its border,
/// in degenerate cases, `r2`.
///
/// Currently this mirrors the behavior of contains for rects in geo-rust, but
/// this appears to be erroneous behavior, so we will not rely on it here.
pub fn rect_in_rect<T: GeoNum>(r1: &Rect<T>, r2: &Rect<T>) -> bool {
    r1.min().x <= r2.min().x
        && r1.max().x >= r2.max().x
        && r1.min().y <= r2.min().y
        && r1.max().y >= r2.max().y
}

/// Determine whether two rectangles intersect (including touching at boundaries).
///
/// Returns true if the rectangles overlap, touch, or one contains the other.
pub fn rect_intersects_rect<T: GeoNum>(r1: &Rect<T>, r2: &Rect<T>) -> bool {
    r1.intersects(r2)
}

/// Determine whether a geometry is completely contained within a bounding box.
///
/// This checks if the geometry's bounding box is contained within the query bbox.
/// Since a geometry cannot extend beyond its bounding box, this is a complete implementation.
pub fn geometry_contained_by_rect<T: GeoNum>(geom: &Geometry<T>, bbox: &Rect<T>) -> bool {
    if let Some(geom_bbox) = geom.bounding_rect() {
        rect_in_rect(bbox, &geom_bbox)
    } else {
        false
    }
}

/// Determine whether a geometry intersects with a bounding box.
///
/// This includes geometries that touch, overlap with, or are contained by the bbox.
pub fn geometry_intersects_rect<T: GeoNum>(geom: &Geometry<T>, bbox: &Rect<T>) -> bool {
    geom.intersects(bbox)
}

pub fn earth_bbox<T: GeoNum>() -> Rect<T> {
    let pi = T::from(-PI).expect("Valid Pi conversion");
    let pi_2 = T::from(-FRAC_PI_2).expect("Valid Pi conversion");

    Rect::new(
        coord! { x: T::zero() - pi, y: T::zero() - pi_2 },
        coord! { x: pi, y: pi_2 },
    )
}

pub trait Bbox<T: GeoNum> {
    fn bbox(&self) -> Option<Rect<T>>;

    fn bbox_unchecked(&self) -> Rect<T> {
        self.bbox().expect("No bbox for geometry")
    }
}

impl<T: GeoNum> Bbox<T> for Geometry<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for Point<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        Some(self.bounding_rect())
    }

    fn bbox_unchecked(&self) -> Rect<T> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for MultiPoint<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for Line<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        Some(self.bounding_rect())
    }

    fn bbox_unchecked(&self) -> Rect<T> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for LineString<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for MultiLineString<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for Polygon<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for MultiPolygon<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for Rect<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        Some(self.bounding_rect())
    }

    fn bbox_unchecked(&self) -> Rect<T> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for Triangle<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        Some(self.bounding_rect())
    }

    fn bbox_unchecked(&self) -> Rect<T> {
        self.bounding_rect()
    }
}

impl<T: GeoNum> Bbox<T> for GeometryCollection<T> {
    fn bbox(&self) -> Option<Rect<T>> {
        self.bounding_rect()
    }
}
