mod args;
mod csv;
mod error;
mod qt;
mod shp;

use clap::Parser;
use geo::{Point, Rect};
use quadtree::MEAN_EARTH_RADIUS;
use std::path::PathBuf;
use std::time::Instant;

use crate::args::Args;
use crate::csv::reader::{build_input_settings, parse_record};
use crate::csv::writer::{make_csv_writer, write_line, WriteData};
use crate::error::FiberError;
use crate::qt::{make_qt, QtData};

// TODO: Refine the API and implementation
//       - Provide option to have infile as as file not just stdin
//       - Provide values for bounds in the cli
//         And option to use bounds from the shapefile
//       - Expand input acceptance to formats other than shp (kml, geojson, csv points)
//       - Do some performance testing with perf and flamegraph
//       - Write concurrent searching, probably with Rayon
//       - Explore concurrent inserts - should be safe as if we can get an &mut at the node where
//         we are inserting or subdividing - this can block, but the rest of the qt is fine
//         can use an atomic usize for size, just need to work out how to get &mut from & when inserting
//       - Investigate a better method of making a polymorphic quadtree than
//         making a new trait
//       - Support different test file formats and non-point test shapes
//       - Make the quadtree a service that can be sent points to test
//       - Make a metadata extraction binary

// TODO: Adding points to a Bounds qt does not seem to be inserting correctly
// TODO: Sphere and Eucl functions from quadtree should take references
// TODO: Can we use Borrow in places like HashMap::get to ease ergonomics?

/// We look in the current directory for a data.shp file by default
const DEFAULT_SHP_PATH: &str = "./data.shp";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let r = args.r.map(|r| r / MEAN_EARTH_RADIUS);
    let delimiter = args.delimiter.as_bytes();
    if delimiter.len() != 1 {
        return Err(Box::new(FiberError::Arg(
            "delimeter option must be a single character",
        )));
    }
    let delimiter = delimiter[0];

    // Set up the options for constructing the quadtree
    let min = Point::new(-180.0, -90.0).to_radians();
    let max = Point::new(180.0, 90.0).to_radians();
    let opts = QtData::new(
        args.bounds,
        Rect::new(min.0, max.0),
        args.depth,
        args.children,
    );

    // Load the shapefile, exiting with an error if the file cannot read
    // Then build the quadtree
    let shapefile = args.shp.unwrap_or(PathBuf::from(DEFAULT_SHP_PATH));
    let mut shapefile = shapefile::Reader::from_path(shapefile).map_err(|_| {
        FiberError::IO("cannot read shapefile, check path and permissions and try again")
    })?;

    // Set up csv parsing before building the quadtree so we can abort early if
    // it crashes on setup
    let (mut csv_reader, settings) = build_input_settings(None, delimiter)?;
    let mut csv_writer = make_csv_writer(settings.id_label, delimiter, &args.fields)?;

    // Now build the quadtree
    if args.verbose {
        let qt_type = if opts.is_bounds { "bounds" } else { "point" };
        eprintln!(
            "Building {} quadtree: depth={}, children={}",
            qt_type, opts.depth, opts.max_children
        )
    }
    let start = Instant::now();
    let (qt, meta) = make_qt(&mut shapefile, opts);
    if args.verbose || args.print {
        eprintln!(
            "Quadtree with {} children built in {} ms",
            qt.size(),
            start.elapsed().as_millis()
        )
    }
    if args.print {
        eprintln!("{}", qt);
    }

    // After loading the quadtree, iterate through all the incoming test records
    for (i, record) in csv_reader.records().enumerate() {
        let start = Instant::now();

        match (parse_record(i, record.as_ref(), &settings), args.k) {
            (Ok(parsed), None) | (Ok(parsed), Some(1)) => {
                if let Ok((datum, dist)) = qt.find(&parsed.point, r) {
                    let data = WriteData {
                        record: parsed.record,
                        datum,
                        meta: meta.get(datum.1).unwrap(),
                        fields: &args.fields,
                        dist,
                        id: parsed.id,
                        index: i,
                    };

                    write_line(&mut csv_writer, &settings, data);
                } else {
                    eprintln!("No result for record at index {i}.");
                }
            }
            (Ok(parsed), Some(k)) => {
                if let Ok(results) = qt.knn(&parsed.point, k, r) {
                    for (datum, dist) in results {
                        let data = WriteData {
                            record: parsed.record,
                            datum,
                            meta: meta.get(datum.1).unwrap(),
                            fields: &args.fields,
                            dist,
                            id: parsed.id,
                            index: i,
                        };

                        write_line(&mut csv_writer, &settings, data);
                    }
                } else {
                    eprintln!("No result for record at index {i}.");
                }
            }
            _ => {
                eprintln!("Failed to parse record at index {i}.")
            }
        }

        if args.verbose {
            eprintln!(
                "Processed record {} in {} ms",
                i,
                start.elapsed().as_millis()
            );
        }
    }

    // Return Ok from main if everything ran correctly
    Ok(())
}
