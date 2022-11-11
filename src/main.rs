mod csv;
mod error;
mod make_qt;

use std::path::PathBuf;

use clap::Parser;
use crate::csv::{build_csv_settings, parse_record};
use geo::{Point, Rect};

use crate::make_qt::make_qt;

// TODO: Refine the API and implementation
//       - shp as a positional param?
//       - Infile as named param, or from stdin if not provided, in what format?
//       - Output to stdout in dsv format, provide ability to specify delimeters?
//       - Provide ability to add fields from the matches to the output, maybe
//         default to id or name, but have ability to pick a field name
//       - Split up and reorganize make_qt file
//       - Better error messages
//       - Provide values for bounds in the cli
//       - Expand input acceptance to formats other than shp (kml, geojson)
//       - Investigate a better method of making a polymorphic quadtree than
//         making a new trait
//       - Support different test file formats and non-point test shapes
//       - Make the quadtree a service that can be sent points to test
//       - Make a metadata extraction binary

// TODO: Ensure we have a point to linestring implementation in quadtree
// TODO: Sphere and Eucl functions from quadtree should take references

/// Command line utility to find nearest neighbors using a quadtree. The
/// quadtree is built from an input shapefile, and tested against an input
/// set of points provided as a csv. Distances are measured using the HAversine
/// formula.
#[derive(Parser, Debug)]
struct Args {
    /// The shapefile to use to assemble the QuadTree. If not provided will use
    /// {n}the default at ./data.shp.
    #[arg(short, long)]
    shp: Option<std::path::PathBuf>,

    /// Pass this flag to generate a bounds quadtree. By default the tool uses
    /// {n}a point quadtree that only accepts point-like shapefile inputs, but
    /// {n}this flag enables bounding box distances.
    #[arg(short, long)]
    bounds: bool,

    /// Retrieve `k` nearest neighbors.
    #[arg(short)]
    k: Option<usize>,

    /// Constrain the search radius by a maximum distance. If not included, the
    /// {n}search ring is unbounded, but if provided, no points outside the
    /// {n}radius will be selected.
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

    // Set up csv parsing before building the quadtree so we can abort early if
    // it crashes on setup
    let (mut csv, settings) = build_csv_settings(None)?;

    // Load the shapefile, exiting with an error if the file cannot read
    // Then build the quadtree
    let shapefile = args.shp.unwrap_or(PathBuf::from(DEFAULT_SHP_PATH));
    let mut shapefile = shapefile::Reader::from_path(shapefile)?;
    let (qt, meta) = make_qt(&mut shapefile, opts);

    // TODO: Now after building the shapefile, iterate through the records
    //       Note that we should not quit on errors, just error the line to stderr
    //       I think this mean we should index the results so people know how to match
    //       Also push the csv line parsing to a function that returns an error, returns a Result<(Point, id), ()> and does the radian conversion
    for (i, record) in csv.records().enumerate() {
        match parse_record(record, &settings) {
            Ok((p, id)) => {
                match args.k {
                    None | Some(1) => {
                        let res = qt.find(&p, args.r);
                        println!("{:?}", res);
                        // TODO: Perhaps a print find function?
                    }
                    Some(k) => {
                        let res = qt.knn(&p, k, args.r);
                        println!("{:?}", res);
                        // TODO: Print properly
                    }
                }
            }
            Err(_) => {
                eprintln!("Failed to parse record at index {i}.")
            }
        }
    }

    // Return Ok from main if everything ran correctly
    Ok(())
}
