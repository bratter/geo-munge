use std::{fs::read_to_string, iter::once, path::PathBuf};

use geojson::GeoJson;
use quadtree::{Geometry, ToRadians};

use crate::error::Error;

pub fn read_geojson(path: &PathBuf) -> Result<GeoJson, Error> {
    read_to_string(&path)
        .map_err(|_| Error::CannotReadFile(path.clone()))?
        .parse::<GeoJson>()
        .map_err(|_| Error::CannotParseFile(path.clone()))
}

/// Convert a GeoJson geometry into the appropriate quadtree-enabled type. Outputs an iterator as
/// it flattens multi-geometries into their single geometry counterparts.
pub fn convert_geom(
    input: &geojson::Geometry,
) -> Box<dyn Iterator<Item = Result<Geometry<f64>, geojson::Error>>> {
    match &input.value {
        d @ geojson::Value::Point(_) => Box::new(once(d.try_into().map(|mut p: geo::Point| {
            p.to_radians_in_place();
            Geometry::Point(p)
        }))),
        d @ geojson::Value::Polygon(_) => {
            Box::new(once(d.try_into().map(|mut p: geo::Polygon| {
                p.to_radians_in_place();
                Geometry::Polygon(p)
            })))
        }
        d @ geojson::Value::LineString(_) => {
            Box::new(once(d.try_into().map(|mut l: geo::LineString| {
                l.to_radians_in_place();
                Geometry::LineString(l)
            })))
        }
        d @ geojson::Value::MultiPoint(_) => match geo::MultiPoint::try_from(d) {
            Ok(mp) => Box::new(mp.into_iter().map(|mut p| {
                p.to_radians_in_place();
                Ok(Geometry::Point(p))
            })),
            Err(err) => Box::new(once(Err(err))),
        },
        d @ geojson::Value::MultiPolygon(_) => match geo::MultiPolygon::try_from(d) {
            Ok(mp) => Box::new(mp.into_iter().map(|mut p| {
                p.to_radians_in_place();
                Ok(Geometry::Polygon(p))
            })),
            Err(err) => Box::new(once(Err(err))),
        },
        d @ geojson::Value::MultiLineString(_) => match geo::MultiLineString::try_from(d) {
            Ok(mls) => Box::new(mls.into_iter().map(|mut l| {
                l.to_radians_in_place();
                Ok(Geometry::LineString(l))
            })),
            Err(err) => Box::new(once(Err(err))),
        },
        geojson::Value::GeometryCollection(_) => {
            Box::new(once(Err(geojson::Error::ExpectedType {
                expected: "not GeometryCollection".to_string(),
                actual: "GeometryCollection".to_string(),
            })))
        }
    }
}
