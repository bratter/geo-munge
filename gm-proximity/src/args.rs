use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::message::prelude::*;

/// Command line client and server for proximity-based geospatial operations.
#[derive(Debug, Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Server,
    Client(ClientCommandWrapper),
}

#[derive(Debug, Parser)]
pub struct ClientCommandWrapper {
    #[command(subcommand)]
    pub command: ClientCommand,
}

#[derive(Debug, Subcommand)]
pub enum ClientCommand {
    /// Get statistics about the current quadtree.
    Stats,

    /// Reset the current quadtree, dropping all entries.
    Reset(ResetArgs),

    /// Load data to insert from a file (if provided) or stdin otherwise.
    Load { file: Option<PathBuf> },

    /// Conduct a Knn search on the quadtree.
    ///
    /// Will return at most k matches, potentially fewer if less matching the filters are in the quadtree or within the
    /// radius defined by the `r` parameter.
    ///
    /// It's possible to set k=1 for this, but to avoid setting k, use find to get the single nearest neighbor.
    Knn(KnnArgs),
}

#[derive(Debug, Parser)]
pub struct ResetArgs {
    #[clap(long, short)]
    pub bbox: Option<Bbox>,

    #[clap(long, short)]
    pub force: bool,
}

#[derive(Debug, Parser)]
pub struct KnnArgs {
    /// The number of nearest neighbors to find.
    #[clap(short)]
    pub k: usize,

    /// Maximum seach radius to truncate the search.
    #[clap(short)]
    pub r: Option<f64>,

    // TODO: We want to have a flag that indicates whether the inputs are keys or geoms, then a flag for data and an arg
    // for file
    /// Flag to determine whether we are trying to test keys instead of geometries.
    ///
    /// When testing with keys, this indicates to the server that the test geometry is already in the quadtree with the
    /// given set of keys. This flag governs the type of data expected as data, as the file, or on stdin.
    #[clap(short = 'x', long = "keys")]
    pub test_keys: bool,

    /// Manually passed data to test.
    #[clap(long, short)]
    pub data: Option<String>,

    /// An optional file input containing items to test.
    #[clap(conflicts_with = "data")]
    pub file: Option<PathBuf>,
}
