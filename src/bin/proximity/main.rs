mod args;

use clap::Parser;
use quadtree::MEAN_EARTH_RADIUS;
use std::time::Instant;

use crate::args::Args;
use geo_munge::csv::reader::{build_input_settings, parse_record};
use geo_munge::csv::writer::{make_csv_writer, write_line, WriteData};
use geo_munge::error::Error;
use geo_munge::qt::{make_bbox, QtData, Quadtree};

// TODO: Refine the API and implementation
//       - Capture and respond to system interupts (e.g. ctrl-c)
//       - Do some performance testing with perf and flamegraph
//       - Write concurrent searching, probably with Rayon, this can be done in proximity itself
//       - Explore concurrent inserts - should be safe as if we can get an &mut at the node where
//         we are inserting or subdividing - this can block, but the rest of the qt is fine
//         can use an atomic usize for size, just need to work out how to get &mut from & when inserting
//         Perhaps something like fine grained locking or lock-free reads would help?
//         Two options: AtomicPtr for lock free or fine grained locking per node (each node in a
//         mutex) - lock free might be more efficient when getting shared references on read
//       - Should meta fields support not scanning all the rows to get the fields,
//         and a number of rows different from the n when pulling data?
//       - Support Euclidean distances
//       - Support different test file formats and non-point test shapes
//       - Make the quadtree a service that can be sent points to test
//       - Enable additional input acceptance types (e.g. ndjson)
//
// TODO: Retrieve on bounds qt needs to be able to retrieve for shapes

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let r = args.r.map(|r| r / MEAN_EARTH_RADIUS);
    let delimiter = args.delimiter.as_bytes();
    if delimiter.len() != 1 {
        return Err(Box::new(Error::InvalidDelimiter));
    }
    let delimiter = delimiter[0];

    // Set up csv parsing before building the quadtree so we can abort early if
    // it crashes on setup
    let (mut csv_reader, settings) = build_input_settings(None, delimiter)?;
    let mut csv_writer = make_csv_writer(settings.id_label, delimiter, &args.fields)?;

    // Set up the options for constructing the quadtree
    let opts = QtData::new(
        args.point,
        make_bbox(&args.path, args.sphere, &args.bbox)?,
        args.depth,
        args.children,
    );

    // Now build the quadtree
    if args.verbose {
        let qt_type = if opts.is_point_qt { "point" } else { "bounds" };
        eprintln!(
            "Building {} quadtree: depth={}, children={}",
            qt_type, opts.depth, opts.max_children
        )
    }
    let start = Instant::now();
    let qt = Quadtree::from_path(args.path, opts)?;
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
    let start = Instant::now();
    for (i, record) in csv_reader.records().enumerate() {
        match (parse_record(i, record, &settings), args.k) {
            (Ok(parsed), None) | (Ok(parsed), Some(1)) => match qt.find(&parsed, r, &args.fields) {
                Ok(result) => {
                    let data = WriteData {
                        result,
                        record: &parsed.record,
                        fields: &args.fields,
                        id: &parsed.id,
                        index: i,
                    };

                    write_line(&mut csv_writer, &settings, data);
                }
                Err(err) => eprintln!("{err}"),
            },
            (Ok(parsed), Some(k)) => match qt.knn(&parsed, k, r, &args.fields) {
                Ok(results) => {
                    for result in results {
                        let data = WriteData {
                            result,
                            record: &parsed.record,
                            fields: &args.fields,
                            id: &parsed.id,
                            index: i,
                        };

                        write_line(&mut csv_writer, &settings, data);
                    }
                }
                Err(err) => eprintln!("{err}"),
            },
            (Err(err), _) => eprintln!("{err}"),
        }

        if args.verbose && i % 10000 == 0 {
            eprintln!(
                "Processed {} records in {} ms",
                i,
                start.elapsed().as_millis()
            );
        }
    }

    // Return Ok from main if everything ran correctly
    Ok(())
}
