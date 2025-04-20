use std::{
    borrow::Cow,
    fs::read_to_string,
    io::{BufRead, Lines, Read},
    iter::{once, Fuse},
    path::PathBuf,
    str::FromStr,
};

use anyhow::{anyhow, Result};
use geojson::{FeatureReader, GeoJson};
use quadtree::{Geometry, ToRadians};
use serde_json::Map;

use crate::format::{GeoItem, GeoItemIterator, Meta, Mode};

pub fn read_geojson(path: &PathBuf) -> Result<GeoJson, crate::error::Error> {
    read_to_string(&path)
        .map_err(|_| crate::error::Error::CannotReadFile(path.clone()))?
        .parse::<GeoJson>()
        .map_err(|_| crate::error::Error::CannotParseFile(path.clone()))
}

/// Convert a GeoJson geometry into the appropriate quadtree-enabled type. Outputs an iterator as
/// it flattens multi-geometries into their single geometry counterparts.
// TODO: Can we replace most of this? If we only accept wkb in the qt, we can do the radians conversion there
pub fn convert_geom(
    input: &geojson::Geometry,
) -> Box<dyn Iterator<Item = Result<Geometry<f64>, geojson::Error>>> {
    match &input.value {
        d @ geojson::Value::Point(_) => Box::new(once(d.try_into().map(|mut p: geo::Point| {
            p.to_radians_in_place();
            Geometry::Point(p)
        }))),
        d @ geojson::Value::Polygon(_) => {
            Box::new(once(d.try_into().map(|mut p: geo::Polygon| {
                p.to_radians_in_place();
                Geometry::Polygon(p)
            })))
        }
        d @ geojson::Value::LineString(_) => {
            Box::new(once(d.try_into().map(|mut l: geo::LineString| {
                l.to_radians_in_place();
                Geometry::LineString(l)
            })))
        }
        d @ geojson::Value::MultiPoint(_) => match geo::MultiPoint::try_from(d) {
            Ok(mp) => Box::new(mp.into_iter().map(|mut p| {
                p.to_radians_in_place();
                Ok(Geometry::Point(p))
            })),
            Err(err) => Box::new(once(Err(err))),
        },
        d @ geojson::Value::MultiPolygon(_) => match geo::MultiPolygon::try_from(d) {
            Ok(mp) => Box::new(mp.into_iter().map(|mut p| {
                p.to_radians_in_place();
                Ok(Geometry::Polygon(p))
            })),
            Err(err) => Box::new(once(Err(err))),
        },
        d @ geojson::Value::MultiLineString(_) => match geo::MultiLineString::try_from(d) {
            Ok(mls) => Box::new(mls.into_iter().map(|mut l| {
                l.to_radians_in_place();
                Ok(Geometry::LineString(l))
            })),
            Err(err) => Box::new(once(Err(err))),
        },
        geojson::Value::GeometryCollection(_) => {
            Box::new(once(Err(geojson::Error::ExpectedType {
                expected: "not GeometryCollection".to_string(),
                actual: "GeometryCollection".to_string(),
            })))
        }
    }
}

const GEOM_FEAT_ONLY_MSG: &'static str = "Can only process Feature and Geometry types";

type FeatureResult = geojson::Result<geojson::Feature>;

/// Read and iterate over a GeoJson [`FeatureCollection`].
///
/// The reader is a permissive stream-based reader that assumes the incoming stream is a [`FeatureCollection`]. The
/// underlying GeoJson reader only requires a `'['` as an opener, then starts reading GeoJson features. This reader
/// therefore will not process single features, geometries or geometry collections. However echoing a single '[' at the
/// start and a ']' at the end will let it read a single feature.
pub struct JsonReader {
    // NOTE: There doesn't seem to be an easy way of removing this dynamic dispatch, given this involves IO and a lot of
    // parsing, it shouldn't matter too much from a performance perspective.
    features: Box<dyn Iterator<Item = FeatureResult>>,
    mode: Mode,
}

impl JsonReader {
    // TODO: Is this static requirement too much? I don't think so
    pub fn new<R: Read + 'static>(reader: R, mode: Mode) -> JsonReader {
        let features = Box::new(FeatureReader::from_reader(reader).features().take_while(
            |result| match result {
                Err(geojson::Error::Io(_)) => false,
                _ => true,
            },
        ));

        JsonReader { features, mode }
    }
}

impl Iterator for JsonReader {
    type Item = Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.features.next();

        match self.mode {
            Mode::Full => next.map(|f| geoitem_from_geojson(GeoJson::Feature(f?), true)),
            Mode::Shapes => next.map(|f| geoitem_from_geojson(GeoJson::Feature(f?), false)),
            Mode::Meta => next.map(|f| {
                Ok(GeoItem::meta_only(Meta::from(
                    f?.properties.unwrap_or_default(),
                )))
            }),
        }
    }
}

pub struct NdjsonReader<R> {
    reader: Lines<R>,
    mode: Mode,
}

impl<R: BufRead> NdjsonReader<R> {
    pub fn new(reader: R, mode: Mode) -> Self {
        Self {
            reader: reader.lines(),
            mode,
        }
    }

    fn get_geojson_line<E>(line: Result<String, E>) -> Result<GeoJson>
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Ok(GeoJson::from_str(&line?)?)
    }
}

impl<R: BufRead> Iterator for NdjsonReader<R> {
    type Item = Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.reader.next()?;

        // Ignore empty lines - these are not errors or None
        // TODO: Is this the best way to skip?
        if let Ok(ref s) = next {
            if s.is_empty() {
                return self.next();
            }
        }

        let geojson = Self::get_geojson_line(next);
        let geoitem = match (self.mode, geojson) {
            (Mode::Full, Ok(f)) => geoitem_from_geojson(f, true),
            (Mode::Shapes, Ok(f)) => geoitem_from_geojson(f, false),
            (Mode::Meta, Ok(f)) => {
                let meta = match f {
                    GeoJson::Feature(feat) => feat.properties.unwrap_or_default().into(),
                    GeoJson::Geometry(_) => Map::default().into(),
                    _ => return Some(Err(anyhow!(GEOM_FEAT_ONLY_MSG))),
                };
                Ok(GeoItem::meta_only(meta))
            }
            (_, Err(err)) => Err(err),
        };

        Some(geoitem)
    }
}

enum State {
    EmitHeader,
    EmitItem,
    EmitComma,
    EmitFooter,
    Done,
}

pub struct JsonTransformer<I: GeoItemIterator>
where
    I: Iterator,
{
    state: State,
    next_item: Option<I::Item>,
    iter: I,
    mode: Mode,
}

impl<I: GeoItemIterator> JsonTransformer<I> {
    pub fn new(iter: I, mode: Mode) -> Self {
        Self {
            state: State::EmitHeader,
            next_item: None,
            iter,
            mode,
        }
    }

    fn header_bytes(&self) -> &'static [u8] {
        match self.mode {
            Mode::Full | Mode::Shapes => b"{type:\"FeatureCollection\",features:[\n",
            Mode::Meta => b"[\n",
        }
    }

    fn footer_bytes(&self) -> &'static [u8] {
        match self.mode {
            Mode::Full | Mode::Shapes => b"\n]}",
            Mode::Meta => b"\n]",
        }
    }
}

impl<I: GeoItemIterator> Iterator for JsonTransformer<I> {
    type Item = Result<Cow<'static, [u8]>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.state {
            State::EmitHeader => {
                self.next_item = self.iter.next();
                self.state = State::EmitItem;
                Some(Ok(Cow::Borrowed(self.header_bytes())))
            }

            State::EmitItem => match self.next_item.take() {
                Some(Ok(item)) => {
                    // When we emit a valid item, then we test to see if we need to inject a comma to separate the
                    // emitted features
                    // NOTE: If make_feature becomes fallible, then will need to handle differently
                    self.state = State::EmitComma;
                    Some(Ok(Cow::Owned(make_feature(item, self.mode))))
                }
                Some(Err(e)) => {
                    // When the next item is an error, emit the error and immediately test the next item, without
                    // inserting a comma
                    self.next_item = self.iter.next();
                    Some(Err(e))
                }
                None => {
                    self.state = State::EmitFooter;
                    self.next()
                }
            },

            State::EmitComma => match self.iter.next() {
                Some(Ok(item)) => {
                    self.next_item = Some(Ok(item));
                    self.state = State::EmitItem;
                    Some(Ok(Cow::Borrowed(b",\n")))
                }
                Some(Err(e)) => Some(Err(e)),
                None => {
                    self.state = State::EmitFooter;
                    self.next()
                }
            },

            State::EmitFooter => {
                self.state = State::Done;
                Some(Ok(Cow::Borrowed(self.footer_bytes())))
            }

            State::Done => None,
        }
    }
}

pub struct NdjsonTransformer<I: GeoItemIterator> {
    iter: Fuse<I>,
    mode: Mode,
}

impl<I: GeoItemIterator> NdjsonTransformer<I> {
    pub fn new(iter: I, mode: Mode) -> Self {
        Self {
            iter: iter.fuse(),
            mode,
        }
    }
}

impl<I: GeoItemIterator> Iterator for NdjsonTransformer<I> {
    type Item = Result<Cow<'static, [u8]>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(v) = self.iter.next() {
            let mut f = make_feature(v.unwrap(), self.mode);
            f.push(b'\n');
            Some(Ok(Cow::Owned(f)))
        } else {
            None
        }
    }
}

fn geoitem_from_geojson(geojson: GeoJson, preserve_meta: bool) -> Result<GeoItem> {
    match geojson {
        GeoJson::Feature(f) => {
            let geom = geo::Geometry::try_from(f.geometry.ok_or(anyhow!("Invalid geometry"))?)?;
            let meta = match f.properties {
                Some(p) if preserve_meta => Some(Meta::from(p)),
                _ => None,
            };
            Ok(GeoItem::new(geom, meta))
        }
        GeoJson::Geometry(g) => Ok(geo::Geometry::try_from(g)?.into()),
        _ => Err(anyhow!(GEOM_FEAT_ONLY_MSG)),
    }
}

/// TODO: This should be done with From on a parent object with the properties most likely
fn make_feature(item: GeoItem, mode: Mode) -> Vec<u8> {
    let vec = match mode {
        Mode::Full | Mode::Shapes => {
            let mut f = geojson::Feature::default();
            f.geometry = item.geom.as_ref().map(geojson::Geometry::from);
            // TODO: This processing will need to be much better
            if mode == Mode::Full {
                f.properties = match item.meta {
                    Some(Meta::Json(props)) => Some(props),
                    None => None,
                }
            }
            serde_json::to_vec(&f)
        }
        Mode::Meta => {
            // TODO: This processing will need to be much better
            match &item.meta {
                Some(Meta::Json(props)) => serde_json::to_vec(&props),
                None => serde_json::to_vec("{}"),
            }
        }
    };

    vec.expect("Serialize succeeds")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_features_from_collection() {
        let fc = r#"
          {
            type: "FeatureCollection",
            features: [
              {
                "type": "Feature",
                "geometry": {
                  "type": "Point",
                  "coordinates": [1.1, 1.2]
                },
                "properties": { }
              },
              {
                "type": "Feature",
                "geometry": {
                  "type": "Point",
                  "coordinates": [2.1, 2.2]
                },
                "properties": { }
              }
            ]
          }
        "#;
        let feature_reader = JsonReader::new(fc.as_bytes(), Mode::Full);
        let features: Vec<GeoItem> = feature_reader
            .map(|result| result.expect("a valid feature"))
            .collect();

        assert_eq!(features.len(), 2);
        assert!(matches!(
            features[0].geom,
            Some(geo::Geometry::Point(geo::Point(_)))
        ));
    }

    #[test]
    fn error_if_not_feature_collection() {
        let f = r#"
          {
            "type": "Feature",
            "geometry": {
              "type": "Point",
              "coordinates": [1.1, 1.2]
            },
            "properties": { }
          },
        "#;
        let features: Vec<Result<GeoItem>> = JsonReader::new(f.as_bytes(), Mode::Full).collect();
        println!("{features:?}");

        assert_eq!(features.len(), 1);
        assert!(matches!(features[0], Err(_)));
    }
}
