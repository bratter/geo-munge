use std::iter::once;

use quadtree::*;
use shapefile::{dbase::FieldValue, Shape};

use crate::error::{Error, UnsupportedGeoType};

/// Convert dbase fields to a string representation for inclusion in csv output.
pub fn convert_dbase_field(f: &FieldValue) -> String {
    match f {
        FieldValue::Character(s) => s.to_owned().unwrap_or(String::default()),
        FieldValue::Memo(s) => s.to_owned(),
        FieldValue::Integer(n) => format!("{}", n),
        FieldValue::Numeric(n) => format!("{}", n.unwrap_or(f64::NAN)),
        FieldValue::Double(n) => format!("{}", n),
        FieldValue::Float(n) => format!("{}", n.unwrap_or(f32::NAN)),
        FieldValue::Currency(n) => format!("{}", n),
        FieldValue::Logical(b) => match b {
            Some(true) => "true".to_owned(),
            Some(false) => "false".to_owned(),
            None => String::default(),
        },
        FieldValue::Date(d) => d.map(|d| d.to_string()).unwrap_or(String::default()),
        FieldValue::DateTime(d) => {
            let date = d.date();
            let time = d.time();
            format!(
                "{:4}-{:2}-{:2} {:2}:{:2}:{:2}",
                date.year(),
                date.month(),
                date.day(),
                time.hours(),
                time.minutes(),
                time.seconds()
            )
        }
    }
}

pub fn convert_dbase_field_opt(f: Option<&FieldValue>) -> String {
    match f {
        Some(f) => convert_dbase_field(f),
        None => String::default(),
    }
}

/// Convert shapefile shapes to their geo-type equivalents. This will only
/// convert those types that are valid in quadtrees.
pub fn convert_shape(shape: Shape) -> Box<dyn Iterator<Item = Result<Geometry<f64>, Error>>> {
    match shape {
        Shape::Point(p) => point_to_iter(p),
        Shape::PointM(p) => point_to_iter(p),
        Shape::PointZ(p) => point_to_iter(p),
        Shape::Polyline(p) => mls_to_iter(p),
        Shape::PolylineM(p) => mls_to_iter(p),
        Shape::PolylineZ(p) => mls_to_iter(p),
        Shape::Multipoint(p) => mp_to_iter(p),
        Shape::MultipointM(p) => mp_to_iter(p),
        Shape::MultipointZ(p) => mp_to_iter(p),
        Shape::Polygon(p) => mpoly_to_iter(p),
        Shape::PolygonM(p) => mpoly_to_iter(p),
        Shape::PolygonZ(p) => mpoly_to_iter(p),
        // NullShape and MultiPatch are not covered
        Shape::Multipatch(_) => Box::new(once(Err(Error::UnsupportedGeometry(
            UnsupportedGeoType::MultipatchShp,
        )))),
        Shape::NullShape => Box::new(once(Err(Error::UnsupportedGeometry(
            UnsupportedGeoType::NullShp,
        )))),
    }
}

fn point_to_iter<S>(shape: S) -> Box<dyn Iterator<Item = Result<Geometry<f64>, Error>>>
where
    S: Into<geo::Point>,
{
    let mut p: geo::Point = shape.into();
    p.to_radians_in_place();
    Box::new(once(Ok(Geometry::Point(p))))
}

fn mls_to_iter<S>(shape: S) -> Box<dyn Iterator<Item = Result<Geometry<f64>, Error>>>
where
    S: Into<geo::MultiLineString>,
{
    let mls: geo::MultiLineString = shape.into();
    Box::new(mls.into_iter().map(move |mut item| {
        item.to_radians_in_place();
        Ok(Geometry::LineString(item))
    }))
}

fn mp_to_iter<S>(shape: S) -> Box<dyn Iterator<Item = Result<Geometry<f64>, Error>>>
where
    S: Into<geo::MultiPoint>,
{
    let mp: geo::MultiPoint = shape.into();
    Box::new(mp.into_iter().map(move |mut item| {
        item.to_radians_in_place();
        Ok(Geometry::Point(item))
    }))
}

fn mpoly_to_iter<S>(shape: S) -> Box<dyn Iterator<Item = Result<Geometry<f64>, Error>>>
where
    S: Into<geo::MultiPolygon>,
{
    let mp: geo::MultiPolygon = shape.into();
    Box::new(mp.into_iter().map(move |mut item| {
        item.to_radians_in_place();
        Ok(Geometry::Polygon(item))
    }))
}
