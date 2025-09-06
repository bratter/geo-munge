//! CSV input and output format processing

use std::{borrow::Cow, collections::BTreeMap, io::Read, iter::Peekable, str::FromStr};

use anyhow::{anyhow, bail, Result};
use csv::{ByteRecord, Reader, ReaderBuilder, WriterBuilder};
use geo::{Geometry, Point};
use geo_traits::to_geo::ToGeoGeometry;
use wkb::{
    reader::read_wkb,
    writer::{geometry_wkb_size, write_geometry},
};
use wkt::{ToWkt, Wkt};

use crate::format::{ContentMode, GeoItem, GeoItemIterator, Meta, Value};

const EAGER_PARSE_MSG: &str = "Indices eagerly parsed";

/// Geometry field extraction options for CSV
/// TODO: These options need good defaults and assembly
#[derive(Debug, Clone)]
pub enum CsvGeom {
    LngLat(String, String),
    Wkt(String),
    Wkb(String),
    Json(String),
}

impl CsvGeom {
    pub fn pt(lnglat: Option<(String, String)>) -> Self {
        match lnglat {
            Some((lng, lat)) => Self::LngLat(lng, lat),
            None => Self::LngLat("lng".to_string(), "lat".to_string()),
        }
    }

    pub fn wkt(field: Option<String>) -> Self {
        CsvGeom::Wkt(field.unwrap_or_else(|| "geom".to_string()))
    }

    pub fn wkb(field: Option<String>) -> Self {
        CsvGeom::Wkb(field.unwrap_or_else(|| "geom".to_string()))
    }

    pub fn json(field: Option<String>) -> Self {
        CsvGeom::Json(field.unwrap_or_else(|| "geom".to_string()))
    }
}

impl Default for CsvGeom {
    fn default() -> Self {
        CsvGeom::wkt(None)
    }
}

impl FromStr for CsvGeom {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let lowercase = s.to_lowercase();
        let parts: Vec<&str> = lowercase.split(',').collect();

        match parts.as_slice() {
            ["pt"] => Ok(CsvGeom::pt(None)),
            ["pt", lon, lat] => Ok(CsvGeom::LngLat(lon.to_string(), lat.to_string())),
            ["wkt"] => Ok(CsvGeom::wkt(None)),
            ["wkt", field] => Ok(CsvGeom::wkt(Some(field.to_string()))),
            ["wkb"] => Ok(CsvGeom::wkb(None)),
            ["wkb", field] => Ok(CsvGeom::wkb(Some(field.to_string()))),
            ["json"] => Ok(CsvGeom::json(None)),
            ["json", field] => Ok(CsvGeom::json(Some(field.to_string()))),
            _ => Err("Invalid format for CsvGeom. Expected 'pt,[lon_field,lat_field]' or 'wkt|wbk|json,[field]'".to_string()),
        }
    }
}

/// CSV parsing settings
#[derive(Debug)]
pub struct CsvSettings {
    pub geom: CsvGeom,
    pub delimiter: u8,
}

impl Default for CsvSettings {
    fn default() -> Self {
        Self {
            geom: CsvGeom::default(),
            delimiter: b',',
        }
    }
}

pub struct CsvReader<R: Read> {
    reader: Reader<R>,
    mode: ContentMode,
    settings: CsvSettings,
    headers: Vec<String>,
    lng_lat_idx: Option<(usize, usize)>,
    wk_idx: Option<usize>,
}

impl<R: Read> CsvReader<R> {
    pub fn new(reader: R, mode: ContentMode, settings: CsvSettings) -> Result<Self> {
        let mut reader = ReaderBuilder::new()
            .delimiter(settings.delimiter)
            .from_reader(reader);

        let headers = reader.headers()?;

        let (lng_lat_idx, single_geom_idx) = match &settings.geom {
            CsvGeom::LngLat(lon_field, lat_field) => {
                let lon_idx = headers.iter().position(|h| h == lon_field).ok_or_else(|| {
                    anyhow!("Longitude field '{}' not found in CSV headers", lon_field)
                })?;
                let lat_idx = headers.iter().position(|h| h == lat_field).ok_or_else(|| {
                    anyhow!("Latitude field '{}' not found in CSV headers", lat_field)
                })?;
                (Some((lon_idx, lat_idx)), None)
            }
            CsvGeom::Wkt(field) | CsvGeom::Wkb(field) | CsvGeom::Json(field) => {
                let idx = headers.iter().position(|h| h == field).ok_or_else(|| {
                    anyhow!("Geometry field '{}' not found in CSV headers", field)
                })?;
                (None, Some(idx))
            }
        };

        // Rewrite headers as a BTreeMap for Meta processing purposes
        // Technically not necessary for MetaMode::Shapes, but minimal overhead and saves an Option
        let headers = headers.into_iter().map(str::to_string).collect();

        Ok(Self {
            reader,
            mode,
            settings,
            headers,
            lng_lat_idx,
            wk_idx: single_geom_idx,
        })
    }

    fn extract_geom(&self, record: &ByteRecord) -> Result<Geometry> {
        match &self.settings.geom {
            CsvGeom::LngLat(..) => {
                let (lon_idx, lat_idx) = self.lng_lat_idx.expect(EAGER_PARSE_MSG);
                let lon: f64 = parse_f64(record, lon_idx)?;
                let lat: f64 = parse_f64(record, lat_idx)?;
                Ok(Geometry::from(Point::new(lon, lat)))
            }
            CsvGeom::Wkt(_) => {
                let idx = self.wk_idx.expect(EAGER_PARSE_MSG);
                let wkt_bytes = record.get(idx).ok_or(anyhow!("Geom field not found"))?;
                let wkt_str = std::str::from_utf8(wkt_bytes)?;
                let parsed = Wkt::from_str(wkt_str).map_err(|e| anyhow!(e))?;
                Geometry::try_from(parsed).map_err(|e| anyhow!(e.to_string()))
            }
            CsvGeom::Wkb(_) => {
                let idx = self.wk_idx.expect(EAGER_PARSE_MSG);
                let wkb_bytes = record.get(idx).ok_or(anyhow!("Geom field not found"))?;
                read_wkb(wkb_bytes)?
                    .try_to_geometry()
                    .ok_or(anyhow!("Unable to read WKB geometry"))
            }
            CsvGeom::Json(_) => {
                let idx = self.wk_idx.expect(EAGER_PARSE_MSG);
                let geojson_bytes = record.get(idx).ok_or(anyhow!("Geom field not found"))?;
                let geojson_bytes = std::str::from_utf8(geojson_bytes)?;
                let geojson = geojson::Geometry::from_str(geojson_bytes)?;
                Ok(geo::Geometry::try_from(geojson)?)
            }
        }
    }

    fn extract_meta(&self, record: &ByteRecord) -> Result<Meta> {
        let mut meta = BTreeMap::new();

        for (i, (header, value)) in self.headers.iter().zip(record.iter()).enumerate() {
            // Skip if the current field is one of the geometry fields
            if let Some((lng, lat)) = self.lng_lat_idx {
                if lat == i || lng == i {
                    continue;
                }
            }
            if let Some(idx) = self.wk_idx {
                if idx == i {
                    continue;
                }
            }

            meta.insert(
                header.to_string(),
                Value::String(std::str::from_utf8(value)?.to_string()),
            );
        }

        Ok(meta)
    }
}

impl<R: Read> Iterator for CsvReader<R> {
    type Item = Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        // We work on byte records in order to support WKB that will likely error utf8 conversion if we attempt to use
        // strings, therefore converting each byte slice into strings when we process meta.
        let item = self
            .reader
            .byte_records()
            .next()?
            .map_err(|e| anyhow!(e))
            .and_then(|r| match self.mode {
                ContentMode::Full => Ok(GeoItem::with_props(
                    self.extract_geom(&r)?,
                    self.extract_meta(&r)?,
                )),
                ContentMode::Geometry => Ok(GeoItem::without_props(self.extract_geom(&r)?)),
                ContentMode::Properties => Ok(GeoItem::props_only(self.extract_meta(&r)?)),
            });

        Some(item)
    }
}

pub struct CsvTransformer<I: Iterator> {
    iter: Peekable<I>,
    mode: ContentMode,
    settings: CsvSettings,
    headers: Option<Vec<String>>,
}

impl<I: GeoItemIterator> CsvTransformer<I> {
    pub fn new(iter: I, mode: ContentMode, settings: CsvSettings) -> Self {
        Self {
            iter: iter.peekable(),
            mode,
            settings,
            headers: None,
        }
    }

    fn build_headers(&mut self) -> ByteRecord {
        let mut header_record = ByteRecord::new();

        match &self.settings.geom {
            CsvGeom::LngLat(lng, lat) => {
                header_record.push_field(lng.as_bytes());
                header_record.push_field(lat.as_bytes());
            }
            CsvGeom::Wkt(s) | CsvGeom::Wkb(s) | CsvGeom::Json(s) => {
                header_record.push_field(s.as_bytes())
            }
        }

        self.headers = Some(
            self.iter
                .peek()
                .unwrap_or(&Ok(GeoItem::default()))
                .as_ref()
                .unwrap_or(&GeoItem::default())
                .meta
                .as_ref()
                .unwrap_or(&BTreeMap::default())
                .keys()
                .map(|k| {
                    // NOTE: Side effect - pushing field
                    header_record.push_field(k.as_bytes());
                    k.clone()
                })
                .collect(),
        );

        header_record
    }

    fn push_geom(&self, record: &mut ByteRecord, geom: Geometry) -> Result<()> {
        match self.settings.geom {
            CsvGeom::LngLat(..) => {
                // LngLat only works for point geometries
                if let Geometry::Point(p) = geom {
                    // TODO: Truncate precision here, but then also for all other types
                    record.push_field(p.x().to_string().as_bytes());
                    record.push_field(p.y().to_string().as_bytes());
                } else {
                    bail!("Lng/Lat output only works for point-type geometries")
                }
            }
            CsvGeom::Wkt(_) => record.push_field(geom.wkt_string().as_bytes()),
            CsvGeom::Wkb(_) => {
                let mut bytes: Vec<u8> = Vec::with_capacity(geometry_wkb_size(&geom));
                write_geometry(&mut bytes, &geom, wkb::Endianness::LittleEndian)?;
                record.push_field(&bytes);
            }
            CsvGeom::Json(_) => {
                let geojson = geojson::Geometry::from(&geom);
                record.push_field(geojson.to_string().as_bytes());
            }
        };

        Ok(())
    }

    /// Push all properties onto the csv record.
    ///
    /// This method is forgiving, it ignores members not present in the headers list and inserts missing members as
    /// empty strings.
    fn push_meta(&self, record: &mut ByteRecord, meta: Meta) {
        // Loop through the headers just in case the order in each item is different
        for h in self
            .headers
            .as_ref()
            .expect("Headers defined on first next")
        {
            match meta.get(h) {
                Some(Value::String(s)) => record.push_field(s.as_bytes()),
                Some(Value::Integer(n)) => record.push_field(n.to_string().as_bytes()),
                Some(Value::Float(n)) => record.push_field(n.to_string().as_bytes()),
                Some(Value::Boolean(b)) => record.push_field(if *b { b"1" } else { b"0" }),
                Some(Value::Date(d)) => record.push_field(d.to_string().as_bytes()),
                Some(Value::DateTime(d)) => record.push_field(d.to_string().as_bytes()),
                Some(Value::Null) | None => record.push_field(b""),
            }
        }
    }

    fn write_record(&self, record: &ByteRecord) -> Result<Cow<'static, [u8]>> {
        // Unfortunately there is no way to extract the inner writer without destroying the CsvWriter, so we cannot
        // easily reset the buffer. However, regenerating the writer on each iteration should be minimal cost in
        // comparison to the string parsing, so unlikely to cause a performance problem
        let mut buffer = Vec::new();

        {
            let mut writer = WriterBuilder::new()
                .delimiter(self.settings.delimiter)
                .has_headers(false)
                .from_writer(&mut buffer);
            writer.write_byte_record(&record)?;
            writer.flush()?;
        }

        Ok(Cow::Owned(buffer))
    }
}

impl<I: GeoItemIterator> Iterator for CsvTransformer<I> {
    type Item = Result<Cow<'static, [u8]>>;

    // TODO: Look ahead option for unstructured files to build headers
    fn next(&mut self) -> Option<Self::Item> {
        // If headers is None, then it is the first item so build and output the headers
        if self.headers.is_none() {
            let headers = self.build_headers();
            return Some(self.write_record(&headers));
        }

        let record = self.iter.next()?.and_then(|item| {
            let mut byte_record = ByteRecord::new();

            // First assemble and push the geometry fields as long as we are outputting them
            // Empty geometry is an error
            // TODO: Option for error on empty output geometry rather than enforcing it? Should be consistent and
            // managed before it hits here? Check all of them
            if self.mode == ContentMode::Full || self.mode == ContentMode::Geometry {
                if let Some(geom) = item.geom {
                    self.push_geom(&mut byte_record, geom)?;
                } else {
                    bail!("No geometry present to write")
                }
            }

            // Then emit the Meta if required, using get to ensure that everything is in the correct order
            // Empty fields are not errors, they are left blank
            if self.mode == ContentMode::Full || self.mode == ContentMode::Properties {
                self.push_meta(&mut byte_record, item.meta.unwrap_or_default());
            }

            self.write_record(&byte_record)
        });

        Some(record)
    }
}

fn parse_f64(record: &ByteRecord, idx: usize) -> Result<f64> {
    let s = record
        .get(idx)
        .ok_or_else(|| anyhow!("Cannot locate geom field"))?;

    Ok(std::str::from_utf8(s)?.parse()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> Geometry {
        Geometry::Point(Point::new(x, y))
    }

    mod reader {
        use super::*;

        #[test]
        fn emits_features_from_lnglat() {
            let csv = "lng,lat,name,value,active\n1.1,1.2,point1,42,true\n2.1,2.2,point2,100,false";

            let settings = CsvSettings {
                geom: CsvGeom::LngLat("lng".to_string(), "lat".to_string()),
                delimiter: b',',
            };

            let reader = CsvReader::new(csv.as_bytes(), ContentMode::Full, settings).unwrap();
            let features: Vec<_> = reader.map(|result| result.unwrap()).collect();

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
            let first_meta = features[0].meta.as_ref().unwrap();
            assert_eq!(
                first_meta.get("name"),
                Some(&Value::String("point1".to_string()))
            );
            assert_eq!(
                first_meta.get("value"),
                Some(&Value::String("42".to_string()))
            );
            assert_eq!(
                first_meta.get("active"),
                Some(&Value::String("true".to_string()))
            );
        }

        #[test]
        fn emits_features_from_wkt() {
            let csv =
                "geom,id,category,score\n\"POINT(1.1 1.2)\",1,A,95.5\n\"POINT(2.1 2.2)\",2,B,87.3";

            let settings = CsvSettings {
                geom: CsvGeom::Wkt("geom".to_string()),
                delimiter: b',',
            };

            let reader = CsvReader::new(csv.as_bytes(), ContentMode::Full, settings).unwrap();
            let features: Vec<_> = reader.map(|result| result.unwrap()).collect();

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
            let first_meta = features[0].meta.as_ref().unwrap();
            assert_eq!(first_meta.get("id"), Some(&Value::String("1".to_string())));
            assert_eq!(
                first_meta.get("category"),
                Some(&Value::String("A".to_string()))
            );
            assert_eq!(
                first_meta.get("score"),
                Some(&Value::String("95.5".to_string()))
            );
        }
    }

    mod transform {
        use super::*;

        #[test]
        fn empty_input_gives_only_geom_headers() {
            let iter: std::iter::Empty<Result<GeoItem>> = std::iter::empty();
            let csv = CsvTransformer::new(iter, ContentMode::Full, CsvSettings::default());
            let buf: Vec<u8> = csv.map(|i| i.unwrap().to_vec()).flatten().collect();

            assert_eq!(buf, b"geom\n");
        }

        #[test]
        fn lng_lat_geom_headers_are_emitted() {
            let iter: std::iter::Empty<Result<GeoItem>> = std::iter::empty();
            let mut settings = CsvSettings::default();
            settings.geom = CsvGeom::LngLat("x".to_string(), "y".to_string());
            let csv = CsvTransformer::new(iter, ContentMode::Full, settings);
            let buf: Vec<u8> = csv.map(|i| i.unwrap().to_vec()).flatten().collect();

            assert_eq!(buf, b"x,y\n");
        }

        #[test]
        fn outputs_wkt_geom() {
            let iter = vec![
                Ok(GeoItem::without_props(pt(0., 0.))),
                Ok(GeoItem::without_props(pt(1., 0.))),
            ];

            let settings = CsvSettings::default();
            let csv = CsvTransformer::new(iter.into_iter(), ContentMode::Geometry, settings);
            let buf: Vec<u8> = csv.map(|i| i.unwrap().to_vec()).flatten().collect();

            assert_eq!(
                std::str::from_utf8(&buf).unwrap(),
                "geom\nPOINT(0 0)\nPOINT(1 0)\n"
            );
        }

        #[test]
        fn outputs_wkb_geom() {
            let iter = vec![
                Ok(GeoItem::without_props(pt(0., 0.))),
                Ok(GeoItem::without_props(pt(1., 0.))),
            ];

            let mut settings = CsvSettings::default();
            settings.geom = CsvGeom::Wkb("geom".to_string());
            let csv = CsvTransformer::new(iter.into_iter(), ContentMode::Geometry, settings);
            let buf: Vec<u8> = csv.map(|i| i.unwrap().to_vec()).flatten().collect();

            let mut check_buf = Vec::from(b"geom\n");
            write_geometry(&mut check_buf, &pt(0., 0.), wkb::Endianness::LittleEndian).unwrap();
            check_buf.push(b'\n');
            write_geometry(&mut check_buf, &pt(1., 0.), wkb::Endianness::LittleEndian).unwrap();
            check_buf.push(b'\n');

            assert_eq!(buf, check_buf);
        }

        #[test]
        fn outputs_point_geom() {
            let iter = vec![
                Ok(GeoItem::without_props(pt(0., 0.))),
                Ok(GeoItem::without_props(pt(1., 0.))),
            ];

            let mut settings = CsvSettings::default();
            settings.geom = CsvGeom::LngLat("lng".to_string(), "lat".to_string());
            let csv = CsvTransformer::new(iter.into_iter(), ContentMode::Geometry, settings);
            let buf: Vec<u8> = csv.map(|i| i.unwrap().to_vec()).flatten().collect();

            assert_eq!(std::str::from_utf8(&buf).unwrap(), "lng,lat\n0,0\n1,0\n");
        }

        #[test]
        fn outputs_json_geom() {
            let iter = vec![
                Ok(GeoItem::without_props(pt(0., 0.))),
                Ok(GeoItem::without_props(pt(1., 0.))),
            ];

            let mut settings = CsvSettings::default();
            settings.geom = CsvGeom::Json("geom".to_string());
            let csv = CsvTransformer::new(iter.into_iter(), ContentMode::Geometry, settings);
            let buf: Vec<u8> = csv.map(|i| i.unwrap().to_vec()).flatten().collect();

            assert_eq!(std::str::from_utf8(&buf).unwrap(), "geom\n\"{\"\"type\"\":\"\"Point\"\",\"\"coordinates\"\":[0.0,0.0]}\"\n\"{\"\"type\"\":\"\"Point\"\",\"\"coordinates\"\":[1.0,0.0]}\"\n");
        }

        #[test]
        fn outputs_with_props_even_when_different_order() {
            let m1 = BTreeMap::from([
                ("f1".to_string(), Value::String("v1".to_string())),
                ("f2".to_string(), Value::String("v2".to_string())),
            ]);
            let m2 = BTreeMap::from([
                ("f2".to_string(), Value::String("v3".to_string())),
                ("f1".to_string(), Value::String("v4".to_string())),
            ]);
            let iter = vec![
                Ok(GeoItem::with_props(pt(0., 0.), m1)),
                Ok(GeoItem::with_props(pt(1., 0.), m2)),
            ];

            let settings = CsvSettings::default();
            let csv = CsvTransformer::new(iter.into_iter(), ContentMode::Full, settings);
            let buf: Vec<u8> = csv.map(|i| i.unwrap().to_vec()).flatten().collect();

            assert_eq!(
                std::str::from_utf8(&buf).unwrap(),
                "geom,f1,f2\nPOINT(0 0),v1,v2\nPOINT(1 0),v4,v3\n"
            );
        }
    }
}
