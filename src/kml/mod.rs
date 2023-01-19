use std::{iter::FlatMap, path::PathBuf};

use kml::{types::*, KmlReader};
use quadtree::{Geometry, ToRadians};

use crate::error::FiberError;

/// Return a [`kml::Kml`] object loaded from a `.kml` or `.kmz` file.
pub fn read_kml(path: &PathBuf) -> Result<kml::Kml, FiberError> {
    let ext = path
        .extension()
        .ok_or(FiberError::IO("Invalid extension"))?;

    if ext == "kml" {
        KmlReader::<_, f64>::from_path(path.clone())
            .map_err(|_| FiberError::IO("Cannot read KML file"))
            .and_then(|mut r| {
                r.read()
                    .map_err(|_| FiberError::Parse(0, "Couldn't parse KML file"))
            })
    } else if ext == "kmz" {
        KmlReader::<_, f64>::from_kmz_path(path.clone())
            .map_err(|_| FiberError::IO("Cannot read KMZ file"))
            .and_then(|mut r| {
                r.read()
                    .map_err(|_| FiberError::Parse(0, "Couldn't parse KMZ file"))
            })
    } else {
        Err(FiberError::IO("Invalid extension"))
    }
}

/// Helper function to convert kml geometries into geo-type geometries when kml geomerties are
/// available from a MultiGeomety field.
pub fn convert_kml_geom(
    item: kml::types::Geometry,
) -> Result<(Geometry<f64>, KmlItem), FiberError> {
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

/// Wrapper around a Kml enum for custom iterators. These custom iterators only emit the kml
/// components that are useful for proximity processing - i.e. the ones that contain geometries.
pub struct Kml {
    kml: kml::Kml,
}

impl Kml {
    /// Build a new Kml document from the path to a KML or KMZ file.
    pub fn from_path(path: &PathBuf) -> Result<Self, FiberError> {
        Ok(Self {
            kml: read_kml(path)?,
        })
    }

    pub fn iter(&self) -> IntoIterRef {
        self.into_iter()
    }
}

impl From<kml::Kml> for Kml {
    fn from(kml: kml::Kml) -> Self {
        Kml { kml }
    }
}

impl IntoIterator for Kml {
    type Item = KmlItem;
    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        match self.kml {
            kml::Kml::KmlDocument(d) => IntoIter::Iter(Box::new(
                d.elements
                    .into_iter()
                    .flat_map(|k| Kml::from(k).into_iter()),
            )),
            kml::Kml::Document { attrs: _, elements } | kml::Kml::Folder { attrs: _, elements } => {
                IntoIter::Iter(Box::new(
                    elements.into_iter().flat_map(|k| Kml::from(k).into_iter()),
                ))
            }
            kml::Kml::MultiGeometry(d) => IntoIter::Once(KmlItem::MultiGeometry(d)),
            kml::Kml::LinearRing(d) => IntoIter::Once(KmlItem::LinearRing(d)),
            kml::Kml::LineString(d) => IntoIter::Once(KmlItem::LineString(d)),
            kml::Kml::Location(d) => IntoIter::Once(KmlItem::Location(d)),
            kml::Kml::Point(d) => IntoIter::Once(KmlItem::Point(d)),
            kml::Kml::Placemark(d) => IntoIter::Once(KmlItem::Placemark(d)),
            kml::Kml::Polygon(d) => IntoIter::Once(KmlItem::Polygon(d)),
            // Ignore all else
            _ => IntoIter::Empty,
        }
    }
}

/// Holds a subset of Kml members that might be emitted by the iterator.
#[derive(Debug)]
pub enum KmlItem {
    MultiGeometry(MultiGeometry),
    LinearRing(LinearRing),
    LineString(LineString),
    Location(Location),
    Placemark(Placemark),
    Point(Point),
    Polygon(Polygon),
}

/// The owned Kml iterator.
pub enum IntoIter {
    Iter(Box<FlatIter>),
    Once(KmlItem),
    Empty,
}

impl Iterator for IntoIter {
    type Item = KmlItem;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            IntoIter::Iter(iter) => iter.next(),
            // Swap out the borrowed IntoIter for the new empty state
            // But the compiler "forgets" that we've already matched,
            // hence requiring the if-let
            once @ IntoIter::Once(_) => {
                if let IntoIter::Once(item) = std::mem::replace(once, IntoIter::Empty) {
                    Some(item)
                } else {
                    unreachable!()
                }
            }
            IntoIter::Empty => None,
        }
    }
}

/// Convenience type for an inner KML iterator of owned objects
type FlatIter = FlatMap<std::vec::IntoIter<kml::Kml>, IntoIter, fn(kml::Kml) -> IntoIter>;

impl<'a> IntoIterator for &'a Kml {
    type Item = KmlItemRef<'a>;
    type IntoIter = IntoIterRef<'a>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIterRef::new(&self.kml)
    }
}

/// Holds a subset of Kml members that might be emitted by a reference iterator
#[derive(Debug, Clone)]
pub enum KmlItemRef<'a> {
    MultiGeometry(&'a MultiGeometry),
    LinearRing(&'a LinearRing),
    LineString(&'a LineString),
    Location(&'a Location),
    Placemark(&'a Placemark),
    Point(&'a Point),
    Polygon(&'a Polygon),
}

pub enum IntoIterRef<'a> {
    Iter(Box<FlatIterRef<'a>>),
    Once(KmlItemRef<'a>),
    Empty,
}

impl<'a> IntoIterRef<'a> {
    fn new(kml: &'a kml::Kml) -> Self {
        match kml {
            kml::Kml::KmlDocument(d) => IntoIterRef::Iter(Box::new(
                d.elements.iter().flat_map(|k| IntoIterRef::new(&k)),
            )),
            kml::Kml::Document { attrs: _, elements } | kml::Kml::Folder { attrs: _, elements } => {
                IntoIterRef::Iter(Box::new(elements.iter().flat_map(|k| IntoIterRef::new(k))))
            }
            kml::Kml::MultiGeometry(d) => IntoIterRef::Once(KmlItemRef::MultiGeometry(d)),
            // Ignore all else
            _ => IntoIterRef::Empty,
        }
    }
}

impl<'a> Iterator for IntoIterRef<'a> {
    type Item = KmlItemRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            IntoIterRef::Iter(iter) => iter.next(),
            IntoIterRef::Once(item) => Some(item.clone()),
            IntoIterRef::Empty => None,
        }
    }
}

/// Convenience type for an inner KML iterator of borrowed objects
type FlatIterRef<'a> =
    FlatMap<std::slice::Iter<'a, kml::Kml>, IntoIterRef<'a>, fn(&kml::Kml) -> IntoIterRef>;
