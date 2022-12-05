use std::{
    collections::{HashMap, HashSet},
    iter::{empty, FlatMap},
    path::PathBuf,
};

use csv::WriterBuilder;
use geo_munge::error::FiberError;
use kml::{
    types::{LineString, LinearRing, Location, MultiGeometry, Placemark, Point, Polygon},
    Kml, KmlReader,
};

use crate::{DataOpts, Meta, MetaResult};

pub struct KmlMeta {
    path: PathBuf,
}

impl KmlMeta {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn kml(&self) -> Result<Kml, FiberError> {
        let ext = self.path.extension().unwrap();

        if ext == "kml" {
            KmlReader::<_, f64>::from_path(self.path.clone())
                .map_err(|_| FiberError::IO("Cannot read KML file"))
                .and_then(|mut r| {
                    r.read()
                        .map_err(|_| FiberError::Parse(0, "Couldn't parse KML file"))
                })
        } else if ext == "kmz" {
            KmlReader::<_, f64>::from_kmz_path(self.path.clone())
                .map_err(|_| FiberError::IO("Cannot read KMZ file"))
                .and_then(|mut r| {
                    r.read()
                        .map_err(|_| FiberError::Parse(0, "Couldn't parse KMZ file"))
                })
        } else {
            Err(FiberError::IO("Invalid extension"))
        }
    }
}

impl Meta for KmlMeta {
    // TODO: It seems to be squashing cdata in the upront description. Why?
    fn headers(&self) -> MetaResult {
        let kml = self.kml()?;

        let elements = match kml {
            Kml::KmlDocument(d) => {
                // I think that a Document is the only valid child here, but
                // even if not header only uses Document elements
                d.elements.into_iter().find_map(|e| match e {
                    Kml::Document { attrs: _, elements } => Some(elements),
                    _ => None,
                })
            }
            Kml::Document { attrs: _, elements } => Some(elements),
            _ => None,
        };

        if let Some(elements) = elements {
            for el in elements {
                match el {
                    Kml::Element(el) => {
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
        let kml = self.kml()?;

        let fields = make_fields(&kml, None, None);

        for field in fields {
            println!("{field}");
        }

        Ok(())
    }

    fn count(&self) -> MetaResult {
        let kml = self.kml()?;
        let count = KmlIterator::new(&kml).count();

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

        let kml = self.kml()?;
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

        for (i, item) in KmlIterator::new(&kml)
            .skip(opts.start)
            .take(opts.length.unwrap_or(usize::MAX))
            .enumerate()
        {
            if opts.index {
                writer.write_field(i.to_string())?;
            }
            match item {
                KmlItem::Placemark(p) => {
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
fn make_fields(kml: &Kml, skip: Option<usize>, n: Option<usize>) -> HashSet<String> {
    // Placemarks can always have name and desc,so add them even if not guaranteed
    let mut fields = HashSet::from(["name".to_string(), "description".to_string()]);

    for d in KmlIterator::new(&kml)
        .skip(skip.unwrap_or(0))
        .take(n.unwrap_or(usize::MAX))
    {
        if let KmlItem::Placemark(p) = d {
            for child in &p.children {
                fields.insert(child.name.to_string());
            }
        }
    }

    fields
}

type KmlFlatMap<'a> = FlatMap<std::slice::Iter<'a, Kml>, KmlIterator<'a>, fn(&Kml) -> KmlIterator>;

/// Holds a subset of Kml members that might be emitted by the iterator
#[derive(Debug)]
enum KmlItem<'a> {
    MultiGeometry(&'a MultiGeometry),
    LinearRing(&'a LinearRing),
    LineString(&'a LineString),
    Location(&'a Location),
    Placemark(&'a Placemark),
    Point(&'a Point),
    Polygon(&'a Polygon),
}

enum KmlIterState<T> {
    Init,
    Done,
    Iter(T),
}
struct KmlIterator<'a> {
    kml: &'a Kml,
    state: KmlIterState<Box<KmlFlatMap<'a>>>,
}

impl<'a> KmlIterator<'a> {
    fn new(kml: &'a Kml) -> Self {
        Self {
            kml,
            state: KmlIterState::Init,
        }
    }
}

impl<'a> Iterator for KmlIterator<'a> {
    type Item = KmlItem<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.state {
            // When we're done, shortcut None
            KmlIterState::Done => None,
            // Loop through all the items, including nested iterators
            KmlIterState::Iter(d) => {
                let next = d.next();
                if next.is_none() {
                    self.state = KmlIterState::Done;
                }
                next
            }
            // Initialize the kml first time
            KmlIterState::Init => {
                // Pre-emptively set state to Done to avoid having to do it everywhere else
                // This will be reset to Iter in the recursive structures
                self.state = KmlIterState::Done;

                match &self.kml {
                    // Nested structures for recursion
                    // Set the state then immediately recurse to emit the first item
                    Kml::KmlDocument(ref d) => {
                        self.state = KmlIterState::Iter(Box::new(
                            d.elements.iter().flat_map(|e| KmlIterator::new(e)),
                        ));
                        self.next()
                    }
                    Kml::Document { attrs: _, elements } | Kml::Folder { attrs: _, elements } => {
                        self.state = KmlIterState::Iter(Box::new(
                            elements.iter().flat_map(|e| KmlIterator::new(e)),
                        ));
                        self.next()
                    }
                    // Shapes for returning, transition to Done performed above, so emit immediately
                    // TODO: How to handle multi geometry? Just emit as one, or break up?
                    // TODO: Does this capture the Data types?
                    Kml::MultiGeometry(d) => Some(KmlItem::MultiGeometry(d)),
                    Kml::LinearRing(d) => Some(KmlItem::LinearRing(d)),
                    Kml::LineString(d) => Some(KmlItem::LineString(d)),
                    Kml::Location(d) => Some(KmlItem::Location(d)),
                    Kml::Point(d) => Some(KmlItem::Point(d)),
                    Kml::Placemark(d) => Some(KmlItem::Placemark(d)),
                    Kml::Polygon(d) => Some(KmlItem::Polygon(d)),
                    // Ignore all else
                    _ => None,
                }
            }
        }
    }
}
