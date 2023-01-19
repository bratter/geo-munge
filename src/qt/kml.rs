use std::{
    collections::HashMap,
    iter::{once, Once},
    path::PathBuf,
};

use quadtree::{Geometry, ToRadians};

use crate::{
    error::FiberError,
    kml::{convert_kml_geom, Kml, KmlItem},
};

use super::{
    datum::{BaseData, Datum},
    QtData, Quadtree,
};

/// Make output strings from a field name and the Kml item.
pub fn kml_field_val(kml: &KmlItem, field: &String) -> String {
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

/// Build the quadtree for kml-based input data
pub fn build_kml(path: PathBuf, opts: QtData) -> Result<Quadtree, FiberError> {
    let kml = Kml::from_path(&path)?;
    let mut qt = Quadtree::new(opts);

    for (index, datum) in kml.into_iter().enumerate().flat_map(map_kml_item) {
        if let Ok(datum) = datum {
            if qt.insert(datum).is_err() {
                eprintln!("Cannot insert datum at index {index} into qt");
            }
        } else {
            eprintln!("Could not read shape at index {index}");
        }
    }

    Ok(qt)
}

/// Map from a [`KmlItem`] and its associated index to an iterator of [`IndexedDatum`]. Most items
/// are wrapped in a single item iterator, but multi-kml types are expanded. This relies on copying
/// which is both time and space inefficient for large geometries, but this is required in order to
/// keep both.
fn map_kml_item(
    (index, item): (usize, KmlItem),
) -> Box<dyn Iterator<Item = (usize, Result<Datum, FiberError>)>> {
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
        KmlItem::Placemark(p) => Box::new(once((
            index,
            // TODO: Clone required here to ensure placemark is provided as the meta, but
            // should be eliminated by reworking something
            p.clone()
                .geometry
                .ok_or(FiberError::Arg("Placemark does not have any geometry"))
                .and_then(convert_kml_geom)
                .map(|(geom, _)| Datum::new(geom, BaseData::Kml(KmlItem::Placemark(p)), index)),
        ))),
        KmlItem::Location(ref l) => bood(
            Geometry::Point(geo::point! {
                x: l.latitude,
                y: l.longitude,
            }),
            item,
            index,
        ),
        KmlItem::MultiGeometry(mg) => Box::new(mg.geometries.into_iter().map(move |g| {
            (
                index,
                convert_kml_geom(g)
                    .map(|(geom, meta)| Datum::new(geom, BaseData::Kml(meta), index)),
            )
        })),
    }
}

/// (B)ox (O)nce (O)k (D)atum. Convenience function to wrap the inputs in the appropriate
/// containers for subsequent use.
fn bood(
    geom: Geometry<f64>,
    meta: KmlItem,
    index: usize,
) -> Box<Once<(usize, Result<Datum, FiberError>)>> {
    Box::new(once((
        index,
        Ok(Datum::new(geom, BaseData::Kml(meta), index)),
    )))
}
