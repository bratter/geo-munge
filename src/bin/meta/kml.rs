use std::{
    collections::{HashMap, HashSet},
    iter::empty,
    path::PathBuf,
};

use csv::WriterBuilder;
use geo_munge::{
    error::FiberError,
    kml::{read_kml, Kml, KmlItemRef},
};

use crate::{DataOpts, Meta, MetaResult};

pub struct KmlMeta {
    path: PathBuf,
}

impl KmlMeta {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Meta for KmlMeta {
    // TODO: It seems to be squashing cdata in the upront description. Why?
    fn headers(&self) -> MetaResult {
        let kml = read_kml(&self.path)?;

        let elements = match kml {
            kml::Kml::KmlDocument(d) => {
                // I think that a Document is the only valid child here, but
                // even if not header only uses Document elements
                d.elements.into_iter().find_map(|e| match e {
                    kml::Kml::Document { attrs: _, elements } => Some(elements),
                    _ => None,
                })
            }
            kml::Kml::Document { attrs: _, elements } => Some(elements),
            _ => None,
        };

        if let Some(elements) = elements {
            for el in elements {
                match el {
                    kml::Kml::Element(el) => {
                        if let Some(content) = el.content {
                            println!("{}: {}", el.name, content)
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    /// Limited support of fields for KML - only reports fields from Placemark
    /// objects.
    fn fields(&self, _: bool) -> MetaResult {
        let kml = Kml::from_path(&self.path)?;

        let fields = make_fields(&kml, None, None);

        for field in fields {
            println!("{field}");
        }

        Ok(())
    }

    fn count(&self) -> MetaResult {
        let kml = Kml::from_path(&self.path)?;
        let count = &kml.into_iter().count();

        println!("{count}");

        Ok(())
    }

    fn data(&self, opts: DataOpts) -> MetaResult {
        let delimiter = opts.delimiter.as_bytes();
        if delimiter.len() != 1 {
            return Err(Box::new(FiberError::Arg("Invalid delimiter provided")));
        }
        let delimiter = delimiter[0];

        let mut writer = WriterBuilder::new()
            .delimiter(delimiter)
            .from_writer(std::io::stdout());

        let kml = Kml::from_path(&self.path)?;
        let fields: Vec<_> = make_fields(&kml, Some(opts.start), opts.length)
            .into_iter()
            .collect();

        // Write out the header
        if opts.headers {
            if opts.index {
                writer.write_field("index")?;
            }
            for field in &fields {
                writer.write_field(field)?;
            }
            writer.write_record(empty::<&str>())?;
        }

        for (i, item) in kml
            .iter()
            .skip(opts.start)
            .take(opts.length.unwrap_or(usize::MAX))
            .enumerate()
        {
            if opts.index {
                writer.write_field(i.to_string())?;
            }
            match item {
                KmlItemRef::Placemark(p) => {
                    // Put children into a hash map rather than finding each time
                    // Unclear that this will be more efficient
                    let data: HashMap<&String, &Option<String>> =
                        HashMap::from_iter(p.children.iter().map(|e| (&e.name, &e.content)));

                    writer.write_record(fields.iter().map(|f| {
                        match f.as_str() {
                            "name" => p.name.to_owned().unwrap_or_default(),
                            "description" => p.description.to_owned().unwrap_or_default(),
                            _ => data
                                .get(f)
                                .and_then(|d| d.as_ref())
                                .map(|s| s.to_owned())
                                .unwrap_or_default(),
                        }
                    }))?;
                }
                // No meta for shapes
                _ => {
                    writer.write_record(fields.iter().map(|_| ""))?;
                }
            }
        }

        Ok(())
    }
}

/// Limited support of fields for KML - only reports fields from Placemark
/// objects.
// TODO: Does/should this have the same semantics as the proximity version?
fn make_fields(kml: &Kml, skip: Option<usize>, n: Option<usize>) -> HashSet<String> {
    // Placemarks can always have name and desc, so add them even if not guaranteed
    let mut fields = HashSet::from(["name".to_string(), "description".to_string()]);

    for d in kml
        .iter()
        .skip(skip.unwrap_or(0))
        .take(n.unwrap_or(usize::MAX))
    {
        if let KmlItemRef::Placemark(p) = d {
            for child in &p.children {
                fields.insert(child.name.to_string());
            }
        }
    }

    fields
}
