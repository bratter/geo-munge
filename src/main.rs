mod make_qt;

use std::path::PathBuf;

use clap::Parser;
use geo::{Point, Rect};
use quadtree::*;

use crate::make_qt::make_qt;

// TODO: Structure a pipeline for building a quadtree
//       Limit this to only spherical geometries
//       - Take command line arguments for the input shapefile and the input set of test points,
//       one of them can be subbed by - for stdin
//       - Auto-detect the type of input from the extension, but stick with just shp at first
//       - Check that the .shp importer iterates, although not mush to do it is doesn't
//       - Iterate over the shapes, flatmapping MultiLineString into LineString, will need to
//       ensure that we have a point to linestring implementation in quadtree for this to work
//       - Load into a BoundsQuadTree
//       - Load the test points, iterate through them doing the comparison, outputting to stdout
//       - Provide options for k and r, defaults to 1 and inifinity using find

/// Command line utility to find nearest neighbors using a quadtree. The
/// quadtree is built from an input shapefile, and tested against an input
/// set of points provided as a csv. Distances are measured using the HAversine
/// formula.
#[derive(Parser, Debug)]
struct Args {
    /// The shapefile to use to assemble the QuadTree. If none is provided will
    /// use the default.
    #[arg(short, long)]
    shp: Option<std::path::PathBuf>,

    /// Pass this flag to generate a bounds quadtree. By default the tool uses
    /// a point quadtree that only accepts point-like shapefile inputs, but this
    /// flag enables bounding box distances.
    #[arg(short, long)]
    bounds: bool,

    /// Retrieve `k` nearest neighbors.
    #[arg(short)]
    k: Option<usize>,

    /// Constrain the search radius by a maximum distance. If not included, the
    /// search ring is unbounded, but if provided, no points outside the radius
    /// will be selected.
    #[arg(short)]
    r: Option<f64>,
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
    println!("{:?}", args);

    // Initialize the quadtree as point or bounds, and load the data
    // Bounds default to a whole Earth model
    // TODO: Sphere and Eucl functions from quadtree should take references
    // TODO: Have the ability to provide values for the bounds
    // TODO: Allow setting the depth and the max children, use the r param
    // TODO: Investigate a better method of making a polymorphic quadtree than
    //       making a new trait
    let min = Point::new(-180.0, -90.0).to_radians();
    let max = Point::new(180.0, 90.0).to_radians();
    let bounds = Rect::new(min.0, max.0);

    // Load the shapefile, exiting with an error if the file cannot read
    let shapefile = args.shp.unwrap_or(PathBuf::from(DEFAULT_SHP_PATH));
    let mut shapefile = shapefile::Reader::from_path(shapefile)?;

    // Set up the options for constructing the shapefile
    let opts = QtData {
        is_bounds: args.bounds,
        bounds,
        depth: 10,
        max_children: 10,
    };

    let qt = make_qt(&mut shapefile, opts);

    // for shp in shapefile.iter_shapes() {
    //     match shp {
    //         Ok(shapefile::Shape::Point(p)) => println!("{:?}", Point::try_from(p)),
    //         Ok(_) => println!("Some other shape"),
    //         Err(err) => println!("{:?}", err),
    //     }
    // }

    // Run the search using find if k is None or 1, knn otherwise
    let cmp = Point::new(-0.5, 0.5);
    match args.k {
        None | Some(1) => {
            let res = qt.find(&cmp).unwrap();
            println!("{:?}", res);
        }
        Some(k) => {
            let res = qt.knn(&cmp, k).unwrap();
            println!("{:?}", res);
        }
    }

    // Return Ok from main if everything ran correctly
    Ok(())
}
