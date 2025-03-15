use std::{fs::File, io::BufReader, iter::once, path::PathBuf};

use csv::WriterBuilder;
use geo_munge::{error::Error, shp::convert_dbase_field_opt};
use shapefile::Reader;

use crate::{DataOpts, Meta, MetaResult};

pub struct ShapefileMeta {
    path: PathBuf,
}

impl ShapefileMeta {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn reader(&self) -> Result<Reader<BufReader<File>, BufReader<File>>, Error> {
        // Load the shapefile, exiting with an error if the file cannot read
        // Then build the quadtree
        shapefile::Reader::from_path(&self.path)
            .map_err(|_| Error::CannotReadFile(self.path.clone()))
    }

    fn field_iter(&self, show_types: bool) -> Result<impl Iterator<Item = String>, Error> {
        let (_, record) = self
            .reader()?
            .iter_shapes_and_records()
            .next()
            .ok_or(Error::UnexpectedEndOfInput)
            .and_then(|r| r.map_err(|err| Error::ShapefileParseError(err)))?;

        Ok(record.into_iter().map(move |(name, value)| {
            if show_types {
                format!("{} [{}]", name, value.field_type())
            } else {
                name
            }
        }))
    }
}

impl Meta for ShapefileMeta {
    fn headers(&self) -> MetaResult {
        let reader = self.reader()?;
        let header = reader.header();

        let bbox_min = header.bbox.min;
        let bbox_max = header.bbox.max;

        println!("Shapefile version: {}", header.version);
        println!("Shape type: {}", header.shape_type);
        println!(
            "Bounding box: [{}, {}, {}, {}]",
            bbox_min.x, bbox_min.y, bbox_max.x, bbox_max.y
        );
        println!("File length: {}", header.file_length);

        Ok(())
    }

    fn fields(&self, show_types: bool) -> MetaResult {
        for v in self.field_iter(show_types)? {
            println!("{}", v);
        }

        Ok(())
    }

    fn count(&self) -> MetaResult {
        let count = self.reader()?.iter_shapes_and_records().count();

        println!("{count}");

        Ok(())
    }

    fn data(&self, opts: DataOpts) -> MetaResult {
        let delimiter = opts.delimiter.as_bytes();
        if delimiter.len() != 1 {
            return Err(Box::new(Error::InvalidDelimiter));
        }
        let delimiter = delimiter[0];

        let mut reader = self.reader()?;
        let mut writer = WriterBuilder::new()
            .delimiter(delimiter)
            .from_writer(std::io::stdout());

        // Need to get the headers in order whether or not they are being printed
        // Underlying hashmap knocks things out of order so need to iterate in
        // order to preserve consistency
        let field_iter: Vec<_> = self.field_iter(false)?.collect();

        if opts.headers {
            // Add an index field at the front if the option is set
            if opts.index {
                writer.write_record(once(&"index".to_string()).chain(&field_iter))?;
            } else {
                writer.write_record(&field_iter)?;
            }
        }

        let n = opts.length.unwrap_or(usize::MAX);
        for (i, record) in reader
            .iter_shapes_and_records()
            .enumerate()
            .skip(opts.start)
            .take(n)
        {
            match record {
                Ok((_, record)) => {
                    // Use the fields vector to iterate in-order
                    let record_iter = field_iter
                        .iter()
                        .map(|f| convert_dbase_field_opt(record.get(f)));

                    // Insert the stringified index as the first item in the
                    // Iterator if the option is set
                    let record_iter: Box<dyn Iterator<Item = String>> = if opts.index {
                        Box::new(once(i.to_string()).chain(record_iter))
                    } else {
                        Box::new(record_iter)
                    };

                    if writer.write_record(record_iter).is_err() {
                        eprintln!("failed to write output for record at index {i}");
                    }
                }
                Err(_) => eprintln!("cannot read record at index {i}"),
            }
        }

        Ok(())
    }
}
