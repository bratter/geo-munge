use std::io::{stderr, Write};

use anyhow::{bail, Result};

use geolib::{
    csv::{CsvReader, CsvSettings, CsvTransformer},
    format::{Format, FormatReader, FormatTransformer, GeoItemIterator, MetaMode},
    geojson::{JsonReader, JsonTransformer, NdjsonReader, NdjsonTransformer},
    shp::{ShapefileReader, ShapefileTransformer},
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

    pub fn create_reader(&self, mode: MetaMode) -> Result<impl GeoItemIterator> {
        // Shapefiles require special management so we process them first
        // This just means we don't have to embed input stream generation in the match below
        if self.format == Format::Shp {
            if let StreamKind::File(f) = &self.stream {
                return Ok(FormatReader::Shp(ShapefileReader::new(f, mode)?));
            } else {
                bail!("Shapefiles can only be read from file input, not stdin");
            }
        }
        let reader = InputStream::from(self.stream.clone());

        match self.format {
            Format::Json => Ok(FormatReader::Json(JsonReader::new(reader, mode))),
            Format::Ndjson => Ok(FormatReader::Ndjson(NdjsonReader::new(reader, mode))),
            Format::Shp => unreachable!(),
            Format::Csv => Ok(FormatReader::Csv(CsvReader::new(
                reader,
                mode,
                CsvSettings::default(),
            )?)),
            _ => todo!(),
        }
    }

    pub fn create_transformer<I>(
        &self,
        iter: I,
        mode: MetaMode,
        csv_settings: CsvSettings,
    ) -> Result<impl Iterator<Item = Result<impl AsRef<[u8]>>>>
    where
        I: GeoItemIterator,
    {
        let ft = match self.format {
            Format::Json => FormatTransformer::Json(JsonTransformer::new(iter, mode)),
            Format::Ndjson => FormatTransformer::Ndjson(NdjsonTransformer::new(iter, mode)),
            Format::Shp => FormatTransformer::Shp(ShapefileTransformer::new(iter, mode)?),
            Format::Csv => FormatTransformer::Csv(CsvTransformer::new(iter, mode, csv_settings)),
            _ => todo!(),
        };
        Ok(ft)
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
