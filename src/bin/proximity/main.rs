mod args;

use clap::Parser;
use quadtree::MEAN_EARTH_RADIUS;
use std::time::Instant;

use crate::args::Args;
use geo_munge::csv::reader::{build_input_settings, parse_record};
use geo_munge::csv::writer::{make_csv_writer, write_line, WriteData};
use geo_munge::error::FiberError;
use geo_munge::qt::{make_bbox, make_qt_from_path, QtData};

// TODO: Refine the API and implementation
//       - If possible, enable polygon-point distances in quadtree
//       - Reject loading shape types that will cause an error in retrieval
//       - Reorganize, including fixing geojson and shp metadata
//       - Investigate a better method of making a polymorphic quadtree than
//         making a new trait, perhaps something with an enum
//       - Improve error naming and handling
//       - Capture and respond to system interupts (e.g. ctrl-c)
//       - Expand input acceptance to formats other than shp (kml, geojson/ndjson, csv points)
//         Do as a dynamic dispatch on a filetype with trait covering the required analysis
//       - Do some performance testing with perf and flamegraph
//       - Write concurrent searching, probably with Rayon
//       - Explore concurrent inserts - should be safe as if we can get an &mut at the node where
//         we are inserting or subdividing - this can block, but the rest of the qt is fine
//         can use an atomic usize for size, just need to work out how to get &mut from & when inserting
//         Perhaps something like fine grained locking or lock-free reads would help?
//       - Should meta fields support not scanning all the rows to get the fields,
//         and a number of rows different from the n when pulling data?
//       - Support Euclidean distances
//       - Support different test file formats and non-point test shapes
//       - Make the quadtree a service that can be sent points to test
//       - Enable additional input acceptance types (ndjson, csv points)

// TODO: Sphere and Eucl functions from quadtree should take references?
// TODO: Can we use Borrow or AsRef in places like HashMap::get to ease ergonomics?
// TODO: Retrieve on bounds qt needs to be able to retrieve for shapes

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

    // Set up csv parsing before building the quadtree so we can abort early if
    // it crashes on setup
    let (mut csv_reader, settings) = build_input_settings(None, delimiter)?;
    let mut csv_writer = make_csv_writer(settings.id_label, delimiter, &args.fields)?;

    // Set up the options for constructing the quadtree
    let opts = QtData::new(
        args.bounds,
        make_bbox(&args.path, args.sphere, &args.bbox)?,
        args.depth,
        args.children,
    );

    // Now build the quadtree
    if args.verbose {
        let qt_type = if opts.is_bounds { "bounds" } else { "point" };
        eprintln!(
            "Building {} quadtree: depth={}, children={}",
            qt_type, opts.depth, opts.max_children
        )
    }
    let start = Instant::now();
    let qt = make_qt_from_path(args.path, opts)?;
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
                match qt.find(&parsed.point, r, &args.fields) {
                    Ok(result) => {
                        let data = WriteData {
                            result,
                            record: parsed.record,
                            fields: &args.fields,
                            id: parsed.id,
                            index: i,
                        };

                        write_line(&mut csv_writer, &settings, data);
                    }
                    Err(quadtree::Error::OutOfBounds) => {
                        eprintln!("Input point at index {i} is out of bounds")
                    }
                    Err(_) => {
                        eprintln!("No result for record at index {i}");
                    }
                }
            }
            (Ok(parsed), Some(k)) => match qt.knn(&parsed.point, k, r, &args.fields) {
                Ok(results) => {
                    for result in results {
                        let data = WriteData {
                            result,
                            record: parsed.record,
                            fields: &args.fields,
                            id: parsed.id,
                            index: i,
                        };

                        write_line(&mut csv_writer, &settings, data);
                    }
                }
                Err(quadtree::Error::OutOfBounds) => {
                    eprintln!("Input point at index {i} is out of bounds")
                }
                Err(_) => {
                    eprintln!("No result for record at index {i}");
                }
            },
            _ => {
                eprintln!("Failed to parse record at index {i}")
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
