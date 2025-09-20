use std::path::PathBuf;

use clap::{Args as ArgsTrait, Parser, Subcommand};
use protocol::prelude::*;

use crate::handler::{OutputFormat, OutputOptions};

/// Command line client for proximity-based geospatial operations.
#[derive(Debug, Parser)]
pub struct Args {
    /// Use TCP instead of Unix socket (Unix only, Windows always uses TCP)
    #[cfg(unix)]
    #[clap(long)]
    pub tcp: bool,

    /// Custom socket path (Unix) or address (TCP)
    /// For Unix sockets, must start with /tmp
    #[clap(long, short)]
    pub socket: Option<String>,

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

    /// Pull shapes and/or metadata from the store by id.
    Get(GetArgs),

    /// Remove entries from the store by id.
    Delete(DeleteArgs),

    /// Conduct a Knn search on the spatial index.
    ///
    /// Will return at most k matches, potentially fewer if less matching the filters are in the quadtree or within the
    /// radius defined by the `r` parameter.
    ///
    /// It's possible to set k=1 for this, but to avoid setting k, use find to get the single nearest neighbor.
    Knn(KnnArgs),

    /// Run a windowing bounding box query on the spatial index.
    ///
    /// Will return all items that either intersect with or are contained by the provided bounding box, depending on the
    /// mode parameter.
    Window(WindowArgs),

    /// Start a REPL loop.
    Repl(ReplArgs),

    /// Benchmarking command to load test the server.
    ///
    /// It can be used in a standalone client, but should be used by the main benchmarking command.
    Bench(BenchClient),
}

/// Output options struct for use as nested clap parser.
///
/// This args version has an optional content_mode to allow Get and the other uses to have different defaults.
/// Additionally the csv flags are negative flags to tie in better with CLI options.
///
/// TODO: Add a pretty print json option that applies to JSON output only
#[derive(Debug, Default, Clone, Copy, ArgsTrait)]
struct OutputOptionArgs {
    /// Content mode for output data. for get requests will default to "full", but all others will default to returning
    /// the request metadata only.
    #[clap(long = "content", short = 'c')]
    pub content_mode: Option<ContentMode>,

    /// Whether to output data as newline delimited json (default) or a csv/json hybrid with the result data in csv and
    /// any returned geometries or properties as json in a csv field.
    #[clap(long = "format", short = 'f', default_value = "json")]
    pub output_format: OutputFormat,

    /// Turn off header rendering for csv output.
    #[clap(long)]
    pub no_header: bool,

    /// Turn off json escaping in csv output.
    #[clap(long)]
    pub no_escape: bool,
}

impl From<OutputOptionArgs> for OutputOptions {
    fn from(value: OutputOptionArgs) -> Self {
        Self {
            content_mode: value.content_mode.unwrap_or_default(),
            output_format: value.output_format,
            header: !value.no_header,
            escape: !value.no_escape,
        }
    }
}

#[derive(Debug, Parser)]
pub struct ResetArgs {
    #[clap(long, short)]
    pub bbox: Option<DegreeBbox>,

    /// Use provided numeric keys instead of auto-increment.
    ///
    /// When this flag is set, clients must provide a numeric key with each feature.
    #[clap(long, conflicts_with = "key_bytes")]
    pub provided_keys: bool,

    /// Use an arbitrary field from metadata as a custom 16-byte key.
    ///
    /// The field must be less than or equal to 16 bytes long or operations will fail. This is intended for use with
    /// alphanumeric identifiers. The argument takes a string that contains the JSON Pointer definition to a string
    /// field.
    #[clap(long)]
    pub byte_keys: Option<String>,

    #[clap(long, short)]
    pub force: bool,
}

/// Get newline-delimited JSON for the passed list of ids.
///
/// Results will be pushed to stdout, errors to stderr. Use the meta only flag to only return the associated metadata
/// and not the full feature.
/// TODO: Fix all these descriptions, this currently doesn't give you ndjson unlike what the description indicates
#[derive(Debug, Parser)]
pub struct GetArgs {
    /// Whether the passed keys are custom byte keys.
    #[clap(long, short = 'y')]
    pub key_bytes: bool,

    /// Manually passed comma-separated keys to get.
    #[clap(long, short, conflicts_with = "input")]
    pub data: Option<String>,

    /// Input file containing keys to get. If not provided, reads from stdin.
    #[clap(long, short)]
    pub input: Option<PathBuf>,

    /// Output file for results. If not provided, writes to stdout.
    #[clap(long, short)]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    output_options: OutputOptionArgs,
}

impl GetArgs {
    pub fn output_options(&self) -> OutputOptions {
        let mut opts: OutputOptions = self.output_options.into();

        opts.content_mode = self
            .output_options
            .content_mode
            .unwrap_or(ContentMode::Full);

        opts
    }
}

#[derive(Debug, Parser)]
pub struct DeleteArgs {
    /// Whether the passed keys are custom byte keys.
    #[clap(long, short = 'y')]
    pub key_bytes: bool,

    /// Manually passed comma-separated keys to remove.
    #[clap(long, short, conflicts_with = "input")]
    pub data: Option<String>,

    /// An optional file input containing keys to remove.
    pub input: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct KnnArgs {
    /// The number of nearest neighbors to find.
    #[clap(short)]
    pub k: usize,

    /// Maximum seach radius to truncate the search in meters.
    #[clap(short)]
    pub r: Option<f64>,

    /// Flag to determine whether we are trying to test uid keys instead of geometries.
    ///
    /// When testing with keys, this indicates to the server that the test geometry is already in the quadtree with the
    /// given set of keys. This flag governs the type of data expected as data, as the file, or on stdin.
    #[clap(long, short = 'u')]
    pub key_uid: bool,

    /// Flag to determine whether to use custom byte keys instead of geometries.
    #[clap(long, short = 'y', conflicts_with = "key_uid")]
    pub key_bytes: bool,

    /// Manually passed data to test.
    #[clap(long, short, conflicts_with = "input")]
    pub data: Option<String>,

    /// Input file containing items to test. If not provided, reads from stdin.
    #[clap(long, short)]
    pub input: Option<PathBuf>,

    /// Output file for results. If not provided, writes to stdout.
    #[clap(long, short)]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    output_options: OutputOptionArgs,
}

impl KnnArgs {
    /// Create a new stub for KnnArgs.
    ///
    /// WARN: Be careful with this method!
    ///
    /// This method is only provided as a plug to enable making a KnnArgs struct without exposing the private
    /// OutputOptionArgs struct or confusing access to output only. It will not enforce any business rules.
    ///
    /// TODO: Create better arg translation that will enforce business rules
    pub fn new(opts: OutputOptions) -> Self {
        Self {
            k: 0,
            r: None,
            key_uid: false,
            key_bytes: false,
            data: None,
            input: None,
            output: None,
            output_options: OutputOptionArgs {
                content_mode: Some(opts.content_mode),
                output_format: opts.output_format,
                no_header: !opts.header,
                no_escape: !opts.escape,
            },
        }
    }

    pub fn output_options(&self) -> OutputOptions {
        self.output_options.into()
    }
}

#[derive(Debug, Parser)]
pub struct ReplArgs {
    /// Output file for REPL results. If not provided, writes to stdout.
    #[clap(long, short)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct WindowArgs {
    /// The bounding box to query.
    pub bbox: DegreeBbox,

    /// The default mode is contains, where it will return only items completely contained by the bbox, but passing
    /// this flag sets intersects mode, where anything that intersects the bbox is returned.
    #[clap(long, short)]
    pub intersects: bool,

    /// Output file for results. If not provided, writes to stdout.
    #[clap(long, short)]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    output_options: OutputOptionArgs,
}

impl WindowArgs {
    pub fn output_options(&self) -> OutputOptions {
        self.output_options.into()
    }
}

// TODO: Is there some way we can move this out of here and bypass the CLI - maybe with a feature
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
