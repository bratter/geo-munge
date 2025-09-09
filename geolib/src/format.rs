use std::{borrow::Cow, io::BufRead};

use anyhow::Result;
use geo::Geometry;

use crate::{
    csv::{CsvReader, CsvTransformer},
    geojson::{JsonStreamReader, JsonTransformer, NdjsonReader, NdjsonTransformer},
    kml::KmlReader,
    shp::ShapefileReader,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    Full,
    Geometry,
    Properties,
}

/// We use a json object as our intermediate property representation.
pub type Properties = serde_json::Map<String, serde_json::Value>;

pub use serde_json::Value;

/// Wrapper object for a geometry and optional properties.
#[derive(Debug, Default)]
pub struct GeoItem {
    pub geom: Option<Geometry>,
    pub props: Option<Properties>,
}

impl GeoItem {
    pub fn new(geom: Geometry, meta: Option<Properties>) -> Self {
        Self {
            geom: Some(geom),
            props: meta,
        }
    }

    pub fn without_props(geom: Geometry) -> Self {
        Self {
            geom: Some(geom),
            props: None,
        }
    }

    pub fn with_props(geom: Geometry, meta: Properties) -> Self {
        Self {
            geom: Some(geom),
            props: Some(meta),
        }
    }

    pub fn props_only(meta: Properties) -> Self {
        Self {
            geom: None,
            props: Some(meta),
        }
    }
}

impl From<Geometry> for GeoItem {
    fn from(value: Geometry) -> Self {
        Self::new(value, None)
    }
}

/// Trait indicating that a type is an Iterator over [`GeoItem`]s.
pub trait GeoItemIterator: Iterator<Item = Result<GeoItem>> {}

impl<T> GeoItemIterator for T where T: Iterator<Item = Result<GeoItem>> {}

/// Monomorphization of underlying readers to avoid the need for dynamic dispatch.
pub enum FormatReader<R: BufRead> {
    Json(JsonStreamReader),
    Ndjson(NdjsonReader<R>),
    Shp(ShapefileReader),
    Kml(KmlReader),
    Csv(CsvReader<R>),
}

impl<R: BufRead> Iterator for FormatReader<R> {
    type Item = Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Json(iter) => iter.next(),
            Self::Ndjson(iter) => iter.next(),
            Self::Shp(iter) => iter.next(),
            Self::Kml(iter) => iter.next(),
            Self::Csv(iter) => iter.next(),
        }
    }
}

/// Monomorphization of underlying output transformers to avoid the need for dynamic dispatch.
pub enum FormatTransformer<I: GeoItemIterator> {
    Json(JsonTransformer<I>),
    Ndjson(NdjsonTransformer<I>),
    Csv(CsvTransformer<I>),
}

impl<I: GeoItemIterator> Iterator for FormatTransformer<I> {
    type Item = Result<Cow<'static, [u8]>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Json(iter) => iter.next(),
            Self::Ndjson(iter) => iter.next(),
            Self::Csv(iter) => iter.next(),
        }
    }
}
