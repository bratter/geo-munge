mod args;
mod csv;
mod multi_thread;
mod run;
mod single_thread;

use clap::Parser;
use quadtree::MEAN_EARTH_RADIUS;
use std::time::Instant;

use crate::args::Args;
use crate::csv::reader::build_input_settings;
use crate::csv::writer::make_csv_writer;
use geo_munge::qt::{make_bbox, QtData, Quadtree};

use multi_thread::exec_multi_thread;
use single_thread::exec_single_thread;

// TODO: Refine the API and implementation
//       - Fix id_label in the build_input_settings call - it is set to none!
//       - We now use Arc instead of Rc in BaseData so the quadtree can be sent through the
//         parallel iterator, but this adds at least some overhead when reading - should we
//         reorganize to only send a reference through the par_iter, and keep the data outside?
//       - Capture and respond to system interupts (e.g. ctrl-c)
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

pub(crate) type CsvReader = ::csv::Reader<std::io::Stdin>;
pub(crate) type CsvWriter = ::csv::Writer<std::io::Stdout>;

/// Index and label and field settings for the stream of test points.
#[derive(Clone)]
pub struct InputSettings {
    pub lat_index: usize,
    pub lng_index: usize,
    pub id_index: Option<usize>,
    pub id_label: &'static str,
    pub delimiter: u8,
    pub k: Option<usize>,
    pub r: Option<f64>,
    pub fields: Option<Vec<String>>,
    pub verbose: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Extract and process everything we need from args
    let mut args = Args::parse();
    args.r = args.r.map(|r| r / MEAN_EARTH_RADIUS);
    let verbose = args.verbose;
    let single_thread = args.single_thread;
    let print_qt = args.print;

    // Set up csv parsing before building the quadtree so we can abort early if
    // it crashes on setup
    let (csv_reader, settings) = build_input_settings(&args)?;
    let csv_writer = make_csv_writer(&settings)?;

    // Set up the options for constructing the quadtree
    let opts = QtData::new(
        args.point,
        make_bbox(&args.path, args.sphere, &args.bbox)?,
        args.depth,
        args.children,
    );

    // Now build the quadtree
    if verbose {
        let qt_type = if opts.is_point_qt { "point" } else { "bounds" };
        eprintln!(
            "Building {} quadtree: depth={}, children={}",
            qt_type, opts.depth, opts.max_children
        )
    }

    let start = Instant::now();
    let qt = Quadtree::from_path(args.path, opts)?;
    if verbose || print_qt {
        eprintln!(
            "Quadtree with {} children built in {} ms",
            qt.size(),
            start.elapsed().as_millis()
        )
    }
    if print_qt {
        eprintln!("{}", qt);
    }

    // After loading the quadtree, iterate through all the incoming test records
    // Run multi-threaded by default, but use the argument to select single-threaded if required
    let start = Instant::now();
    if single_thread {
        if settings.verbose {
            eprintln!("Starting single-threaded execution");
        }

        exec_single_thread(csv_reader, csv_writer, &qt, &settings);
    } else {
        if verbose {
            eprintln!("Starting multi-threaded execution");
        }

        exec_multi_thread(csv_reader, csv_writer, &qt, &settings);
    }
    if settings.verbose {
        eprintln!("Finished in {} ms", start.elapsed().as_millis());
    }

    // Return Ok from main if everything ran correctly
    Ok(())
}
