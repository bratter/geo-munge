use std::{
    borrow::Cow,
    io::{BufRead, Lines, Read},
    iter::Fuse,
    str::FromStr,
};

use anyhow::{anyhow, Result};
use geojson::{FeatureReader, GeoJson};

use crate::format::{ContentMode, GeoItem, GeoItemIterator, Properties};

const GEOM_FEAT_ONLY_MSG: &str = "Can only process Feature and Geometry types";

type FeatureResult = geojson::Result<geojson::Feature>;

/// Read and iterate over a GeoJson [`FeatureCollection`].
///
/// The reader is a permissive stream-based reader that assumes the incoming stream is a [`FeatureCollection`]. The
/// underlying GeoJson reader only requires a `'['` as an opener, then starts reading GeoJson features. This reader
/// therefore will not process single features, geometries or geometry collections. However echoing a single '[' at the
/// start and a ']' at the end will let it read a single feature.
pub struct JsonStreamReader {
    // NOTE: There doesn't seem to be an easy way of removing this dynamic dispatch, given this involves IO and a lot of
    // parsing, it shouldn't matter too much from a performance perspective.
    features: Box<dyn Iterator<Item = FeatureResult>>,
    mode: ContentMode,
}

impl JsonStreamReader {
    // TODO: Is this static requirement too much? I don't think so
    pub fn new<R: Read + 'static>(reader: R, mode: ContentMode) -> Self {
        let features = Box::new(FeatureReader::from_reader(reader).features().take_while(
            |result| match result {
                Err(geojson::Error::Io(_)) => false,
                _ => true,
            },
        ));

        JsonStreamReader { features, mode }
    }
}

impl Iterator for JsonStreamReader {
    type Item = Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.features.next();

        match self.mode {
            ContentMode::Full => next.map(|f| geoitem_from_geojson(GeoJson::Feature(f?), true)),
            ContentMode::Geometry => {
                next.map(|f| geoitem_from_geojson(GeoJson::Feature(f?), false))
            }
            ContentMode::Properties => next.map(|f| {
                let props = f?.properties.unwrap_or_default();
                Ok(GeoItem::props_only(props))
            }),
        }
    }
}

pub struct NdjsonReader<R> {
    reader: Lines<R>,
    mode: ContentMode,
}

impl<R: BufRead> NdjsonReader<R> {
    pub fn new(reader: R, mode: ContentMode) -> Self {
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
            (ContentMode::Full, Ok(f)) => geoitem_from_geojson(f, true),
            (ContentMode::Geometry, Ok(f)) => geoitem_from_geojson(f, false),
            (ContentMode::Properties, Ok(f)) => {
                let props = match f {
                    GeoJson::Feature(feat) => feat.properties.unwrap_or_default(),
                    GeoJson::Geometry(_) => Properties::default().into(),
                    _ => return Some(Err(anyhow!(GEOM_FEAT_ONLY_MSG))),
                };
                Ok(GeoItem::props_only(props))
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
    mode: ContentMode,
}

impl<I: GeoItemIterator> JsonTransformer<I> {
    pub fn new(iter: I, mode: ContentMode) -> Self {
        Self {
            state: State::EmitHeader,
            next_item: None,
            iter,
            mode,
        }
    }

    fn header_bytes(&self) -> &'static [u8] {
        match self.mode {
            ContentMode::Full | ContentMode::Geometry => {
                b"{\"type\":\"FeatureCollection\",\"features\":[\n"
            }
            ContentMode::Properties => b"[\n",
        }
    }

    fn footer_bytes(&self) -> &'static [u8] {
        match self.mode {
            ContentMode::Full | ContentMode::Geometry => b"\n]}",
            ContentMode::Properties => b"\n]",
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
    mode: ContentMode,
}

impl<I: GeoItemIterator> NdjsonTransformer<I> {
    pub fn new(iter: I, mode: ContentMode) -> Self {
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
            let props = match f.properties {
                Some(p) if preserve_meta => Some(p),
                _ => None,
            };
            Ok(GeoItem::new(geom, props))
        }
        GeoJson::Geometry(g) => Ok(geo::Geometry::try_from(g)?.into()),
        _ => Err(anyhow!(GEOM_FEAT_ONLY_MSG)),
    }
}

/// Make the appropriate json output given the input and the [`ContentMode`].
///
/// The [GeoJSON Feature spec](https://datatracker.ietf.org/doc/html/rfc7946#section-3.2) says that a properties key
/// must be present but can be an  Object or Null. For consistency in showing downstream that the conversion worked but
/// there was nothing there, we always serialize an empty object.
fn make_feature(item: GeoItem, mode: ContentMode) -> Vec<u8> {
    let vec = match mode {
        ContentMode::Full | ContentMode::Geometry => {
            let mut f = geojson::Feature::default();
            f.geometry = item.geom.as_ref().map(geojson::Geometry::from);
            if mode == ContentMode::Full {
                f.properties = Some(item.props.unwrap_or_default());
            }
            serde_json::to_vec(&f)
        }
        ContentMode::Properties => serde_json::to_vec(&item.props.unwrap_or_default()),
    };

    vec.expect("Serialize succeeds")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::format::Value;

    mod reader {
        use super::*;

        #[test]
        fn stream_emits_features_from_collection() {
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
                "properties": { "name": "point1", "value": 42 }
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
            let feature_reader = JsonStreamReader::new(fc.as_bytes(), ContentMode::Full);
            let features: Vec<_> = feature_reader.map(|result| result.unwrap()).collect();

            assert_eq!(features.len(), 2);
            assert!(matches!(
                features[0].geom,
                Some(geo::Geometry::Point(geo::Point(_)))
            ));

            // Check that properties are preserved
            let first_meta = features[0].props.as_ref().unwrap();
            assert_eq!(
                first_meta.get("name"),
                Some(&Value::String("point1".to_string()))
            );
            assert_eq!(first_meta.get("value"), Some(&Value::Number(42.into())));
        }

        #[test]
        fn stream_errors_if_not_feature_collection() {
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
            let features: Vec<Result<GeoItem>> =
                JsonStreamReader::new(f.as_bytes(), ContentMode::Full).collect();

            assert_eq!(features.len(), 1);
            assert!(matches!(features[0], Err(_)));
        }

        #[test]
        fn ndjson_emits_features_from_ndjson() {
            let ndjson = r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [1.1, 1.2]}, "properties": {"name": "point1", "value": 42}}
{"type": "Feature", "geometry": {"type": "Point", "coordinates": [2.1, 2.2]}, "properties": {}}"#;

            let reader = std::io::BufReader::new(ndjson.as_bytes());
            let ndjson_reader = NdjsonReader::new(reader, ContentMode::Full);
            let features: Vec<_> = ndjson_reader.map(|result| result.unwrap()).collect();

            assert_eq!(features.len(), 2);
            assert!(matches!(
                features[0].geom,
                Some(geo::Geometry::Point(geo::Point(_)))
            ));
            assert!(matches!(
                features[1].geom,
                Some(geo::Geometry::Point(geo::Point(_)))
            ));

            // Check that properties are preserved
            let first_meta = features[0].props.as_ref().unwrap();
            assert_eq!(
                first_meta.get("name"),
                Some(&Value::String("point1".to_string()))
            );
            assert_eq!(first_meta.get("value"), Some(&Value::Number(42.into())));
        }
    }

    mod transform {
        use super::*;

        fn pt(x: f64, y: f64) -> geo::Geometry {
            geo::Geometry::Point(geo::Point::new(x, y))
        }

        fn transform_test_data() -> Vec<Result<GeoItem>> {
            let mut m1 = Properties::new();
            m1.insert("name".to_string(), Value::String("point1".to_string()));
            m1.insert("value".to_string(), Value::Number(42.into()));
            m1.insert("active".to_string(), Value::Bool(true));
            vec![
                Ok(GeoItem::with_props(pt(1.1, 1.2), m1)),
                Ok(GeoItem::without_props(pt(2.1, 2.2))),
            ]
        }

        #[test]
        fn json_outputs_feature_collection() {
            let iter = transform_test_data();

            let json_transformer = JsonTransformer::new(iter.into_iter(), ContentMode::Full);
            let buf: Vec<u8> = json_transformer
                .map(|i| i.unwrap().to_vec())
                .flatten()
                .collect();
            let output = std::str::from_utf8(&buf).unwrap();

            // Parse the output as GeoJSON
            let geojson: geojson::GeoJson = output.parse().unwrap();
            if let geojson::GeoJson::FeatureCollection(fc) = geojson {
                assert_eq!(fc.features.len(), 2);

                // First feature should have properties
                let first_feature = &fc.features[0];
                assert!(matches!(
                    first_feature.geometry.as_ref().unwrap().value,
                    geojson::Value::Point(_)
                ));
                let props = first_feature.properties.as_ref().unwrap();
                assert_eq!(
                    props.get("name"),
                    Some(&serde_json::Value::String("point1".to_string()))
                );
                assert_eq!(props.get("value"), Some(&Value::Number(42.into())));
                assert_eq!(props.get("active"), Some(&serde_json::Value::Bool(true)));

                // Second feature should have null properties
                let second_feature = &fc.features[1];
                assert!(matches!(
                    second_feature.geometry.as_ref().unwrap().value,
                    geojson::Value::Point(_)
                ));
                assert!(second_feature.properties.as_ref().unwrap().is_empty());
            } else {
                panic!("Expected FeatureCollection");
            }
        }

        #[test]
        fn json_outputs_properties_only() {
            let iter = transform_test_data();

            let json_transformer = JsonTransformer::new(iter.into_iter(), ContentMode::Properties);
            let buf: Vec<u8> = json_transformer
                .map(|i| i.unwrap().to_vec())
                .flatten()
                .collect();
            let output = std::str::from_utf8(&buf).unwrap();

            // Should be a JSON array of property objects
            let json: serde_json::Value = serde_json::from_str(output).unwrap();
            if let serde_json::Value::Array(arr) = json {
                assert_eq!(arr.len(), 2);

                // First object has properties
                assert_eq!(arr[0]["name"], "point1");
                assert_eq!(arr[0]["value"], 42);
                assert_eq!(arr[0]["active"], true);

                // Second object is empty (without_props)
                assert_eq!(arr[1], serde_json::Value::Object(serde_json::Map::new()));
            } else {
                panic!("Expected JSON array");
            }
        }

        #[test]
        fn ndjson_outputs_newline_delimited() {
            let iter = transform_test_data();

            let ndjson_transformer = NdjsonTransformer::new(iter.into_iter(), ContentMode::Full);
            let buf: Vec<u8> = ndjson_transformer
                .map(|i| i.unwrap().to_vec())
                .flatten()
                .collect();
            let output = std::str::from_utf8(&buf).unwrap();

            let lines: Vec<&str> = output.trim().split('\n').collect();
            assert_eq!(lines.len(), 2);

            // First line should have properties
            assert!(lines[0].contains("\"type\":\"Feature\""));
            assert!(
                lines[0].contains("\"geometry\":{\"type\":\"Point\",\"coordinates\":[1.1,1.2]}")
            );
            assert!(lines[0]
                .contains("\"properties\":{\"active\":true,\"name\":\"point1\",\"value\":42}"));

            // Second line should have null properties
            assert!(lines[1].contains("\"type\":\"Feature\""));
            assert!(
                lines[1].contains("\"geometry\":{\"type\":\"Point\",\"coordinates\":[2.1,2.2]}")
            );
            assert!(lines[1].contains("\"properties\":{}"));
        }

        #[test]
        fn ndjson_outputs_properties_only() {
            let iter = transform_test_data();

            let ndjson_transformer =
                NdjsonTransformer::new(iter.into_iter(), ContentMode::Properties);
            let buf: Vec<u8> = ndjson_transformer
                .map(|i| i.unwrap().to_vec())
                .flatten()
                .collect();
            let output = std::str::from_utf8(&buf).unwrap();

            let lines: Vec<&str> = output.trim().split('\n').collect();
            assert_eq!(lines.len(), 2);

            // First line has properties
            let first_json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
            assert_eq!(first_json["name"], "point1");
            assert_eq!(first_json["value"], 42);
            assert_eq!(first_json["active"], true);

            // Second line is empty object (without_props)
            let second_json: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
            assert_eq!(
                second_json,
                serde_json::Value::Object(serde_json::Map::new())
            );
        }
    }
}
