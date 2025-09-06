use std::{collections::HashMap, io::BufRead, iter::FlatMap, path::PathBuf};

use anyhow::{anyhow, Error, Result};
use kml::types::*;
use quadtree::{Geometry, ToRadians};

use crate::{
    error::{Error as GeoError, UnsupportedGeoType},
    format::{ContentMode, GeoItem, Meta, Value},
};

/// Return a [`kml::Kml`] object loaded from a `.kml` or `.kmz` file.
pub fn read_kml(path: &PathBuf) -> std::result::Result<kml::Kml, GeoError> {
    let ext = path
        .extension()
        .ok_or(GeoError::CannotParseFileExtension(path.clone()))?;

    if ext == "kml" {
        kml::KmlReader::<_, f64>::from_path(path.clone())
            .map_err(|_| GeoError::CannotReadFile(path.clone()))
            .and_then(|mut r| {
                r.read()
                    .map_err(|_| GeoError::CannotParseFile(path.clone()))
            })
    } else if ext == "kmz" {
        kml::KmlReader::<_, f64>::from_kmz_path(path.clone())
            .map_err(|_| GeoError::CannotReadFile(path.clone()))
            .and_then(|mut r| {
                r.read()
                    .map_err(|_| GeoError::CannotParseFile(path.clone()))
            })
    } else {
        Err(GeoError::UnsupportedFileType)
    }
}

/// Helper function to convert kml geometries into geo-type geometries when kml geomerties are
/// available from a MultiGeomety field.
pub fn convert_kml_geom(
    item: kml::types::Geometry,
) -> std::result::Result<(Geometry<f64>, KmlItem), GeoError> {
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
        kml::types::Geometry::MultiGeometry(_) => Err(GeoError::UnsupportedGeometry(
            UnsupportedGeoType::NestedKmlMulti,
        )),
        kml::types::Geometry::Element(_) => Err(GeoError::UnsupportedGeometry(
            UnsupportedGeoType::KmlElement,
        )),
        _ => Err(GeoError::UnsupportedGeometry(
            UnsupportedGeoType::UnknownKml,
        )),
    }
}

/// Wrapper around a Kml enum for custom iterators. These custom iterators only emit the kml
/// components that are useful for proximity processing - i.e. the ones that contain geometries.
pub struct Kml {
    kml: kml::Kml,
}

// TODO: The only place iter() is used seems to be in the Meta create, if this is going away, then we should delete it
// and all the underlying ref implementations
impl Kml {
    /// Build a new Kml document from the path to a KML or KMZ file.
    pub fn from_path(path: &PathBuf) -> std::result::Result<Self, GeoError> {
        Ok(Self {
            kml: read_kml(path)?,
        })
    }

    pub fn iter(&self) -> KmlRefIterator {
        self.into_iter()
    }
}

impl From<kml::Kml> for Kml {
    fn from(kml: kml::Kml) -> Self {
        Kml { kml }
    }
}

// TODO: Should this iterator cover the Element type? Look into it more
impl IntoIterator for Kml {
    type Item = KmlItem;
    type IntoIter = KmlIterator;

    fn into_iter(self) -> Self::IntoIter {
        match self.kml {
            kml::Kml::KmlDocument(d) => KmlIterator::Iter(Box::new(
                d.elements
                    .into_iter()
                    .flat_map(|k| Kml::from(k).into_iter()),
            )),
            kml::Kml::Document { attrs: _, elements } | kml::Kml::Folder { attrs: _, elements } => {
                KmlIterator::Iter(Box::new(
                    elements.into_iter().flat_map(|k| Kml::from(k).into_iter()),
                ))
            }
            kml::Kml::MultiGeometry(d) => KmlIterator::Once(KmlItem::MultiGeometry(d)),
            kml::Kml::LinearRing(d) => KmlIterator::Once(KmlItem::LinearRing(d)),
            kml::Kml::LineString(d) => KmlIterator::Once(KmlItem::LineString(d)),
            kml::Kml::Location(d) => KmlIterator::Once(KmlItem::Location(d)),
            kml::Kml::Point(d) => KmlIterator::Once(KmlItem::Point(d)),
            kml::Kml::Placemark(d) => KmlIterator::Once(KmlItem::Placemark(d)),
            kml::Kml::Polygon(d) => KmlIterator::Once(KmlItem::Polygon(d)),
            // Ignore all else
            _ => KmlIterator::Empty,
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

// TODO: As this also contains attrs, may want to do a version of this that also emits attrs
impl TryFrom<KmlItem> for geo::Geometry {
    type Error = Error;

    fn try_from(value: KmlItem) -> Result<Self> {
        match value {
            KmlItem::Placemark(p) => match p
                .geometry
                .ok_or(anyhow!("Placemark doesn't have geometry"))?
            {
                kml::types::Geometry::Point(p) => Self::try_from(KmlItem::Point(p)),
                kml::types::Geometry::LineString(l) => Self::try_from(KmlItem::LineString(l)),
                kml::types::Geometry::LinearRing(l) => Self::try_from(KmlItem::LinearRing(l)),
                kml::types::Geometry::Polygon(p) => Self::try_from(KmlItem::Polygon(p)),
                kml::types::Geometry::MultiGeometry(mg) => {
                    Self::try_from(KmlItem::MultiGeometry(mg))
                }
                _ => unreachable!("Will not be passed through the iterator"),
            },
            KmlItem::Point(p) => Ok(geo::Geometry::Point(geo::Point::try_from(p)?)),
            KmlItem::Location(p) => Ok(geo::Geometry::Point(geo::Point::new(
                p.longitude,
                p.latitude,
            ))),
            KmlItem::LineString(l) => Ok(geo::Geometry::LineString(geo::LineString::try_from(l)?)),
            KmlItem::LinearRing(l) => Ok(geo::Geometry::LineString(geo::LineString::try_from(l)?)),
            KmlItem::Polygon(p) => Ok(geo::Geometry::Polygon(geo::Polygon::try_from(p)?)),
            // TODO: Support multi-geometry?
            KmlItem::MultiGeometry(_) => Err(anyhow!("MultiGeometry currently not supported")),
        }
    }
}

/// The owned Kml iterator.
pub enum KmlIterator {
    Iter(Box<FlatIter>),
    Once(KmlItem),
    Empty,
}

impl Iterator for KmlIterator {
    type Item = KmlItem;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            KmlIterator::Iter(iter) => iter.next(),
            // Swap out the borrowed IntoIter for the new empty state
            // But the compiler "forgets" that we've already matched,
            // hence requiring the if-let
            once @ KmlIterator::Once(_) => {
                if let KmlIterator::Once(item) = std::mem::replace(once, KmlIterator::Empty) {
                    Some(item)
                } else {
                    unreachable!()
                }
            }
            KmlIterator::Empty => None,
        }
    }
}

/// Convenience type for an inner KML iterator of owned objects
type FlatIter = FlatMap<std::vec::IntoIter<kml::Kml>, KmlIterator, fn(kml::Kml) -> KmlIterator>;

impl<'a> IntoIterator for &'a Kml {
    type Item = KmlItemRef<'a>;
    type IntoIter = KmlRefIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        KmlRefIterator::new(&self.kml)
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

pub enum KmlRefIterator<'a> {
    Iter(Box<FlatIterRef<'a>>),
    Once(KmlItemRef<'a>),
    Empty,
}

impl<'a> KmlRefIterator<'a> {
    fn new(kml: &'a kml::Kml) -> Self {
        match kml {
            kml::Kml::KmlDocument(d) => KmlRefIterator::Iter(Box::new(
                d.elements.iter().flat_map(|k| KmlRefIterator::new(&k)),
            )),
            kml::Kml::Document { attrs: _, elements } | kml::Kml::Folder { attrs: _, elements } => {
                KmlRefIterator::Iter(Box::new(
                    elements.iter().flat_map(|k| KmlRefIterator::new(k)),
                ))
            }
            kml::Kml::MultiGeometry(d) => KmlRefIterator::Once(KmlItemRef::MultiGeometry(d)),
            // Ignore all else
            _ => KmlRefIterator::Empty,
        }
    }
}

impl<'a> Iterator for KmlRefIterator<'a> {
    type Item = KmlItemRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            KmlRefIterator::Iter(iter) => iter.next(),
            KmlRefIterator::Once(item) => Some(item.clone()),
            KmlRefIterator::Empty => None,
        }
    }
}

/// Convenience type for an inner KML iterator of borrowed objects
type FlatIterRef<'a> =
    FlatMap<std::slice::Iter<'a, kml::Kml>, KmlRefIterator<'a>, fn(&kml::Kml) -> KmlRefIterator>;

// TODO: New conversion implementation starts here
// TODO: Contemplate capturing nested attrs in the iterator
pub struct KmlReader {
    kml: KmlIterator,
    mode: ContentMode,
}

impl KmlReader {
    pub fn try_new<R: BufRead>(reader: R, mode: ContentMode) -> Result<Self> {
        let raw_kml = kml::KmlReader::<_, f64>::from_reader(reader).read()?;
        let kml = Kml::from(raw_kml).into_iter();

        Ok(Self { kml, mode })
    }

    fn make_geoitem(&self, mut item: KmlItem) -> Result<GeoItem> {
        let geoitem = match self.mode {
            ContentMode::Geometry => GeoItem::without_props(geo::Geometry::try_from(item)?),
            ContentMode::Full => {
                let meta = Meta::from(take_attrs(&mut item));
                GeoItem::with_props(geo::Geometry::try_from(item)?, meta)
            }
            ContentMode::Properties => GeoItem::props_only(Meta::from(take_attrs(&mut item))),
        };

        Ok(geoitem)
    }
}

impl Iterator for KmlReader {
    type Item = Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.kml.next()?;
        Some(self.make_geoitem(next))
    }
}

struct KmlMeta(HashMap<String, String>);

impl From<KmlMeta> for Meta {
    fn from(value: KmlMeta) -> Self {
        value
            .0
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect()
    }
}

fn take_attrs(item: &mut KmlItem) -> KmlMeta {
    let attrs = match item {
        KmlItem::LinearRing(l) => std::mem::take(&mut l.attrs),
        KmlItem::LineString(l) => std::mem::take(&mut l.attrs),
        KmlItem::Location(l) => std::mem::take(&mut l.attrs),
        KmlItem::Placemark(p) => std::mem::take(&mut p.attrs),
        KmlItem::Point(p) => std::mem::take(&mut p.attrs),
        KmlItem::Polygon(p) => std::mem::take(&mut p.attrs),
        KmlItem::MultiGeometry(m) => std::mem::take(&mut m.attrs),
    };

    KmlMeta(attrs)
}
