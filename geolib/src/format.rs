use std::{borrow::Cow, io::BufRead};

use anyhow::Result;
use geo::Geometry;
use serde_json::{Map, Value};

use crate::geojson::{JsonReader, JsonTransformer, NdjsonReader, NdjsonTransformer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Full,
    Shapes,
    Meta,
}

/// Wrapper object for a geometry and optional properties.
#[derive(Debug)]
pub struct GeoItem {
    pub geom: Option<Geometry>,
    pub meta: Option<Meta>,
}

impl GeoItem {
    pub fn new(geom: Geometry, meta: Option<Meta>) -> Self {
        Self {
            geom: Some(geom),
            meta,
        }
    }

    pub fn without_meta(geom: Geometry) -> Self {
        Self {
            geom: Some(geom),
            meta: None,
        }
    }

    pub fn with_meta(geom: Geometry, meta: Meta) -> Self {
        Self {
            geom: Some(geom),
            meta: Some(meta),
        }
    }

    pub fn meta_only(meta: Meta) -> Self {
        Self {
            geom: None,
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

/// Monomorphization of underlying readers to avoid the need for dynamic dispatch.
pub enum FormatReader<R: BufRead> {
    Json(JsonReader),
    Ndjson(NdjsonReader<R>),
}

impl<R: BufRead> Iterator for FormatReader<R> {
    type Item = Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Json(iter) => iter.next(),
            Self::Ndjson(iter) => iter.next(),
        }
    }
}

/// Monomorphization of underlying output transformers to avoid the need for dynamic dispatch.
pub enum FormatTransformer<I: GeoItemIterator> {
    Json(JsonTransformer<I>),
    Ndjson(NdjsonTransformer<I>),
}

impl<I: GeoItemIterator> Iterator for FormatTransformer<I> {
    type Item = anyhow::Result<Cow<'static, [u8]>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Json(iter) => iter.next(),
            Self::Ndjson(iter) => iter.next(),
        }
    }
}
