use std::path::PathBuf;
use std::{fs::read_to_string, iter::once};

use geojson::feature::Id;
use geojson::{Feature, GeoJson};
use quadtree::*;
use serde_json::Value;

use crate::error::FiberError;

use super::{
    datum::IndexedDatum, make_dyn_qt, QtData, SearchResult, Searchable, SearchableWithMeta,
};

// TODO: Try to get this working, perhaps using an unsafe?
// This should be fine if the GeoJson is stored as Pin<Box<GeoJson>> then can have raw pointers to
// it, or even Pin<Rc<GeoJson>> so we know we won't drop it, but is that enough - does everything
// inside needto be pinned also?
// pub struct JsonMeta<'a>(&'a geojson::Feature);
// impl<'a> JsonMeta<'a> {
//     pub fn meta(&self, fields: &'a Option<Vec<String>>) -> Box<dyn Iterator<Item = String> + 'a> {
//         todo!()
//     }
// }

pub struct JsonWithMeta {
    qt: Box<dyn Searchable<IndexedDatum<()>>>,
    geojson: GeoJson,
}

enum FeatureOrGeom<'a> {
    Feature(&'a Feature),
    Geom(&'a geojson::Geometry),
}

impl FeatureOrGeom<'_> {
    pub fn id(&self) -> Option<String> {
        match self {
            Self::Geom(_) => None,
            Self::Feature(f) => match f.id.as_ref()? {
                Id::String(s) => Some(s.to_string()),
                Id::Number(n) => Some(n.to_string()),
            },
        }
    }

    pub fn properties(&self) -> &Option<serde_json::map::Map<String, serde_json::Value>> {
        match self {
            Self::Geom(_) => &None,
            Self::Feature(f) => &f.properties,
        }
    }
}

// TODO: Should we deal with arrays and objects differently?
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

impl JsonWithMeta {
    fn make_search_result<'a>(
        &'a self,
        found: (&'a IndexedDatum, f64),
        fields: &'a Option<Vec<String>>,
    ) -> SearchResult {
        let (datum, distance) = found;
        let feature = match &self.geojson {
            GeoJson::Geometry(g) => FeatureOrGeom::Geom(g),
            GeoJson::Feature(f) => FeatureOrGeom::Feature(f),
            GeoJson::FeatureCollection(fc) => FeatureOrGeom::Feature(&fc.features[datum.index]),
        };

        // First item in the iterator is the id, or an empty string if it doesn't exist
        // TODO: Here we clone the properties and then move the clone into the closure so that we
        // are not returning a reference to a local variable, but if this is cloning the underlying
        // data it is both expensive and unecessary - so how to eliminiate?
        // We should be able to eliminate the clone by not using the FeatureOrGeom enum and just
        // matching on the geojson type in here - its not as nice an API, but it does avoid the
        // cloning madness.
        let id = once(feature.id().unwrap_or_default());
        let meta: Box<dyn Iterator<Item = String>> =
            match (fields.as_ref(), feature.properties().clone()) {
                (Some(fields), Some(props)) => {
                    Box::new(id.chain(fields.iter().map(move |field| {
                        props.get(field).map(map_json_value).unwrap_or_default()
                    })))
                }
                _ => Box::new(id),
            };

        // TODO: Do we have to make this such that we have an iterator that stores the state? Would
        // require a big reorganization of how it works. basically have to get the iterator to take
        // ownership of the feature so that it can be returned safely, but probably becomes moot if
        // we do the unsafe thing
        SearchResult {
            datum,
            distance,
            meta,
        }
    }
}

impl std::fmt::Display for JsonWithMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.qt.fmt(f)
    }
}

impl SearchableWithMeta for JsonWithMeta {
    fn size(&self) -> usize {
        self.qt.size()
    }

    fn find<'a>(
        &'a self,
        cmp: &geo::Point,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<super::SearchResult<'a>, quadtree::Error> {
        let item = self.qt.find(cmp, r)?;
        Ok(self.make_search_result(item, fields))
    }

    fn knn<'a>(
        &'a self,
        cmp: &geo::Point,
        k: usize,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<Vec<SearchResult<'a>>, quadtree::Error> {
        Ok(self
            .qt
            .knn(cmp, k, r)?
            .into_iter()
            .map(|item| self.make_search_result(item, fields))
            .collect())
    }
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
    (i, f): (usize, &Feature),
) -> Box<dyn Iterator<Item = Result<(Geometry<f64>, usize), FiberError>>> {
    // the geometry member is an option, so need to handle separately
    if let Some(g) = &f.geometry {
        Box::new(convert_geom(&g).map(move |res| {
            res.map(|g| (g, i))
                .map_err(|_| FiberError::Arg("GeoJson error"))
        }))
    } else {
        Box::new(once(Err(FiberError::Arg(
            "Feature does not have a geometry",
        ))))
    }
}

// TODO: This conception relies on indexing into a GeoJson FeatureCollection to pull metadata, this
// is not an ideal solution - it would be better to maintain a reference to tge original data and
// use that, but because of lifetime issues, it seems this won't be possible without a raw pointer
pub fn geojson_build<'a>(
    path: PathBuf,
    opts: QtData,
) -> Result<Box<dyn SearchableWithMeta>, FiberError> {
    let geojson = read_to_string(&path)
        .map_err(|_| FiberError::IO("Cannot read GeoJson file"))?
        .parse::<GeoJson>()
        .map_err(|_| FiberError::IO("Cannot parse GeoJson file"))?;

    // Create an iterator that runs through and flattens all geometries in the GeoJson, preparing
    // them for adding to the qt
    let items: Box<dyn Iterator<Item = _>> = match &geojson {
        GeoJson::Geometry(g) => {
            // Map the index in here rather than in the convert geometry function
            // May be less efficient, but keeps the coversion more generalized
            Box::new(convert_geom(g).map(|res| {
                res.map(|g| (g, 0))
                    .map_err(|_| FiberError::Arg("GeoJson error"))
            }))
        }
        GeoJson::Feature(f) => {
            // For features, there is still only a single index and geometry is an option
            map_feature((0, &f))
        }
        GeoJson::FeatureCollection(fc) => {
            // Feature collections we need to flatmap through the features vector and do the same
            // thing as an individual feature
            Box::new(fc.features.iter().enumerate().flat_map(map_feature))
        }
    };

    let mut qt = make_dyn_qt(&opts);
    for item in items {
        if let Ok((d, i)) = item {
            if qt.insert(IndexedDatum::without_meta(d, i)).is_err() {
                eprintln!("Cannot insert datum at index {i} into qt");
            }
        } else {
            // TODO: Move the indexing outside of the result so it can be used in the error here
            eprintln!("Could not read shape");
        }
    }

    Ok(Box::new(JsonWithMeta { qt, geojson }))
}
