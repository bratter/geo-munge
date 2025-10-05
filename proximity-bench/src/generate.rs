//! Data generation utilities for proximity benchmarking.
//!
//! Provides functionality to generate random geospatial point data within specified bounding boxes
//! and write it as newline-delimited GeoJSON to any Write implementation.

use std::io::Write;

use anyhow::Result;
use geojson::{feature::Id, Feature, Value};
use protocol::request::DegreeBbox;
use rand::{rngs::StdRng, Rng, SeedableRng};

/// Random point generator with configurable seed and bounding box.
pub struct PointGenerator {
    rng: StdRng,
    bbox: DegreeBbox,
}

impl PointGenerator {
    /// Create a new point generator with the specified bounding box and optional seed.
    ///
    /// If no seed is provided, a random seed will be used.
    pub fn new(bbox: DegreeBbox, seed: Option<u64>) -> Self {
        let rng = match seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_entropy(),
        };

        Self { rng, bbox }
    }

    /// Generate a single random point within the bounding box.
    fn generate_point(&mut self) -> Value {
        let (min_lng, min_lat) = self.bbox.min();
        let (max_lng, max_lat) = self.bbox.max();

        let lng = self.rng.gen_range(min_lng..=max_lng);
        let lat = self.rng.gen_range(min_lat..=max_lat);

        Value::Point(vec![lng, lat])
    }

    /// Write the specified number of random points as newline-delimited GeoJSON.
    ///
    /// Each point is written as a GeoJSON Feature with a simple incrementing ID.
    pub fn write_points<W: Write>(&mut self, writer: &mut W, count: usize) -> Result<()> {
        for id in 0..count {
            let pt = self.generate_point();

            let mut feature = Feature::from(pt);
            feature.id = Some(Id::Number(serde_json::Number::from(id)));

            serde_json::to_writer(&mut *writer, &feature)?;
            writeln!(writer, "")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_point_generation_with_seed() {
        let bbox = DegreeBbox::default(); // World bbox
        let mut generator1 = PointGenerator::new(bbox.clone(), Some(42));
        let mut generator2 = PointGenerator::new(bbox, Some(42));

        let mut buffer1 = Cursor::new(Vec::new());
        let mut buffer2 = Cursor::new(Vec::new());

        generator1.write_points(&mut buffer1, 10).unwrap();
        generator2.write_points(&mut buffer2, 10).unwrap();

        assert_eq!(buffer1.into_inner(), buffer2.into_inner());
    }

    #[test]
    fn test_point_generation_different_seeds() {
        let bbox = DegreeBbox::default();
        let mut generator1 = PointGenerator::new(bbox.clone(), Some(42));
        let mut generator2 = PointGenerator::new(bbox, Some(43));

        let mut buffer1 = Cursor::new(Vec::new());
        let mut buffer2 = Cursor::new(Vec::new());

        generator1.write_points(&mut buffer1, 10).unwrap();
        generator2.write_points(&mut buffer2, 10).unwrap();

        assert_ne!(buffer1.into_inner(), buffer2.into_inner());
    }

    #[test]
    fn test_valid_geojson_output() {
        let bbox = DegreeBbox::default();
        let mut generator = PointGenerator::new(bbox, Some(123));
        let mut buffer = Cursor::new(Vec::new());

        generator.write_points(&mut buffer, 5).unwrap();

        let output = String::from_utf8(buffer.into_inner()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();

        assert_eq!(lines.len(), 5);

        // Verify each line is valid GeoJSON
        for line in lines {
            let feature: Feature = serde_json::from_str(line).unwrap();
            assert!(feature.geometry.is_some());
            match feature.geometry.unwrap().value {
                Value::Point(coords) => {
                    assert_eq!(coords.len(), 2);
                    assert!(coords[0] >= -180.0 && coords[0] <= 180.0); // lng
                    assert!(coords[1] >= -90.0 && coords[1] <= 90.0); // lat
                }
                _ => panic!("Expected Point geometry"),
            }
        }
    }
}

