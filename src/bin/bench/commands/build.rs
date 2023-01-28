use rand::distributions::{Distribution, Uniform};
use shapefile::{
    dbase::{Record, TableWriterBuilder},
    Point,
};
use std::fs;

use geo_munge::error::Error;

use crate::{run::Run, Paths};

pub fn build(name: &String, paths: &Paths) -> Result<(), Error> {
    // Open the run file and load
    let mut run_path = paths.run.clone();
    run_path.push(format!("{name}.json"));

    let run_file = fs::File::open(&run_path).map_err(|err| Error::FileIOError(err))?;

    let run_set: Vec<Run> = serde_json::from_reader(&run_file)
        .map_err(|err| Error::FailedToDeserialize(run_path, err))?;

    // TODO: This create and loop will probably be dropped into its own file for reuse in execute
    // TODO: Consider cleanup if writing fails rather than just aborting
    // Clean any existing folder and create fresh
    let mut build_dir = paths.build.clone();
    build_dir.push(name);
    if build_dir
        .try_exists()
        .map_err(|err| Error::FileIOError(err))?
    {
        fs::remove_dir_all(&build_dir).map_err(|err| Error::FileIOError(err))?;
    }
    fs::create_dir_all(&build_dir).map_err(|err| Error::FileIOError(err))?;

    // Build the random number generators
    let mut rng = rand::thread_rng();
    let lng_dist = Uniform::from(-180.0..180.0);
    let lat_dist = Uniform::from(-90.0..90.0);

    // Iterate over all the runs to build
    for (i, run) in run_set.iter().enumerate() {
        // Open data file for writing a shapefile
        let mut data_path = build_dir.clone();
        data_path.push(format!("{i}data.shp"));
        let dbf = TableWriterBuilder::new();
        let mut writer = shapefile::Writer::from_path(data_path, dbf)
            .map_err(|err| Error::ShapeFileWriteError(err))?;

        for _ in 0..run.count {
            writer
                .write_shape_and_record(
                    &Point {
                        x: lng_dist.sample(&mut rng),
                        y: lat_dist.sample(&mut rng),
                    },
                    &Record::default(),
                )
                .map_err(|err| Error::ShapeFileWriteError(err))?;
        }

        // Open cmp file for writing a csv
        let mut cmp_path = build_dir.clone();
        cmp_path.push(format!("{i}cmp.csv"));
        let mut writer =
            csv::Writer::from_path(cmp_path).map_err(|err| Error::CsvWriteError(err))?;
        writer
            .write_record(["lng", "lat"])
            .map_err(|err| Error::CsvWriteError(err))?;

        for _ in 0..run.cmp {
            writer
                .write_record([
                    lng_dist.sample(&mut rng).to_string(),
                    lat_dist.sample(&mut rng).to_string(),
                ])
                .map_err(|err| Error::CsvWriteError(err))?;
        }
    }

    Ok(())
}
