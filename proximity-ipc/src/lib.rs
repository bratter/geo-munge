//! Common functionality between IPC-driven proximity server and clients.
//!
//! Ensures that common constants are defined and encode/decode paths are implemented.

pub mod channel;
pub mod codec;

#[cfg(unix)]
/// The default Unix socket name to use for a proximity server/client.
pub const DEFAULT_UNIX_SOCKET_NAME: &str = "/tmp/gm-proximity";

#[cfg(windows)]
/// The default TCP socket to use for a proximity server/client.
pub const DEFAULT_TCP_SOCKET_ADDR: &str = "127.0.0.1:6378";
