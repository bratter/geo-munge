use std::io::{stderr, Write};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use anyhow::Result;

use geolib::{
    csv::{CsvReader, CsvSettings, CsvTransformer},
    format::{ContentMode, FormatReader, FormatTransformer, GeoItemIterator},
    geojson::{JsonStreamReader, JsonTransformer, NdjsonReader, NdjsonTransformer},
    shp::ShapefileReader,
};

use crate::args::QuietLevel;
use crate::stream::{InputStream, OutputStream, StreamKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamableFormat {
    JsonStream,
    Ndjson,
    Csv,
}

impl std::fmt::Display for StreamableFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::JsonStream => "json",
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

#[derive(Debug, Clone)]
pub struct OutputSpec {
    pub format: StreamableFormat,
    pub stream: StreamKind,
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
        match self {
            InputSpec::Streamable { format, stream } => {
                let reader = InputStream::try_new(stream.clone())?;
                match format {
                    StreamableFormat::JsonStream => {
                        Ok(FormatReader::Json(JsonStreamReader::new(reader, mode)))
                    }
                    StreamableFormat::Ndjson => {
                        Ok(FormatReader::Ndjson(NdjsonReader::new(reader, mode)))
                    }
                    StreamableFormat::Csv => Ok(FormatReader::Csv(CsvReader::new(
                        reader,
                        mode,
                        CsvSettings::default(),
                    )?)),
                }
            }
            InputSpec::FileOnly { format, path } => match format {
                FileOnlyFormat::Shp => Ok(FormatReader::Shp(ShapefileReader::try_new(path, mode)?)),
                FileOnlyFormat::Kml => todo!("KML reader implementation"),
                FileOnlyFormat::Kmz => todo!("KMZ reader implementation"),
            },
        }
    }
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
            StreamableFormat::JsonStream => {
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
