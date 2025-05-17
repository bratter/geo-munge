//! Connection wrapper for working with the Mio-based event loop on both client and server.

use std::io::{ErrorKind, Read, Write};

use anyhow::{anyhow, Result};
use mio::{net::UnixStream, Interest, Poll, Token};

use crate::message::prelude::*;

// TODO: Is it worth having some mechanism to prevent dropping a connection on a different pool? Probably not as this is
// the only user
// TODO: When writing, think about a backpressure mechanism - track the outgoing buffer size and if it gets too big, put
// a flag in the connection that skips the read - READABLE should still keep coming, but it backpressures the reads
// TODO: Put some buffer instrumentation in to keep track of buffer sizes
pub struct ConnectionPool<const N: usize> {
    pool: [Option<Connection>; N],
}

impl<const N: usize> ConnectionPool<N> {
    pub const fn new() -> Self {
        const NONE: Option<Connection> = None;

        Self { pool: [NONE; N] }
    }

    pub fn get_mut<'a>(&'a mut self, token: Token) -> Option<&'a mut Connection> {
        self.pool.get_mut(token.0)?.as_mut()
    }

    /// Try to register and store an incoming stream.
    ///
    /// First checks if a connection slot is available, rejecting if not. Then tries to register with the poll. If
    /// either fail returns the stream to give the caller the ability to handle (e.g., retry) rather than dropping the
    /// connection itself.
    pub fn register<'a>(&'a mut self, poll: &mut Poll, mut stream: UnixStream) -> AddResult {
        match self.find_slot_idx() {
            Some(idx) => {
                match poll.registry().register(
                    &mut stream,
                    Token(idx),
                    Interest::READABLE | Interest::WRITABLE,
                ) {
                    Ok(_) => {
                        self.pool[idx] = Some(Connection::new_server(Token(idx), stream));
                        AddResult::Success
                    }
                    Err(_) => AddResult::RegistrationFailure(stream),
                }
            }
            None => AddResult::NoSpace(stream),
        }
    }

    /// Drop the connection and free space in the connection pool.
    ///
    /// It should be ok to drop without deregistering, and token reuse is fine.
    /// TODO: This currently ignores out of range or non-existent connections, should it at least report an error?
    /// TODO: Mio docs say this should be deregistered - pass the poll/registry and deregister in here
    pub fn cleanup(&mut self, poll: &mut Poll, token: Token) {
        match self.pool.get_mut(token.0) {
            Some(conn) => {
                if let Some(mut conn) = std::mem::take(conn) {
                    // TODO: What should we do if deregister fails?
                    let _ = poll.registry().deregister(&mut conn.stream);
                }
            }
            None => {}
        }
    }

    fn find_slot_idx(&self) -> Option<usize> {
        self.pool
            .iter()
            .enumerate()
            .find_map(|(idx, slot)| if slot.is_none() { Some(idx) } else { None })
    }
}

pub enum AddResult {
    Success,
    NoSpace(UnixStream),
    RegistrationFailure(UnixStream),
}

pub struct Connection {
    token: Token,
    stream: UnixStream,
    kind: ConnectionKind,
    read_state: ConnectionReadState,
}

impl Connection {
    /// Make a new Connection.
    ///
    /// This is private as making a client and a server are slightly different.
    /// When inside a [`ConnectionPool`] new shouldn't be used.
    fn new(token: Token, stream: UnixStream, kind: ConnectionKind) -> Self {
        Self {
            token,
            stream,
            kind,
            read_state: ConnectionReadState::default(),
        }
    }

    /// Make a new server connection. This should only be called inside a conenction pool.
    fn new_server(token: Token, stream: UnixStream) -> Self {
        Self::new(token, stream, ConnectionKind::Server)
    }

    /// Create a new connection from the provided stream, and register with the poll.
    pub fn new_client(poll: &mut Poll, token: Token, mut stream: UnixStream) -> Result<Self> {
        poll.registry()
            .register(&mut stream, token, Interest::READABLE | Interest::WRITABLE)?;

        Ok(Self::new(token, stream, ConnectionKind::Client))
    }

    pub fn token(&self) -> Token {
        self.token
    }

    pub fn enable_write_interest(&mut self, poll: &mut Poll) -> Result<()> {
        poll.registry().reregister(
            &mut self.stream,
            self.token,
            Interest::READABLE | Interest::WRITABLE,
        )?;
        Ok(())
    }

    pub fn disable_write_interest(&mut self, poll: &mut Poll) -> Result<()> {
        poll.registry()
            .reregister(&mut self.stream, self.token, Interest::READABLE)?;
        Ok(())
    }

    /// Read off the incoming stream and buffer into [`Request`]s.
    ///
    /// The connection decodes the incoming stream into [`Request`] objects, but doesn't dispatch them anywhere or
    /// handle errors/EOFs - this is up for the calling code to do - although we do extract and report WouldBlock to
    /// make the API simpler.
    pub fn read(&mut self) -> ReadResult {
        match &mut self.read_state {
            ConnectionReadState::Length {
                len_buf,
                bytes_read,
            } => match self.stream.read(&mut len_buf[*bytes_read..]) {
                Ok(0) if *bytes_read == 0 => ReadResult::Eof,
                Ok(0) => ReadResult::Error(anyhow!("Unexpected EOF")),
                Ok(n) => {
                    *bytes_read += n;
                    if *bytes_read == 4 {
                        // When we are transitioning states, we pre-prepare a correctly sized vector that has been set
                        // with resize to ensure that reading into it works correctly and requires no further
                        // allocations for this message; afterwards the len is encoded in the Vec so doesn't need to be
                        // retained
                        // TODO: What to do if this is zero?
                        let len = u32::from_le_bytes(*len_buf) as usize;
                        let mut buf = Vec::with_capacity(len);
                        buf.resize(len, 0);
                        self.read_state = ConnectionReadState::Body { buf, bytes_read: 0 };
                    }
                    ReadResult::Continue
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => ReadResult::WouldBlock,
                Err(err) => ReadResult::Error(anyhow![err]),
            },

            ConnectionReadState::Body { buf, bytes_read } => {
                match self.stream.read(&mut buf[*bytes_read..]) {
                    Ok(0) => ReadResult::Error(anyhow!("Unexpected EOF")),
                    Ok(n) => {
                        *bytes_read += n;
                        if *bytes_read == buf.len() {
                            // When we've filled the buffer, we should have a complete message, so decode and send on;
                            // also reset the state machine in preparation for the next message by setting back to its
                            // default, which will also drop the buf vector
                            // The type of decode we attempt depends on whether this is a server or a client connection.
                            let res = match Request::decode(&buf) {
                                Ok(request) => ReadResult::Request(request),
                                Err(err) => ReadResult::Error(err),
                            };
                            self.read_state = ConnectionReadState::default();
                            res
                        } else {
                            ReadResult::Continue
                        }
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => ReadResult::WouldBlock,
                    Err(err) => ReadResult::Error(anyhow!(err)),
                }
            }
        }
    }

    fn decode(&self, buf: &[u8]) -> ReadResult {
        match self.kind {
            ConnectionKind::Server => match Request::decode(buf) {
                Ok(req) => ReadResult::Request(req),
                Err(err) => ReadResult::Error(err),
            },
            ConnectionKind::Client => match Response::decode(buf) {
                Ok(res) => ReadResult::Response(res),
                Err(err) => ReadResult::Error(err),
            },
        }
    }
}

// TODO: Consider simplifying the error types
pub enum ReadResult {
    Request(Request),
    Response(Response),
    Continue,
    WouldBlock,
    Eof,
    Error(anyhow::Error),
}

enum ConnectionReadState {
    Length { len_buf: [u8; 4], bytes_read: usize },
    Body { buf: Vec<u8>, bytes_read: usize },
}

impl Default for ConnectionReadState {
    fn default() -> Self {
        ConnectionReadState::Length {
            len_buf: [0u8; 4],
            bytes_read: 0,
        }
    }
}

/// The kind of the [`Connection`].
///
/// A server conenction will read [`Request`]s and write [`Response`]s, while the client will do the opposite.
enum ConnectionKind {
    Server,
    Client,
}

// TODO: These will likely go away and be replaced with a custom encoder/decoder
impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}
