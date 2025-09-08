use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{anyhow, Result};
use shapefile::dbase::{Date, DateTime, Record};
use shapefile::reader::{ShapeIterator, ShapeRecordIterator};
use shapefile::Reader;
use shapefile::{dbase::FieldValue, Shape, ShapeReader};

use crate::format::{ContentMode, GeoItem, Value};

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
    pub fn try_new(file: impl AsRef<Path>, mode: ContentMode) -> Result<Self> {
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

    fn next_shape(shape: Shape) -> Result<GeoItem> {
        Ok(GeoItem::without_props(
            geo::Geometry::try_from(shape).map_err(|e| anyhow!(e))?,
        ))
    }

    fn next_shape_record((shape, record): (Shape, Record), use_shape: bool) -> Result<GeoItem> {
        let mut item = if use_shape {
            Self::next_shape(shape)?
        } else {
            GeoItem::default()
        };

        let properties = record
            .into_iter()
            .map(|(k, fv)| (k, field_to_json(fv)))
            .collect();

        item.props = Some(properties);

        Ok(item)
    }
}

impl Iterator for ShapefileReader {
    type Item = Result<GeoItem>;

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

fn field_to_json(fv: FieldValue) -> Value {
    match fv {
        FieldValue::Character(Some(s)) => Value::String(s),
        FieldValue::Character(None) => Value::Null,
        FieldValue::Numeric(Some(n)) => match serde_json::Number::from_f64(n) {
            Some(n) => Value::Number(n),
            None => Value::Null,
        },
        FieldValue::Numeric(None) => Value::Null,
        FieldValue::Float(Some(n)) => match serde_json::Number::from_f64(n as f64) {
            Some(n) => Value::Number(n),
            None => Value::Null,
        },
        FieldValue::Float(None) => Value::Null,
        FieldValue::Logical(Some(b)) => Value::Bool(b),
        FieldValue::Logical(None) => Value::Null,
        FieldValue::Date(Some(d)) => Value::String(to_iso8601_date(&d)),
        FieldValue::Date(None) => Value::Null,
        FieldValue::Integer(i) => Value::Number(i.into()),
        FieldValue::Currency(c) => match serde_json::Number::from_f64(c) {
            Some(n) => Value::Number(n),
            None => Value::Null,
        },
        FieldValue::DateTime(d) => Value::String(to_iso8601_datetime(&d)),
        FieldValue::Double(f) => match serde_json::Number::from_f64(f) {
            Some(n) => Value::Number(n),
            None => Value::Null,
        },
        FieldValue::Memo(s) => Value::String(s),
    }
}

pub fn to_iso8601_date(date: &Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

pub fn to_iso8601_datetime(datetime: &DateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        datetime.date().year(),
        datetime.date().month(),
        datetime.date().day(),
        datetime.time().hours(),
        datetime.time().minutes(),
        datetime.time().seconds()
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn shapefile_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/sample_shapefile/stations.shp")
    }

    #[test]
    fn shapefile_reader_emits_features_full_mode() {
        let reader = ShapefileReader::try_new(shapefile_path(), ContentMode::Full).unwrap();
        let features: Vec<_> = reader.map(|result| result.unwrap()).collect();

        assert_eq!(features.len(), 86);

        let first_feature = &features[0];
        assert!(matches!(
            first_feature.geom,
            Some(geo::Geometry::Point(geo::Point(_)))
        ));

        // Check properties
        let first_props = first_feature.props.as_ref().unwrap();
        assert_eq!(
            first_props.get("line"),
            Some(&Value::String("blue".to_string()))
        );
    }

    #[test]
    fn shapefile_reader_emits_features_properties_mode() {
        let reader = ShapefileReader::try_new(shapefile_path(), ContentMode::Properties).unwrap();
        let features: Vec<_> = reader.map(|result| result.unwrap()).collect();

        assert_eq!(features.len(), 86);

        // First item should have no geometry but properties
        let first_feature = &features[0];
        assert!(first_feature.geom.is_none());

        // Check properties
        let first_props = first_feature.props.as_ref().unwrap();
        assert_eq!(
            first_props.get("line"),
            Some(&Value::String("blue".to_string()))
        );
    }
}
