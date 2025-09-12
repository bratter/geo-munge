//! Geo-munge networking library.
//!
//! Contains logic for inter-process communication, common offloaded io, and other shared low-level constructs.
//!
//! Geo-munge uses a single-threaded, non-blocking io model based on the [`mio`] crate that dispatches messages to the
//! rest of the respective application using channels.

pub mod client;
pub mod connection;
pub mod server;
pub mod signals;

// TODO: Can we remove or change these?
#[cfg(unix)]
const UNIX_SOCKET_NAME: &str = "/tmp/gm-proximity";
#[cfg(windows)]
const TCP_SOCKET_ADDR: &str = "127.0.0.1:6378";
