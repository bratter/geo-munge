use std::io::{stderr, Write};

use anyhow::Result;

use geolib::{
    format::{Format, FormatReader, FormatTransformer, GeoItemIterator, MetaMode},
    geojson::{JsonReader, JsonTransformer, NdjsonReader, NdjsonTransformer},
};

use crate::args::QuietLevel;
use crate::stream::{InputStream, OutputStream, StreamKind};

#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct IO {
    pub stream: StreamKind,
    pub format: Format,
}

impl IO {
    pub fn new(stream: StreamKind, format: Format) -> Self {
        Self { stream, format }
    }

    #[cfg(test)]
    pub fn with_str(str: String, format: Format) -> Self {
        Self::new(StreamKind::String(str), format)
    }

    #[cfg(test)]
    pub fn with_output_str(str: Arc<Mutex<String>>, format: Format) -> Self {
        Self::new(StreamKind::OutputString(str), format)
    }

    pub fn create_reader(&self, mode: MetaMode) -> impl GeoItemIterator {
        let reader = InputStream::from(self.stream.clone());

        match self.format {
            Format::Json => FormatReader::Json(JsonReader::new(reader, mode)),
            Format::Ndjson => FormatReader::Ndjson(NdjsonReader::new(reader, mode)),
            _ => todo!(),
        }
    }

    pub fn create_transformer<I>(
        &self,
        iter: I,
        mode: MetaMode,
    ) -> impl Iterator<Item = Result<impl AsRef<[u8]>>>
    where
        I: GeoItemIterator,
    {
        match self.format {
            Format::Json => FormatTransformer::Json(JsonTransformer::new(iter, mode)),
            Format::Ndjson => FormatTransformer::Ndjson(NdjsonTransformer::new(iter, mode)),
            _ => todo!(),
        }
    }

    pub fn create_writer(
        &self,
        byte_iter: impl Iterator<Item = Result<impl AsRef<[u8]>>>,
        quiet: QuietLevel,
    ) -> impl Iterator<Item = Result<()>> {
        let mut writer = OutputStream::from(self.stream.clone());

        byte_iter.map(move |byte_result| match byte_result {
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
        })
    }
}
