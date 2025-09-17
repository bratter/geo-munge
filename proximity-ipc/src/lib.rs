//! Common functionality between IPC-driven proximity server and clients.
//!
//! Ensures that common constants are defined and encode/decode paths are implemented.

pub mod batch;
pub mod channel;
pub mod codec;

use crossbeam::channel::Sender;
// Re-export the network stuff
pub use network::signals::{set_ctrlc_handler, RunToken};

#[cfg(unix)]
/// The default Unix socket name to use for a proximity server/client.
pub const DEFAULT_UNIX_SOCKET_NAME: &str = "/tmp/gm-proximity";

/// The default TCP socket to use for a proximity server/client.
pub const DEFAULT_TCP_SOCKET_ADDR: &str = "127.0.0.1:6378";

/// Application context. Works for both servers and clients.
pub struct Context<C> {
    /// Config struct specific to the application.
    pub config: C,

    /// An optional signal to use that notifies the receiving end the the io event loop is about to start.
    pub ready: Option<Sender<()>>,

    /// Shared system status token.
    pub running: RunToken,
}
