use std::fs::read_to_string;
use std::iter::once;
use std::path::PathBuf;
use std::rc::Rc;

use geojson::feature::Id;
use geojson::{Feature, GeoJson};
use quadtree::*;
use serde_json::Value;

use crate::error::FiberError;

use super::datum::{VarDatum, VarMeta};
use super::QtData;
use super::VarQt;

// TODO: Move this to a library location
pub fn read_geojson(path: &PathBuf) -> Result<GeoJson, FiberError> {
    read_to_string(&path)
        .map_err(|_| FiberError::IO("Cannot read GeoJson file"))?
        .parse::<GeoJson>()
        .map_err(|_| FiberError::IO("Cannot parse GeoJson file"))
}

pub fn json_field_val(feature: &Feature, field: &String) -> String {
    // Special handling of id as it is a named property
    if field == "id" {
        match &feature.id {
            Some(Id::String(s)) => s.to_string(),
            Some(Id::Number(n)) => n.to_string(),
            None => String::default(),
        }
    } else if let Some(props) = &feature.properties {
        props.get(field).map(map_json_value).unwrap_or_default()
    } else {
        String::default()
    }
}

// TODO: Should we deal with arrays and objects differently?
// TODO: Move to lib
fn map_json_value(val: &Value) -> String {
    match val {
        Value::Null => String::default(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) => String::default(),
        Value::Object(_) => String::default(),
    }
}

pub fn build_geojson(path: PathBuf, opts: QtData) -> Result<VarQt, FiberError> {
    let geojson = read_geojson(&path)?;
    let mut qt = VarQt::new(opts);

    // Create an iterator that runs through and flattens all geometries in the GeoJson, preparing
    // them for adding to the qt
    let geometries: Box<dyn Iterator<Item = _>> = match geojson {
        GeoJson::Geometry(g) => {
            // Note that geometries don't contain any metadata, so using the None meta here
            Box::new(convert_geom(&g).map(|res| {
                (
                    0,
                    res.map(|geom| VarDatum::new(geom, VarMeta::None, 0))
                        .map_err(|_| FiberError::Arg("GeoJson error")),
                )
            }))
        }
        GeoJson::Feature(f) => {
            // For features, there is still only a single index and geometry is an option
            map_feature((0, f))
        }
        GeoJson::FeatureCollection(fc) => {
            // Feature collections we need to flatmap through the features vector and do the same
            // thing as an individual feature
            Box::new(fc.features.into_iter().enumerate().flat_map(map_feature))
        }
    };

    for (index, datum_res) in geometries {
        if let Ok(datum) = datum_res {
            if qt.insert(datum).is_err() {
                eprintln!("Cannot insert datum at index {index} into qt");
            }
        } else {
            eprintln!("Could not read shape at index {index}");
        }
    }

    Ok(qt)
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
                actual: "GeomtryCollection".to_string(),
            })))
        }
    }
}

/// Convenience function to build a feature iterator over a single geojson feature, using, for
/// convenience, output in the form of an `enumerate` on an `Iterator`.
fn map_feature(
    (i, f): (usize, Feature),
) -> Box<dyn Iterator<Item = (usize, Result<VarDatum, FiberError>)>> {
    // The feature needs to be an Rc so it can be duplicated into each datum
    let f = Rc::new(f);

    // The geometry member is an option, without which the feature is irrelevant
    if let Some(g) = &f.geometry {
        Box::new(convert_geom(&g).map(move |res| {
            (
                i,
                res.map(|geom| VarDatum::new(geom, VarMeta::Json(Rc::clone(&f)), i))
                    .map_err(|_| FiberError::Arg("GeoJson error")),
            )
        }))
    } else {
        Box::new(once((
            i,
            Err(FiberError::Arg("Feature does not have a geometry")),
        )))
    }
}
