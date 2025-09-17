use clap::Parser;

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
}
