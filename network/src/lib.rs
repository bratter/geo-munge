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

use anyhow::Result;

#[cfg(unix)]
const DEFAULT_UNIX_SOCKET_NAME: &str = "/tmp/net_lib_socket";
#[cfg(windows)]
const DEFAULT_TCP_SOCKET_ADDR: &str = "127.0.0.1:6378";

/// Codec trait to ensure that the connection can serialize and deserialize types on the wire
pub trait IoCodec
where
    Self: Sized,
{
    fn decode_from_slice(buf: &[u8]) -> Result<Self>;

    fn encode_to_vec(&self) -> Result<Vec<u8>>;
}
