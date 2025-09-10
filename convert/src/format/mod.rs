pub mod csv;
pub mod geojson;
pub mod html_parser;
pub mod kml;
pub mod shp;

#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::{
    borrow::Cow,
    io::{stderr, BufRead, Write},
    path::PathBuf,
};

use anyhow::Result;
use csv::CsvSettings;
use geo::Geometry;
use geojson::JsonStringReader;

use crate::{
    stream::{InputStream, OutputStream, StreamKind},
    QuietLevel,
};

use csv::{CsvReader, CsvTransformer};
use geojson::{JsonStreamReader, JsonTransformer, NdjsonReader, NdjsonTransformer};
use kml::KmlReader;
use shp::ShapefileReader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    Full,
    Geometry,
    Properties,
}

/// We use a json object as our intermediate property representation.
type Properties = serde_json::Map<String, serde_json::Value>;

use serde_json::Value;

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
    JsonString(JsonStringReader),
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
            Self::JsonString(iter) => iter.next(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamableFormat {
    JsonStream,
    JsonString,
    Ndjson,
    Csv,
}

impl std::fmt::Display for StreamableFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::JsonStream => "json-stream",
            Self::JsonString => "json-string",
            Self::Ndjson => "ndjson",
            Self::Csv => "csv",
        };

        f.write_str(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOnlyFormat {
    Shp,
    Kml,
    Kmz,
}

impl std::fmt::Display for FileOnlyFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Shp => "shp",
            Self::Kml => "kml",
            Self::Kmz => "kmz",
        };

        f.write_str(name)
    }
}

#[derive(Debug, Clone)]
pub enum InputSpec {
    Streamable {
        format: StreamableFormat,
        stream: StreamKind,
    },
    FileOnly {
        format: FileOnlyFormat,
        path: PathBuf,
    },
}

impl InputSpec {
    #[cfg(test)]
    pub fn with_str(str: String, format: StreamableFormat) -> Self {
        Self::Streamable {
            format,
            stream: StreamKind::String(str),
        }
    }

    pub fn create_reader(&self, mode: ContentMode) -> Result<impl GeoItemIterator> {
        let iter = match self {
            InputSpec::Streamable { format, stream } => {
                let reader = InputStream::try_new(stream.clone())?;
                match format {
                    StreamableFormat::JsonStream => {
                        FormatReader::Json(JsonStreamReader::new(reader, mode))
                    }
                    StreamableFormat::JsonString => {
                        FormatReader::JsonString(JsonStringReader::try_new(reader, mode)?)
                    }
                    StreamableFormat::Ndjson => {
                        FormatReader::Ndjson(NdjsonReader::new(reader, mode))
                    }
                    StreamableFormat::Csv => {
                        FormatReader::Csv(CsvReader::new(reader, mode, CsvSettings::default())?)
                    }
                }
            }
            InputSpec::FileOnly { format, path } => match format {
                FileOnlyFormat::Shp => FormatReader::Shp(ShapefileReader::try_new(path, mode)?),
                FileOnlyFormat::Kml | FileOnlyFormat::Kmz => {
                    FormatReader::Kml(KmlReader::try_new(path, mode)?)
                }
            },
        };

        Ok(iter)
    }
}

#[derive(Debug, Clone)]
pub struct OutputSpec {
    pub format: StreamableFormat,
    pub stream: StreamKind,
}

impl OutputSpec {
    pub fn new(format: StreamableFormat, stream: StreamKind) -> Self {
        Self { format, stream }
    }

    #[cfg(test)]
    pub fn with_str(str: Arc<Mutex<String>>, format: StreamableFormat) -> Self {
        Self::new(format, StreamKind::OutputString(str))
    }

    pub fn create_transformer<I>(
        &self,
        iter: I,
        mode: ContentMode,
        csv_settings: CsvSettings,
    ) -> Result<impl Iterator<Item = Result<impl AsRef<[u8]>>>>
    where
        I: GeoItemIterator,
    {
        let ft = match self.format {
            StreamableFormat::JsonStream | StreamableFormat::JsonString => {
                FormatTransformer::Json(JsonTransformer::new(iter, mode))
            }
            StreamableFormat::Ndjson => {
                FormatTransformer::Ndjson(NdjsonTransformer::new(iter, mode))
            }
            StreamableFormat::Csv => {
                FormatTransformer::Csv(CsvTransformer::new(iter, mode, csv_settings))
            }
        };
        Ok(ft)
    }

    pub fn create_writer(
        &self,
        byte_iter: impl Iterator<Item = Result<impl AsRef<[u8]>>>,
        quiet: QuietLevel,
    ) -> Result<impl Iterator<Item = Result<()>>> {
        let mut writer = OutputStream::try_new(self.stream.clone())?;

        let write_iter = byte_iter.map(move |byte_result| match byte_result {
            Ok(bytes) => {
                writer.write(&bytes.as_ref())?;
                Ok(())
            }
            Err(e) => {
                if quiet < QuietLevel::NoErrors {
                    writeln!(stderr(), "{}", e)?;
                }
                Ok(())
            }
        });
        Ok(write_iter)
    }
}
