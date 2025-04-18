use std::borrow::Cow;

use anyhow::Result;
use geo::Geometry;
use serde_json::{Map, Value};

use crate::geojson::{JsonWriter, NdjsonWriter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Full,
    Shapes,
    Meta,
}

/// Wrapper object for a geometry and optional properties.
#[derive(Debug)]
pub struct GeoItem {
    pub geom: Geometry,
    pub meta: Option<Meta>,
}

impl GeoItem {
    pub fn new(geom: Geometry, meta: Option<Meta>) -> Self {
        Self { geom, meta }
    }

    pub fn without_meta(geom: Geometry) -> Self {
        Self { geom, meta: None }
    }

    pub fn with_meta(geom: Geometry, meta: Meta) -> Self {
        Self {
            geom,
            meta: Some(meta),
        }
    }
}

impl From<Geometry> for GeoItem {
    fn from(value: Geometry) -> Self {
        Self::new(value, None)
    }
}

#[derive(Debug)]
pub enum Meta {
    Json(Map<String, Value>),
}

impl From<Map<String, Value>> for Meta {
    fn from(value: Map<String, Value>) -> Self {
        Self::Json(value)
    }
}

pub trait GeoItemIterator: Iterator<Item = Result<GeoItem>> {}

impl<T> GeoItemIterator for T where T: Iterator<Item = Result<GeoItem>> {}

pub enum Writer<I: GeoItemIterator> {
    Json(JsonWriter<I>),
    Ndjson(NdjsonWriter<I>),
}

impl<I: GeoItemIterator> Iterator for Writer<I> {
    type Item = anyhow::Result<Cow<'static, [u8]>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Json(iter) => iter.next(),
            Self::Ndjson(iter) => iter.next(),
        }
    }
}

// TODO: Is the meta type better as a generic, associated type, or a concrete? Should it have a trait that lets us read
// it like a geojson object but with some guarantees?
pub trait FormatReader {
    fn iter(self) -> impl GeoItemIterator;

    fn iter_shapes(self) -> impl GeoItemIterator;

    fn iter_meta(self) -> impl Iterator<Item = Result<Meta>>;
}

pub trait FormatWriter<I: GeoItemIterator>
where
    Self: Iterator,
{
    fn iter(iter: I, mode: Mode) -> Self;
}
