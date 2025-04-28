use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use std::iter::once;
use std::path::Path;

use anyhow::{anyhow, bail};
use shapefile::dbase::Record;
use shapefile::reader::{ShapeIterator, ShapeRecordIterator};
use shapefile::Reader;
use shapefile::{dbase::FieldValue, Shape, ShapeReader};

use quadtree::*;

use crate::error::{Error, UnsupportedGeoType};
use crate::format::{GeoItem, GeoItemIterator, Meta, MetaMode, Value};

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

enum ShapeIter {
    Shape(ShapeIterator<'static, BufReader<File>, Shape>),
    ShapeRecord(ShapeRecordIterator<'static, BufReader<File>, BufReader<File>, Shape, Record>),
}

// TODO: The new version starts here
// TODO: Make some notes about the box leak and how static bound is OK as we are only passing owned readers
pub struct ShapefileReader {
    mode: MetaMode,
    shapes: ShapeIter,
}

impl ShapefileReader {
    pub fn new(file: impl AsRef<Path>, mode: MetaMode) -> anyhow::Result<Self> {
        let shapes = match mode {
            MetaMode::Full | MetaMode::Meta => {
                let reader = Box::leak(Box::new(Reader::from_path(file)?));
                ShapeIter::ShapeRecord(reader.iter_shapes_and_records())
            }
            MetaMode::Shapes => {
                let reader = Box::leak(Box::new(ShapeReader::from_path(file)?));
                ShapeIter::Shape(reader.iter_shapes())
            }
        };

        Ok(Self { mode, shapes })
    }

    fn next_shape(shape: Shape) -> anyhow::Result<GeoItem> {
        Ok(GeoItem::without_meta(
            geo::Geometry::try_from(shape).map_err(|e| anyhow!(e))?,
        ))
    }

    fn next_shape_record(
        (shape, record): (Shape, Record),
        use_shape: bool,
    ) -> anyhow::Result<GeoItem> {
        let mut item = if use_shape {
            Self::next_shape(shape)?
        } else {
            GeoItem::default()
        };

        item.meta = Some(Meta::from(RecordWrapper(record)));

        Ok(item)
    }
}

impl Iterator for ShapefileReader {
    type Item = anyhow::Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = match &mut self.shapes {
            ShapeIter::Shape(iter) => iter
                .next()?
                .map_err(|e| anyhow!(e))
                .and_then(|shape| Self::next_shape(shape)),
            ShapeIter::ShapeRecord(iter) => iter
                .next()?
                .map_err(|e| anyhow!(e))
                .and_then(|arg| Self::next_shape_record(arg, matches!(self.mode, MetaMode::Full))),
        };
        Some(result)
    }
}

// TODO: Work on outputting shapefiles. This will require some level of workarounds as it is better to create the writer
// directly, and making the dbase file for Meta requires work upfront
pub struct ShapefileTransformer<I: GeoItemIterator> {
    _iter: I,
    _mode: MetaMode,
}

impl<I: GeoItemIterator> ShapefileTransformer<I> {
    pub fn new(_iter: I, _mode: MetaMode) -> anyhow::Result<Self> {
        //Self { iter, mode }
        bail!("Cannot use Shapefile transformer")
    }
}

impl<I: GeoItemIterator> Iterator for ShapefileTransformer<I> {
    // TODO: The wrapper enum needs a cow, but may be able to map that if this is better making a String or other simpler type
    type Item = anyhow::Result<Cow<'static, [u8]>>;

    fn next(&mut self) -> Option<Self::Item> {
        unreachable!("Should be erroring Shapefile transformation")
    }
}

struct RecordWrapper(Record);

impl From<RecordWrapper> for Meta {
    fn from(record: RecordWrapper) -> Self {
        record
            .0
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    match v {
                        // TODO: Null handling - do we want to use options where they are used in dbase?
                        // Probably adjust it, but depends on what KML offers probabably
                        FieldValue::Character(Some(s)) => Value::String(s),
                        FieldValue::Character(None) => Value::Null,
                        FieldValue::Numeric(Some(n)) => Value::Float(n),
                        FieldValue::Numeric(None) => Value::Null,
                        FieldValue::Float(Some(n)) => Value::Float(n as f64),
                        FieldValue::Float(None) => Value::Null,
                        FieldValue::Logical(Some(b)) => Value::Boolean(b),
                        FieldValue::Logical(None) => Value::Null,
                        FieldValue::Date(Some(d)) => Value::Date(d.into()),
                        FieldValue::Date(None) => Value::Null,
                        FieldValue::Integer(i) => Value::Integer(i as i64),
                        // TODO: Preserve currency?
                        FieldValue::Currency(c) => Value::Float(c),
                        FieldValue::DateTime(d) => Value::DateTime(d.into()),
                        FieldValue::Double(f) => Value::Float(f),
                        FieldValue::Memo(s) => Value::String(s),
                    },
                )
            })
            .collect()
    }
}

// TODO: Error behavior
// TODO: Revist this mapping once JSON and KML are done
impl From<Meta> for RecordWrapper {
    fn from(meta: Meta) -> Self {
        let mut record = Record::default();
        for (k, v) in meta {
            let field_value = match v {
                Value::String(s) => FieldValue::Character(Some(s)),
                Value::Float(f) => FieldValue::Numeric(Some(f)),
                Value::Integer(i) => FieldValue::Integer(i as i32),
                Value::Boolean(b) => FieldValue::Logical(Some(b)),
                Value::Date(d) => FieldValue::Date(Some(d.into_inner())),
                Value::DateTime(d) => FieldValue::DateTime(d.into_inner()),
                // Default to empty string
                Value::Null => FieldValue::Character(None),
            };
            record.insert(k, field_value);
        }
        RecordWrapper(record)
    }
}
