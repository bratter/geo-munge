use std::{borrow::Cow, collections::BTreeMap, fmt::Display, io::BufRead, path::PathBuf};

use anyhow::{anyhow, Error, Result};
use clap::ValueEnum;
use geo::Geometry;
use shapefile::dbase::{Date as DbaseDate, DateTime as DbaseDateTime};

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

/// Wrapper object for a geometry and optional properties.
#[derive(Debug, Default)]
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

    pub fn without_props(geom: Geometry) -> Self {
        Self {
            geom: Some(geom),
            meta: None,
        }
    }

    pub fn with_props(geom: Geometry, meta: Meta) -> Self {
        Self {
            geom: Some(geom),
            meta: Some(meta),
        }
    }

    pub fn props_only(meta: Meta) -> Self {
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

/// Available GIS formats for input and output
/// TODO: Can we get the ValueEnum out of here, it doesn't fit in the library. Might just require a separate enum with a
/// From in the Args file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Shapefile.
    ///
    /// Streamable, reading and writing can be done by feature.
    /// TODO: Need to work on transformation and writing
    Shp,

    /// JSON.
    ///
    /// Uses geojson's permissive, streaming parser and therefore only works on FeatureCollections.
    /// TODO: Support for collections outside of the top level
    JsonStream,

    /// Newline delimited JSON.
    ///
    /// Streamable, with individual features separated by `\n` (input also supports \r\n`). Each underlying feature must
    /// be a valid geojson Feature, we do not support FeatureCollections or GeometryCollections for simplicity and
    /// compatibility.
    Ndjson,

    /// KML, uncompressed.
    ///
    /// Not streamable, requires buffering the whole file in memory for input or output.
    /// TODO: Need to work on transformation and writing
    Kml,

    /// KMZ, compressed KML.
    ///
    /// Not streamable, requires buffering the whole file in memory for input or output.
    /// TODO: Implement this by hand using the zip crate. Looks like the reader needs read + seek, but there is a
    /// read_zipfile_from_stream method that might be useful. Zip can also be used on other formats too.
    Kmz,

    // TODO: WKB and WKT
    // They are both technically streamable as it is easy to parse a termination point
    // Same comment that we won't parse a collection unless it is the first feature
    // Also include csv as a variant of Wkt that has Wkt in a geom column, otherwise can't have metadata
    Wkb,
    Wkt,
    Csv,
}

impl TryFrom<&PathBuf> for Format {
    type Error = Error;

    fn try_from(path: &PathBuf) -> Result<Self, Self::Error> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("shp") => Ok(Format::Shp),
            Some("json") | Some("geojson") => Ok(Format::JsonStream),
            Some("ndjson") => Ok(Format::Ndjson),
            Some("kml") => Ok(Format::Kml),
            Some("kmz") => Ok(Format::Kmz),
            Some("wkb") => Ok(Format::Wkb),
            Some("wkt") => Ok(Format::Wkt),
            Some("csv") => Ok(Format::Csv),
            _ => Err(anyhow!("Unknown format")),
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Shp => "shp",
            Self::JsonStream => "json",
            Self::Ndjson => "ndjson",
            Self::Kml => "kml",
            Self::Kmz => "kmz",
            Self::Wkb => "wkb",
            Self::Wkt => "wkt",
            Self::Csv => "csv",
        };

        f.write_str(name)
    }
}

/// Intermediate representation for metadata.
pub type Meta = BTreeMap<String, Value>;

/// Abstract value for attribute data across GIS formats.
///
/// TODO: Upgrade handlilng for all types
#[derive(Debug, PartialEq)]
pub enum Value {
    String(String),
    Float(f64),
    Integer(i64),
    Boolean(bool),
    // NOTE: Using naive dbase Date/Time representations as they are just simple transport and shapefile is going to be
    // one of the few formats with these types - this avoids requiring a new dependency
    Date(Date),
    DateTime(DateTime),
    // TODO: Null should be coercable into a non-null type to account for cases where something is missing in a
    // permissive format in some records
    Null,
}

#[derive(Debug, PartialEq)]
pub struct Date(DbaseDate);

impl Date {
    pub fn into_inner(self) -> DbaseDate {
        self.0
    }
}

impl From<DbaseDate> for Date {
    fn from(value: DbaseDate) -> Self {
        Self(value)
    }
}

impl Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.0;
        write!(
            f,
            "{:04}-{:02}-{:02}",
            inner.year(),
            inner.month(),
            inner.day()
        )
    }
}

#[derive(Debug, PartialEq)]
pub struct DateTime(DbaseDateTime);

impl DateTime {
    pub fn into_inner(self) -> DbaseDateTime {
        self.0
    }
}

impl From<DbaseDateTime> for DateTime {
    fn from(value: DbaseDateTime) -> Self {
        Self(value)
    }
}

impl Display for DateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let d = self.0.date();
        let t = self.0.time();
        write!(
            f,
            "{}T{:02}:{:02}:{:02}",
            Date::from(d),
            t.hours(),
            t.minutes(),
            t.seconds()
        )
    }
}

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
