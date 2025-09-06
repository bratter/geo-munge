use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::anyhow;
use shapefile::dbase::Record;
use shapefile::reader::{ShapeIterator, ShapeRecordIterator};
use shapefile::Reader;
use shapefile::{dbase::FieldValue, Shape, ShapeReader};

use crate::format::{ContentMode, GeoItem, Meta, Value};

// TODO: The new version starts here
// TODO: Make some notes about the box leak and how static bound is OK as we are only passing owned readers
enum ShapeIter {
    Shape(ShapeIterator<'static, BufReader<File>, Shape>),
    ShapeRecord(ShapeRecordIterator<'static, BufReader<File>, BufReader<File>, Shape, Record>),
}

pub struct ShapefileReader {
    mode: ContentMode,
    shapes: ShapeIter,
}

impl ShapefileReader {
    pub fn new(file: impl AsRef<Path>, mode: ContentMode) -> anyhow::Result<Self> {
        let shapes = match mode {
            ContentMode::Full | ContentMode::Properties => {
                let reader = Box::leak(Box::new(Reader::from_path(file)?));
                ShapeIter::ShapeRecord(reader.iter_shapes_and_records())
            }
            ContentMode::Geometry => {
                let reader = Box::leak(Box::new(ShapeReader::from_path(file)?));
                ShapeIter::Shape(reader.iter_shapes())
            }
        };

        Ok(Self { mode, shapes })
    }

    fn next_shape(shape: Shape) -> anyhow::Result<GeoItem> {
        Ok(GeoItem::without_props(
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
            ShapeIter::ShapeRecord(iter) => iter.next()?.map_err(|e| anyhow!(e)).and_then(|arg| {
                Self::next_shape_record(arg, matches!(self.mode, ContentMode::Full))
            }),
        };
        Some(result)
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
