use clap::Parser;
use protocol::prelude::*;

/// Start a proximity server instance.
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

    #[command(flatten)]
    pub initial_config: InitialConfig,
}

/// Initial server configuration options
#[derive(Debug, Default, Parser)]
pub struct InitialConfig {
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
}
