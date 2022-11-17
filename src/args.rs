use clap::Parser;

/// Command line utility to find nearest neighbors using a quadtree. The
/// quadtree is built from an input shapefile, and tested against an input
/// set of points provided as a csv. Distances are measured using the HAversine
/// formula.
#[derive(Parser, Debug)]
pub struct Args {
    /// The shapefile to use to assemble the QuadTree. If not provided will use
    /// {n}the default at ./data.shp.
    pub shp: Option<std::path::PathBuf>,

    /// Pass this flag to generate a bounds quadtree. By default the tool uses
    /// {n}a point quadtree that only accepts point-like shapefile inputs, but
    /// {n}this flag enables bounding box distances.
    #[arg(short, long)]
    pub bounds: bool,

    /// Retrieve `k` nearest neighbors.
    #[arg(short)]
    pub k: Option<usize>,

    /// Constrain the search radius by a maximum distance in meters. If not
    /// {n}included, the search ring is unbounded, but if provided, no
    /// {n}points outside the radius will be selected.
    #[arg(short)]
    pub r: Option<f64>,

    /// Provide a customized maximum depth for the quadtree. Defaults to 10.
    #[arg(short, long)]
    pub depth: Option<u8>,

    /// Provide a customized value for the maximum number of child entries
    /// {n}before the quadtree splits. Defaults to 10. Does not apply at the
    /// {n}maximum depth.
    #[arg(short, long)]
    pub children: Option<usize>,

    /// Provide an optional list of any metadata fields from the quadtree
    /// {n}data that should be output with the match. The input's index
    /// {n}in load order and the `id` field will automatically be added.
    /// {n}Any other must be provided here as a comma separated list of
    /// {n}field names.
    #[arg(long, value_delimiter = ',')]
    pub fields: Option<Vec<String>>,

    /// Set the delimiter for both the input test points and the output
    /// {n}results. Defaults to a comma. Will error of a valid single
    /// {n}character is not provided. This program will always use the
    /// {n}same delimiter on output as input.
    #[arg(long, short = 'l', default_value = ",")]
    pub delimiter: String,
}
