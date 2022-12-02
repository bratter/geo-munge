use std::{iter::FlatMap, path::PathBuf};

use geo_munge::error::FiberError;
use kml::{
    types::{LineString, LinearRing, Location, MultiGeometry, Placemark, Point, Polygon},
    Kml, KmlReader,
};

use crate::Meta;

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
    fn headers(&self) -> crate::MetaResult {
        let kml = self.kml()?;

        // TODO: Probably need an iterator to walk through the kml, can be recursive and should stop at Placemarks or shapes to read
        match kml {
            Kml::KmlDocument(d) => {
                println!("KmlDocument");
            }
            Kml::Document { attrs, elements } => {
                println!("Attrs\n {:?}", attrs);
                println!("El\n {:?}", elements);
            }
            _ => {}
        }

        Ok(())
    }

    fn fields(&self, show_types: bool) -> crate::MetaResult {
        let kml = self.kml()?;

        for d in KmlIterator::new(&kml) {
            println!("{:?}", d);
        }

        Ok(())
    }

    fn count(&self) -> crate::MetaResult {
        todo!()
    }

    fn data(&self, opts: crate::DataOpts) -> crate::MetaResult {
        todo!()
    }
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
