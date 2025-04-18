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
use serde_json::{Map, Value};

use crate::format::{FormatReader, FormatWriter, GeoItem, GeoItemIterator, Meta, Mode};

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

/// Read and iterate over a GeoJson [`FeatureCollection`].
///
/// The reader is a permissive stream-based reader that assumes the incoming stream is a [`FeatureCollection`]. The
/// underlying GeoJson reader only requires a `'['` as an opener, then starts reading GeoJson features. This reader
/// therefore will not process single features, geometries or geometry collections. However echoing a single '[' at the
/// start and a ']' at the end will let it read a single feature.
pub struct JsonReader<R> {
    reader: FeatureReader<R>,
}

impl<R: Read> JsonReader<R> {
    pub fn try_new(reader: R) -> Result<Self> {
        Ok(Self {
            reader: FeatureReader::from_reader(reader),
        })
    }

    // TODO: Consider modifying the take while to capture other error types
    // For instance this currently produces an error on an empty feature collection
    fn features(self) -> impl Iterator<Item = geojson::Result<geojson::Feature>> {
        self.reader.features().take_while(|r| match r {
            Err(geojson::Error::Io(_)) => false,
            _ => true,
        })
    }
}

impl<R: Read> FormatReader for JsonReader<R> {
    fn iter(self) -> impl GeoItemIterator {
        self.features()
            .map(|result| geoitem_from_geojson(GeoJson::Feature(result?), true))
    }

    fn iter_shapes(self) -> impl Iterator<Item = Result<GeoItem>> {
        self.features()
            .map(|result| geoitem_from_geojson(GeoJson::Feature(result?), false))
    }

    // TODO: This just needs to be a geo item with a None in geom
    fn iter_meta(self) -> impl Iterator<Item = Result<Meta>> {
        self.features()
            .map(|item| Ok(item?.properties.unwrap_or_default().into()))
    }
}

pub struct NdjsonReader<R> {
    reader: Lines<R>,
}

impl<R: BufRead> NdjsonReader<R> {
    pub fn try_new(reader: R) -> Result<Self> {
        Ok(Self {
            reader: reader.lines(),
        })
    }
}

impl<R: BufRead> FormatReader for NdjsonReader<R> {
    fn iter(self) -> impl Iterator<Item = Result<GeoItem>> {
        self.reader.map(|line| {
            let geojson = GeoJson::from_str(&line?)?;
            geoitem_from_geojson(geojson, true)
        })
    }

    fn iter_shapes(self) -> impl Iterator<Item = Result<GeoItem>> {
        self.reader.map(|line| {
            let geojson = GeoJson::from_str(&line?)?;
            geoitem_from_geojson(geojson, false)
        })
    }

    fn iter_meta(self) -> impl Iterator<Item = Result<Meta>> {
        self.reader.map(|line| {
            let geojson = GeoJson::from_str(&line?)?;
            match geojson {
                GeoJson::Feature(f) => Ok(f.properties.unwrap_or_default().into()),
                GeoJson::Geometry(_) => Ok(Map::default().into()),
                _ => Err(anyhow!(GEOM_FEAT_ONLY_MSG)),
            }
        })
    }
}

pub struct JsonWriter<I: GeoItemIterator>
where
    I: Iterator,
{
    started: bool,
    ended: bool,

    /// Buffer to ensure there is a next item in the iterator.
    next_item: Option<I::Item>,

    /// [`Fuse`] ensures that we don't mess up intervleaving the separator.
    iter: Fuse<I>,

    mode: Mode,
}

// TODO: This should probably take a mode switch that deals with both, shape, meta... this needs to be baked into
// GeoItem probably just as an Option around the Geometry
impl<I: GeoItemIterator> JsonWriter<I> {
    pub fn new(iter: I, mode: Mode) -> Self {
        Self {
            started: false,
            ended: false,
            next_item: None,
            iter: iter.fuse(),
            mode,
        }
    }
}

/*
impl<I: GeoItemIterator> FormatWriter<I> for JsonWriter<I> {
    fn iter(iter: I, mode: Mode) -> Self {
        Self::new(iter, mode)
    }
}
*/

// TODO: Probably make this a specific trait method for JsonWriter, then do a blanket implementation for Iterator (or
// the other way around)
// TODO: This needs to be a result, or just make a wrapper that has a result
impl<I: GeoItemIterator> Iterator for JsonWriter<I> {
    type Item = Result<Cow<'static, [u8]>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.started {
            if let Some(v) = self.next_item.take() {
                Some(Ok(Cow::Owned(make_feature(v.unwrap(), self.mode))))
            } else {
                let next_item = self.iter.next();
                if next_item.is_some() {
                    self.next_item = next_item;
                    Some(Ok(Cow::Borrowed(b",\n")))
                } else if self.ended {
                    None
                } else {
                    self.ended = true;
                    match self.mode {
                        Mode::Full | Mode::Shapes => Some(Ok(Cow::Borrowed(b"\n]}"))),
                        Mode::Meta => Some(Ok(Cow::Borrowed(b"\n]"))),
                    }
                }
            }
        } else {
            self.started = true;
            self.next_item = self.iter.next();
            match self.mode {
                Mode::Full | Mode::Shapes => Some(Ok(Cow::Borrowed(
                    b"{type:\"FeatureCollection\",features:[\n",
                ))),
                Mode::Meta => Some(Ok(Cow::Borrowed(b"[\n"))),
            }
        }
    }
}

pub struct NdjsonWriter<I: GeoItemIterator> {
    iter: Fuse<I>,
    mode: Mode,
}

impl<I: GeoItemIterator> NdjsonWriter<I> {
    pub fn new(iter: I, mode: Mode) -> Self {
        Self {
            iter: iter.fuse(),
            mode,
        }
    }
}

// TODO: Not quite right... the output has to error also, as does the input
/*
impl<I: GeoItemIterator> FormatWriter<I> for NdjsonWriter<I> {
    fn iter(iter: I, mode: Mode) -> Self {
        Self::new(iter, mode)
    }
}
*/

impl<I: GeoItemIterator> Iterator for NdjsonWriter<I> {
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

/// TODO: This should be done with From on a parent object with the properties most likely
fn make_feature(item: GeoItem, mode: Mode) -> Vec<u8> {
    let vec = match mode {
        Mode::Full | Mode::Shapes => {
            let mut f = geojson::Feature::default();
            f.geometry = Some(geojson::Geometry::from(&item.geom));
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
        let feature_reader = JsonReader::try_new(fc.as_bytes()).expect("a valid iterator");
        let features: Vec<GeoItem> = feature_reader
            .iter_shapes()
            .map(|result| result.expect("a valid feature"))
            .collect();

        assert_eq!(features.len(), 2);
        assert!(matches!(
            features[0].geom,
            geo::Geometry::Point(geo::Point(_))
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
        let features: Vec<Result<GeoItem>> = JsonReader::try_new(f.as_bytes())
            .expect("a valid reader")
            .iter_shapes()
            .collect();
        println!("{features:?}");

        assert_eq!(features.len(), 1);
        assert!(matches!(features[0], Err(_)));
    }
}
