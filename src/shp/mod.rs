use std::iter::once;

use quadtree::*;
use shapefile::{dbase::FieldValue, Shape};

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
