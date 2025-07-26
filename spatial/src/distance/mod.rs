mod haversine;
mod newton;

use geo::{GeoFloat, Geometry};

use haversine::*;

// TODO: Have to handle radian conversion appropriately, maybe as a numeric type?
// TODO: Need to work out what we want for the result type - all result, or assoc type - we can't panic, but might not
// be able to do all distances

pub trait Distance<G, T: GeoFloat> {
    fn distance(&self, other: &G) -> T;
}

impl<T: GeoFloat> Distance<geo::Point<T>, T> for geo::Point<T> {
    fn distance(&self, other: &geo::Point<T>) -> T {
        haversine(self, other)
    }
}

impl<T: GeoFloat> Distance<geo::Rect<T>, T> for geo::Rect<T> {
    fn distance(&self, other: &geo::Rect<T>) -> T {
        haversine_rect_rect(self, other)
    }
}

impl<T: GeoFloat> Distance<Geometry<T>, T> for Geometry<T> {
    fn distance(&self, other: &Geometry<T>) -> T {
        match self {
            Geometry::Point(a) => a.distance(other),
            Geometry::Line(a) => a.distance(other),
            Geometry::LineString(a) => a.distance(other),
            Geometry::Polygon(a) => a.distance(other),
            Geometry::Rect(a) => a.distance(other),
            _ => todo!(),
        }
    }
}

impl<T: GeoFloat> Distance<Geometry<T>, T> for geo::Point<T> {
    fn distance(&self, other: &Geometry<T>) -> T {
        match other {
            Geometry::Point(b) => haversine(self, b),
            Geometry::Line(b) => haversine_pt_line(self, b),
            Geometry::LineString(b) => haversine_pt_linestring(self, b),
            Geometry::Polygon(b) => haversine_pt_poly(self, b),
            Geometry::Rect(b) => haversine_pt_rect(self, b),
            _ => todo!(),
        }
    }
}

impl<T: GeoFloat> Distance<Geometry<T>, T> for geo::Line<T> {
    fn distance(&self, other: &Geometry<T>) -> T {
        match other {
            Geometry::Point(b) => haversine_pt_line(b, self),
            _ => todo!(),
        }
    }
}

impl<T: GeoFloat> Distance<Geometry<T>, T> for geo::LineString<T> {
    fn distance(&self, other: &Geometry<T>) -> T {
        match other {
            Geometry::Point(b) => haversine_pt_linestring(b, self),
            _ => todo!(),
        }
    }
}

impl<T: GeoFloat> Distance<Geometry<T>, T> for geo::Polygon<T> {
    fn distance(&self, other: &Geometry<T>) -> T {
        match other {
            Geometry::Point(b) => haversine_pt_poly(b, self),
            _ => todo!(),
        }
    }
}

impl<T: GeoFloat> Distance<Geometry<T>, T> for geo::Rect<T> {
    fn distance(&self, other: &Geometry<T>) -> T {
        match other {
            Geometry::Point(b) => haversine_pt_rect(b, self),
            Geometry::Rect(b) => haversine_rect_rect(self, b),
            _ => todo!(),
        }
    }
}
