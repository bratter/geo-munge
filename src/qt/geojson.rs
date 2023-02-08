use std::iter::once;
use std::path::PathBuf;
use std::sync::Arc;

use geo::Point;
use geojson::feature::Id;
use geojson::{Feature, GeoJson};
use serde_json::Value;

use crate::error::{Error, ParseType};
use crate::geojson::{convert_geom, read_geojson};

use super::datum::{BaseData, Datum};
use super::QtData;
use super::Quadtree;

pub fn json_field_val(feature: &Feature, field: &String) -> String {
    // Special handling of id as it is a named property
    if field == "id" {
        match &feature.id {
            Some(Id::String(s)) => s.to_string(),
            Some(Id::Number(n)) => n.to_string(),
            None => String::default(),
        }
    } else if let Some(props) = &feature.properties {
        match props.get(field) {
            Some(Value::Null) => String::default(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) => s.to_owned(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Array(_)) => String::default(),
            Some(Value::Object(_)) => String::default(),
            None => String::default(),
        }
    } else {
        String::default()
    }
}

pub fn build_geojson(path: PathBuf, opts: QtData) -> Result<Quadtree, Error> {
    let geojson = read_geojson(&path)?;
    let mut qt = Quadtree::new(opts);

    // Create an iterator that runs through and flattens all geometries in the GeoJson, preparing
    // them for adding to the qt
    let geometries: Box<dyn Iterator<Item = _>> = match geojson {
        GeoJson::Geometry(g) => {
            // Note that geometries don't contain any metadata, so using the None meta here
            Box::new(convert_geom(&g).map(|res| {
                res.map(|geom| Datum::new(geom, BaseData::None, 0))
                    .map_err(|_| Error::CannotParseRecord(0, ParseType::GeoJson))
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

    // Insert into the quadtree, chaining errors to print to stderr if the insertion fails
    for datum in geometries {
        if let Some(err) = datum.and_then(|d| qt.insert(d)).err() {
            eprintln!("{err}");
        }
    }

    Ok(qt)
}

pub fn geojson_bbox(path: &PathBuf) -> Result<(Point, Point), Error> {
    let json = read_geojson(path)?;
    let bbox = match json {
        GeoJson::Feature(f) => f.bbox,
        GeoJson::Geometry(g) => g.bbox,
        GeoJson::FeatureCollection(fc) => fc.bbox,
    };
    let bbox = bbox.ok_or(Error::MissingBoundingBox)?;

    // Ensure that the bounding box has length 4 to guarantee we can build a proper
    // bouding box
    if bbox.len() != 2 {
        return Err(Error::InvalidBoundingBox);
    }

    Ok((Point::new(bbox[0], bbox[1]), Point::new(bbox[2], bbox[3])))
}

/// Convenience function to build a feature iterator over a single geojson feature, using, for
/// convenience, output in the form of an `enumerate` on an `Iterator`.
fn map_feature((i, f): (usize, Feature)) -> Box<dyn Iterator<Item = Result<Datum, Error>>> {
    // The feature needs to be an Rc so it can be duplicated into each datum
    let f = Arc::new(f);

    // The geometry member is an option, without which the feature is irrelevant
    if let Some(g) = &f.geometry {
        Box::new(convert_geom(&g).map(move |res| {
            res.map(|geom| Datum::new(geom, BaseData::Json(Arc::clone(&f)), i))
                .map_err(|_| Error::CannotParseRecord(i, ParseType::GeoJson))
        }))
    } else {
        Box::new(once(Err(Error::CannotParseRecord(
            i,
            ParseType::MissingGeometry,
        ))))
    }
}
