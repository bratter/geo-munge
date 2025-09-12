mod connection;
mod connection_pool;
mod traffic;

use protocol::prelude::IoCodec;

pub use connection::Connection;
pub use connection_pool::{AddStreamResult, ConnectionPool, MAX_POOL_CONNECTIONS};
pub use traffic::Traffic;

/// An opaque token to track the connection of a request/response pair to enable sending back on the right connection.
#[derive(Debug, Clone, Copy)]
pub struct MsgToken {
    conn_id: u32,
    msg_id: u32,
}

impl MsgToken {
    pub fn new(conn_id: u32, msg_id: u32) -> Self {
        Self { conn_id, msg_id }
    }

    pub fn msg(&self) -> u32 {
        self.msg_id
    }
}

// TODO: Try to get rid of this reference or have the trait in this crate?
pub enum ReadResult<T: IoCodec> {
    Message((MsgToken, T)),
    Continue,
    WouldBlock,
    SendFull,
    /// Expected EOF, unexpected will be returned as errors.
    Eof,
    Error(anyhow::Error),
}

pub enum WriteResult {
    Continue,
    Sent(MsgToken),
    Drained,
    WouldBlock,
    /// For a direct-from-channel write indicates that that channel has been disconnected.
    Disconnected,
    /// Expected EOF, unexpected will be returned as errors.
    Eof,
    Error(anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WriteQueueState {
    /// Queue is equal to or above soft cap, take action to throttle.
    Full,

    /// Queue is between clear and full, no actions required.
    Draining,

    /// Queue is below threshold, take action to unthrottle.
    Clear,
}
