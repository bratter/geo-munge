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
    Bench(Bench),
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

    /// Benchmarking command to load test the server.
    ///
    /// It can be used in a standalone client, but should be used by the main benchmarking command.
    Bench(BenchClient),
}

#[derive(Debug, Parser)]
pub struct ResetArgs {
    #[clap(long, short)]
    pub bbox: Option<Bbox>,

    /// Use a u32 key from metadata instead of an auto-increment.
    ///
    /// The key must exist and be within the range of a u32 for all items or operations will fail.
    ///
    /// The argument takes a string that contains the JSON Pointer definition to a numeric field.
    #[clap(long, short = 'k', conflicts_with = "key_bytes")]
    pub key_int: Option<String>,

    /// Use an arbitrary field from metadata instead of an auto-increment.
    ///
    /// The field must be less than or equal to 16 bytes long or operations will fail. This is intended for use with
    /// alphanumeric identifiers. The argument takes a string that contains the JSON Pointer definition to a string
    /// field.
    #[clap(long, short = 'y')]
    pub key_bytes: Option<String>,

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

#[derive(Debug, Parser)]
pub struct Bench {
    /// Total amount of data to test with **in Mb**. Doesn't include overhead.
    pub total_data: Option<usize>,

    #[clap(long, short = 'q')]
    pub request_size: Option<u32>,

    #[clap(long, short = 'r')]
    pub response_size: Option<u32>,

    #[clap(long, short = 't')]
    pub response_ratio: Option<u32>,

    /// Delay to simulate processing time to generate **each response** on the server. This applies to each response,
    /// not request, so when the response ratio is >1 this delay will apply multiple times to a single request.
    #[clap(long, short = 'p')]
    pub handle_delay: Option<u64>,

    #[clap(long, short = 's')]
    pub send_delay: Option<u64>,

    #[clap(long, short = 'v')]
    pub receive_delay: Option<u64>,
}

#[derive(Debug, Parser)]
pub struct BenchClient {
    pub total_data: usize,

    #[clap(long, short = 'q')]
    pub request_size: u32,

    #[clap(long, short = 'r')]
    pub response_size: u32,

    #[clap(long, short = 't')]
    pub response_ratio: u32,

    #[clap(long, short = 'p')]
    pub handle_delay: Option<u64>,

    #[clap(long, short = 's')]
    pub send_delay: Option<u64>,

    #[clap(long, short = 'v')]
    pub receive_delay: Option<u64>,
}
