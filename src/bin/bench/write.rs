use rand::{
    distr::{Distribution, Uniform},
    rngs::ThreadRng,
};
use std::path::PathBuf;

use geo_munge::error::Error;
use shapefile::{
    dbase::{Record, TableWriterBuilder},
    Point,
};

use crate::run::Run;

// TODO: It is probably safe to have the end user only require a shared reference, so could wrap in
// a raw or something
struct Rand {
    rng: ThreadRng,
    lng_dist: Uniform<f64>,
    lat_dist: Uniform<f64>,
}

impl Rand {
    fn lng(&mut self) -> f64 {
        self.lng_dist.sample(&mut self.rng)
    }

    fn lat(&mut self) -> f64 {
        self.lat_dist.sample(&mut self.rng)
    }
}

/// Default rand drops uniformly over lng/lat coordinates.
impl Default for Rand {
    fn default() -> Self {
        Self {
            rng: rand::rng(),
            lng_dist: Uniform::new_inclusive(-180.0, 180.0).expect("Valid hardcoded range"),
            lat_dist: Uniform::new_inclusive(-90.0, 90.0).expect("Valid hardcoded range"),
        }
    }
}

pub fn write_data(build_dir: &PathBuf, run: &Run) -> Result<PathBuf, Error> {
    // Build the shapefile writer
    let mut data_path = build_dir.clone();
    data_path.push(format!("{}data.shp", run.index));
    let dbf = TableWriterBuilder::new();
    let mut writer = shapefile::Writer::from_path(&data_path, dbf)
        .map_err(|err| Error::ShapeFileWriteError(err))?;

    // Get the random number generator
    let mut rand = Rand::default();

    for _ in 0..run.count {
        writer
            .write_shape_and_record(
                &Point {
                    x: rand.lng(),
                    y: rand.lat(),
                },
                &Record::default(),
            )
            .map_err(|err| Error::ShapeFileWriteError(err))?;
    }

    Ok(data_path)
}

pub fn write_cmp(build_dir: &PathBuf, run: &Run) -> Result<PathBuf, Error> {
    // Build the csv writer
    let mut cmp_path = build_dir.clone();
    cmp_path.push(format!("{}cmp.csv", run.index));
    let mut writer = csv::Writer::from_path(&cmp_path).map_err(|err| Error::CsvWriteError(err))?;
    writer
        .write_record(["lng", "lat"])
        .map_err(|err| Error::CsvWriteError(err))?;

    // Get the random number generator
    let mut rand = Rand::default();

    for _ in 0..run.cmp {
        writer
            .write_record([rand.lng().to_string(), rand.lat().to_string()])
            .map_err(|err| Error::CsvWriteError(err))?;
    }

    Ok(cmp_path)
}
