//! Some test harnesses.

use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use geo::{Geometry, Point};

use crate::Identified;

fn get_data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../data/sample_geojson")
        .canonicalize()
        .unwrap()
}

pub fn read_cities() -> impl Iterator<Item = (String, Point)> {
    BufReader::new(File::open(get_data_dir().join("cities.ndjson")).unwrap())
        .lines()
        .map(|s| {
            let f = s.unwrap().parse::<geojson::Feature>().unwrap();

            (
                f.properties
                    .as_ref()
                    .unwrap()
                    .get("city")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
                geo::Point::<f64>::try_from(f).unwrap().to_radians(),
            )
        })
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestRecord {
    pub name: String,
    pub point: geo::Geometry,
}

impl TestRecord {
    pub fn new_point<T: ToString>(name: T, point: geo::Point) -> Self {
        Self {
            name: name.to_string(),
            point: geo::Geometry::Point(point),
        }
    }
}

impl std::ops::Deref for TestRecord {
    type Target = Self;

    fn deref(&self) -> &Self::Target {
        self
    }
}

impl AsRef<geo::Geometry> for TestRecord {
    fn as_ref(&self) -> &geo::Geometry {
        &self.point
    }
}

// Dummy implementation, not required for testing
impl Identified for TestRecord {
    fn uid(&self) -> u32 {
        0
    }
}

pub fn read_cities_as_record() -> Vec<TestRecord> {
    read_cities()
        .map(|(name, point)| TestRecord {
            name,
            point: Geometry::from(point),
        })
        .collect()
}

pub fn read_city_pairs() -> HashMap<(String, String), f64> {
    BufReader::new(File::open(get_data_dir().join("city_dist.csv")).unwrap())
        .lines()
        .skip(1)
        .map(|s| {
            let s = s.unwrap();
            let (a, rest) = s.split_once(',').unwrap();
            let (b, dist) = rest.split_once(',').unwrap();

            ((a.to_string(), b.to_string()), dist.parse::<f64>().unwrap())
        })
        .collect()
}
