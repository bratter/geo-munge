use std::io::{stderr, Write};

use anyhow::{bail, Result};

use geolib::{
    csv::{CsvReader, CsvSettings, CsvTransformer},
    format::{ContentMode, Format, FormatReader, FormatTransformer, GeoItemIterator},
    geojson::{JsonStreamReader, JsonTransformer, NdjsonReader, NdjsonTransformer},
    shp::ShapefileReader,
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

    pub fn create_reader(&self, mode: ContentMode) -> Result<impl GeoItemIterator> {
        // Shapefiles require special management so we process them first
        // This just means we don't have to embed input stream generation in the match below
        if self.format == Format::Shp {
            if let StreamKind::File(f) = &self.stream {
                return Ok(FormatReader::Shp(ShapefileReader::try_new(f, mode)?));
            } else {
                bail!("Shapefiles can only be read from file input, not stdin");
            }
        }
        let reader = InputStream::from(self.stream.clone());

        match self.format {
            Format::JsonStream => Ok(FormatReader::Json(JsonStreamReader::new(reader, mode))),
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
        mode: ContentMode,
        csv_settings: CsvSettings,
    ) -> Result<impl Iterator<Item = Result<impl AsRef<[u8]>>>>
    where
        I: GeoItemIterator,
    {
        let ft = match self.format {
            Format::JsonStream => FormatTransformer::Json(JsonTransformer::new(iter, mode)),
            Format::Ndjson => FormatTransformer::Ndjson(NdjsonTransformer::new(iter, mode)),
            Format::Csv => FormatTransformer::Csv(CsvTransformer::new(iter, mode, csv_settings)),
            _ => bail!("Format type not available as an output"),
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
