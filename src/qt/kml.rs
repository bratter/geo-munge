use std::{
    collections::HashMap,
    iter::{empty, once, Once},
    path::PathBuf,
};

use quadtree::{Geometry, ToRadians};

use crate::{
    error::FiberError,
    kml::{Kml, KmlItem},
};

use super::{
    datum::IndexedDatum, make_dyn_qt, QtData, SearchResult, Searchable, SearchableWithMeta,
};

pub struct KmlWithMeta {
    qt: Box<dyn Searchable<IndexedDatum<KmlItem>>>,
}

impl KmlWithMeta {
    fn make_search_result<'a>(
        &'a self,
        found: (&'a IndexedDatum<KmlItem>, f64),
        fields: &'a Option<Vec<String>>,
    ) -> SearchResult {
        let (datum, distance) = found;
        let meta: Box<dyn Iterator<Item = String>> = match (fields, &datum.meta) {
            (Some(fields), Some(kml)) => {
                Box::new(fields.iter().map(move |f| extract_field_value(f, &kml)))
            }
            (Some(fields), None) => Box::new(fields.iter().map(|_| String::default())),
            _ => Box::new(empty()),
        };

        SearchResult {
            geom: &datum.geom,
            index: datum.index,
            distance,
            meta,
        }
    }
}

impl std::fmt::Display for KmlWithMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.qt.fmt(f)
    }
}

impl SearchableWithMeta for KmlWithMeta {
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
        let res = self.make_search_result(item, fields);
        eprintln!("{:?}", res.geom);
        Ok(self.make_search_result(item, fields))
    }

    fn knn<'a>(
        &'a self,
        cmp: &geo::Point,
        k: usize,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<Vec<SearchResult<'a>>, quadtree::Error> {
        eprintln!("{:?}", self.qt.knn(cmp, k, r));
        Ok(self
            .qt
            .knn(cmp, k, r)?
            .into_iter()
            .map(|item| self.make_search_result(item, fields))
            .collect())
    }
}

pub fn kml_build(path: PathBuf, opts: QtData) -> Result<Box<dyn SearchableWithMeta>, FiberError> {
    let kml = Kml::from_path(&path)?;
    let mut qt = make_dyn_qt(&opts);

    // Get the set of kml items that we want to attempt to load into the qt
    // The iterator here only emits a subset of Kml types
    for datum in kml.into_iter().enumerate().flat_map(map_kml_item) {
        if let Ok(datum) = datum {
            let i = datum.index;
            if qt.insert(datum).is_err() {
                eprintln!("Cannot insert datum at index {i} into qt");
            }
        } else {
            eprintln!("Could not read shape");
        }
    }

    Ok(Box::new(KmlWithMeta { qt }))
}

/// Make output strings from a field name and the Kml item.
fn extract_field_value(field: &String, kml: &KmlItem) -> String {
    eprintln!("{:?}", kml);
    match kml {
        KmlItem::Point(p) => make_string(&p.attrs, field),
        KmlItem::Polygon(p) => make_string(&p.attrs, field),
        KmlItem::Location(l) => make_string(&l.attrs, field),
        KmlItem::LinearRing(l) => make_string(&l.attrs, field),
        KmlItem::LineString(l) => make_string(&l.attrs, field),
        KmlItem::Placemark(p) => {
            if field == "name" {
                p.name.to_owned().unwrap_or_default()
            } else if field == "description" {
                p.description.to_owned().unwrap_or_default()
            } else {
                make_string(&p.attrs, field)
            }
        }
        KmlItem::MultiGeometry(_) => unreachable!("Nested MultiGeometries not allowed"),
    }
}

fn make_string(attrs: &HashMap<String, String>, field: &String) -> String {
    attrs.get(field).map(|s| s.to_string()).unwrap_or_default()
}

/// Map from a [`KmlItem`] and its associated index to an iterator of [`IndexedDatum`]. Most items
/// are wrapped in a single item iterator, but multi-kml types are expanded. This relies on copying
/// which is both time and space inefficient for large geometries, but this is required in order to
/// keep both.
fn map_kml_item(
    (index, item): (usize, KmlItem),
) -> Box<dyn Iterator<Item = Result<IndexedDatum<KmlItem>, FiberError>>> {
    match item {
        KmlItem::Point(ref p) => {
            let mut geo = geo::Point::from(p.clone());
            geo.to_radians_in_place();
            bood(Geometry::Point(geo), item, index)
        }
        KmlItem::Polygon(ref p) => {
            let mut geo = geo::Polygon::from(p.clone());
            geo.to_radians_in_place();
            bood(Geometry::Polygon(geo), item, index)
        }
        KmlItem::LinearRing(ref l) => {
            let mut geo = geo::LineString::from(l.clone());
            geo.to_radians_in_place();
            bood(Geometry::LineString(geo), item, index)
        }
        KmlItem::LineString(ref l) => {
            let mut geo = geo::LineString::from(l.clone());
            geo.to_radians_in_place();
            bood(Geometry::LineString(geo), item, index)
        }
        KmlItem::Placemark(p) => Box::new(once(
            // TODO: Clone required here to ensure placemark is provided as the meta, but
            // should be eliminated by reworking something
            p.clone()
                .geometry
                .ok_or(FiberError::Arg("Placemark does not have any geometry"))
                .and_then(convert_kml_geom)
                .map(|(geom, _)| IndexedDatum {
                    geom,
                    index,
                    meta: Some(KmlItem::Placemark(p)),
                }),
        )),
        KmlItem::Location(ref l) => bood(
            Geometry::Point(geo::point! {
                x: l.latitude,
                y: l.longitude,
            }),
            item,
            index,
        ),
        KmlItem::MultiGeometry(mg) => Box::new(mg.geometries.into_iter().map(move |g| {
            convert_kml_geom(g).map(|(geom, meta)| IndexedDatum {
                geom,
                index,
                meta: Some(meta),
            })
        })),
    }
}

/// (B)ox (O)nce (O)k (D)atum. Convenience function to wrap the inputs in the appropriate
/// containers for subsequent use.
fn bood(
    geom: Geometry<f64>,
    meta: KmlItem,
    index: usize,
) -> Box<Once<Result<IndexedDatum<KmlItem>, FiberError>>> {
    Box::new(once(Ok(IndexedDatum {
        geom,
        index,
        meta: Some(meta),
    })))
}

/// Helper function to convert kml geometries into geo-type geometries when kml geomerties are
/// available from a MultiGeomety field.
fn convert_kml_geom(item: kml::types::Geometry) -> Result<(Geometry<f64>, KmlItem), FiberError> {
    match item {
        kml::types::Geometry::Point(p) => {
            let mut geo = geo::Point::from(p.clone());
            geo.to_radians_in_place();
            Ok((Geometry::Point(geo), KmlItem::Point(p)))
        }
        kml::types::Geometry::Polygon(p) => {
            let mut geo = geo::Polygon::from(p.clone());
            geo.to_radians_in_place();
            Ok((Geometry::Polygon(geo), KmlItem::Polygon(p)))
        }
        kml::types::Geometry::LineString(l) => {
            let mut geo = geo::LineString::from(l.clone());
            geo.to_radians_in_place();

            Ok((Geometry::LineString(geo), KmlItem::LineString(l)))
        }
        kml::types::Geometry::LinearRing(l) => {
            let mut geo = geo::LineString::from(l.clone());
            geo.to_radians_in_place();

            Ok((Geometry::LineString(geo), KmlItem::LinearRing(l)))
        }
        kml::types::Geometry::MultiGeometry(_) => Err(FiberError::Arg(
            "Nested KML MultiGeometries are not supported",
        )),
        kml::types::Geometry::Element(_) => {
            Err(FiberError::Arg("Elements do not contain geometry data"))
        }
        _ => Err(FiberError::Arg("Unknown type")),
    }
}
