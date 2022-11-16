mod csv;
mod error;
mod make_qt;

use std::path::PathBuf;

use crate::csv::{build_csv_settings, make_csv_writer, parse_record, write_line, WriteData};
use crate::error::FiberError;
use clap::Parser;
use geo::{Point, Rect};
use quadtree::MEAN_EARTH_RADIUS;

use crate::make_qt::make_qt;

// TODO: Refine the API and implementation
//       - Provide option to have infile as as file not just stdin
//       - Split up and reorganize make_qt file
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

// TODO: Sphere and Eucl functions from quadtree should take references
// TODO: Can we use Borrow in places like HashMap::get to ease ergonomics?

/// Command line utility to find nearest neighbors using a quadtree. The
/// quadtree is built from an input shapefile, and tested against an input
/// set of points provided as a csv. Distances are measured using the HAversine
/// formula.
#[derive(Parser, Debug)]
struct Args {
    /// The shapefile to use to assemble the QuadTree. If not provided will use
    /// {n}the default at ./data.shp.
    shp: Option<std::path::PathBuf>,

    /// Pass this flag to generate a bounds quadtree. By default the tool uses
    /// {n}a point quadtree that only accepts point-like shapefile inputs, but
    /// {n}this flag enables bounding box distances.
    #[arg(short, long)]
    bounds: bool,

    /// Retrieve `k` nearest neighbors.
    #[arg(short)]
    k: Option<usize>,

    /// Constrain the search radius by a maximum distance in meters. If not
    /// {n}included, the search ring is unbounded, but if provided, no
    /// {n}points outside the radius will be selected.
    #[arg(short)]
    r: Option<f64>,

    /// Provide a customized maximum depth for the quadtree. Defaults to 10.
    #[arg(short, long)]
    depth: Option<u8>,

    /// Provide a customized value for the maximum number of child entries
    /// {n}before the quadtree splits. Defaults to 10. Does not apply at the
    /// {n}maximum depth.
    #[arg(short, long)]
    children: Option<usize>,

    /// Provide an optional list of any metadata fields from the quadtree
    /// {n}data that should be output with the match. The input's index
    /// {n}in load order and the `id` field will automatically be added.
    /// {n}Any other must be provided here as a comma separated list of
    /// {n}field names.
    #[arg(long, value_delimiter = ',')]
    fields: Option<Vec<String>>,

    /// Set the delimiter for both the input test points and the output
    /// {n}results. Defaults to a comma. Will error of a valid single
    /// {n}character is not provided. This program will always use the
    /// {n}same delimiter on output as input.
    #[arg(long, short = 'l', default_value = ",")]
    delimiter: String,
}

pub struct QtData {
    is_bounds: bool,
    bounds: Rect<f64>,
    depth: u8,
    max_children: usize,
}

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
    let bounds = Rect::new(min.0, max.0);
    let opts = QtData {
        is_bounds: args.bounds,
        bounds,
        depth: args.depth.unwrap_or(10),
        max_children: args.children.unwrap_or(10),
    };

    // Load the shapefile, exiting with an error if the file cannot read
    // Then build the quadtree
    let shapefile = args.shp.unwrap_or(PathBuf::from(DEFAULT_SHP_PATH));
    let mut shapefile = shapefile::Reader::from_path(shapefile).map_err(|_| {
        FiberError::IO("cannot read shapefile, check path and permissions and try again")
    })?;

    // Set up csv parsing before building the quadtree so we can abort early if
    // it crashes on setup
    let (mut csv_reader, settings) = build_csv_settings(None, delimiter)?;
    let mut csv_writer = make_csv_writer(settings.id_label, delimiter, &args.fields)?;

    // Now build the quadtree
    let (qt, meta) = make_qt(&mut shapefile, opts);

    // After loading the quadtree, iterate through all the incoming test records
    for (i, record) in csv_reader.records().enumerate() {
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
    }

    // Return Ok from main if everything ran correctly
    Ok(())
}
