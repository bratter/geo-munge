mod args;

use clap::Parser;
use geo::{Point, Rect};
use quadtree::{ToRadians, MEAN_EARTH_RADIUS};
use shapefile::Reader;
use std::time::Instant;

use crate::args::Args;
use fiberdist::csv::reader::{build_input_settings, parse_record};
use fiberdist::csv::writer::{make_csv_writer, write_line, WriteData};
use fiberdist::error::FiberError;
use fiberdist::qt::{make_qt, QtData};

// TODO: Refine the API and implementation
//       - Provide option to have infile as as file not just stdin
//       - Capture and respond to system interupts (e.g. ctrl-c)
//       - Expand input acceptance to formats other than shp (kml, geojson/ndjson, csv points)
//         Do as a dynamic dispatch on a filetype with trait covering the required analysis
//       - Do some performance testing with perf and flamegraph
//       - Write concurrent searching, probably with Rayon
//       - Explore concurrent inserts - should be safe as if we can get an &mut at the node where
//         we are inserting or subdividing - this can block, but the rest of the qt is fine
//         can use an atomic usize for size, just need to work out how to get &mut from & when inserting
//         Perhaps something like fine grained locking or lock-free reads would help?
//       - Support Euclidean distances
//       - Investigate a better method of making a polymorphic quadtree than
//         making a new trait
//       - Support different test file formats and non-point test shapes
//       - Make the quadtree a service that can be sent points to test

// TODO: Sphere and Eucl functions from quadtree should take references?
// TODO: Can we use Borrow in places like HashMap::get to ease ergonomics?
// TODO: Retrieve on bounds qt needs to be able to retrieve for shapes

fn make_bbox<'a, T>(args: &Args, shp: &Reader<T>) -> Result<Rect, FiberError>
where
    T: std::io::Read + std::io::Seek,
{
    // Get the right bbox points given the argument values
    let (a, b) = if args.sphere {
        // Sphere option builds sphere bounds broken at the anitmeridian
        (Point::new(-180.0, -90.0), Point::new(180.0, 90.0))
    } else if let Some(bbox_str) = &args.bbox {
        // Parse from the bbox_str
        let mut pts = bbox_str.split(',').map(|s| {
            s.parse::<f64>()
                .map_err(|_| FiberError::Arg("Cannot parse bbox input"))
        });
        (
            Point::new(bbox_next(&mut pts)?, bbox_next(&mut pts)?),
            Point::new(bbox_next(&mut pts)?, bbox_next(&mut pts)?),
        )
    } else {
        // Default to the bbox available on the input file
        (shp.header().bbox.min.into(), shp.header().bbox.max.into())
    };

    let mut rect = Rect::new(a.0, b.0);
    rect.to_radians_in_place();
    Ok(rect)
}

fn bbox_next<'a>(
    pts: &mut dyn Iterator<Item = Result<f64, FiberError>>,
) -> Result<f64, FiberError> {
    pts.next()
        .ok_or(FiberError::Arg("Unexpected end of bbox input"))
        .and_then(|x| x)
}

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

    // Load the shapefile, exiting with an error if the file cannot read
    // Then build the quadtree
    let mut shapefile = shapefile::Reader::from_path(&args.shp).map_err(|_| {
        FiberError::IO("cannot read shapefile, check path and permissions and try again")
    })?;

    // Set up the options for constructing the quadtree
    let opts = QtData::new(
        args.bounds,
        make_bbox(&args, &shapefile)?,
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
            (Ok(parsed), None) | (Ok(parsed), Some(1)) => match qt.find(&parsed.point, r) {
                Ok((datum, dist)) => {
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
                Err(quadtree::Error::OutOfBounds) => {
                    eprintln!("Input point at index {i} is out of bounds")
                }
                Err(_) => {
                    eprintln!("No result for record at index {i}");
                }
            },
            (Ok(parsed), Some(k)) => match qt.knn(&parsed.point, k, r) {
                Ok(results) => {
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
