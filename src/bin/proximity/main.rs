mod args;
mod single_thread;

use clap::Parser;
use quadtree::MEAN_EARTH_RADIUS;
use single_thread::{exec_single_thread, SingleThreadOptions};
use std::time::Instant;

use crate::args::Args;
use geo_munge::csv::reader::build_input_settings;
use geo_munge::csv::writer::make_csv_writer;
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
    let mut args = Args::parse();
    args.r = args.r.map(|r| r / MEAN_EARTH_RADIUS);
    let delimiter = args.delimiter.as_bytes();
    if delimiter.len() != 1 {
        return Err(Box::new(Error::InvalidDelimiter));
    }
    let delimiter = delimiter[0];

    // Set up csv parsing before building the quadtree so we can abort early if
    // it crashes on setup
    let (csv_reader, settings) = build_input_settings(None, delimiter)?;
    let csv_writer = make_csv_writer(settings.id_label, delimiter, &args.fields)?;

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
    let qt = Quadtree::from_path(args.path.clone(), opts)?;
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

    // TODO: Turn this into a parallel iterator and process in parallel
    //       - The csv output cannot be directly turned into a par_iter
    //       - Likely provide an option to parallelize or not
    //       - Likely have to buffer that reads into a vector then switch over to processing
    //       - To help avoid starvation perhaps use two rotating vectors that are locked to write
    //       - Stick the routine in a loop that rotates one vecotr reading and the other writing
    //       - But first get it working just buffering and running per: https://github.com/rayon-rs/rayon/issues/46
    //       - Then have to think about tuning - which depends on IO speed vs processing speed
    //       - First move the non-current loop to its own file and go from there

    // After loading the quadtree, iterate through all the incoming test records
    exec_single_thread(SingleThreadOptions {
        qt,
        csv_reader,
        csv_writer,
        args,
        settings,
    });

    // Return Ok from main if everything ran correctly
    Ok(())
}
