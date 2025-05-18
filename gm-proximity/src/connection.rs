//! Connection wrapper for working with the Mio-based event loop on both client and server.

use std::{
    collections::VecDeque,
    io::{ErrorKind, Read, Write},
    sync::mpsc::{Receiver, TryRecvError},
};

use anyhow::{anyhow, bail, Result};
use mio::{net::UnixStream, Interest, Poll, Token};

use crate::message::prelude::*;

// TODO: Is it worth having some mechanism to prevent dropping a connection on a different pool? Probably not as this is
// the only user
// TODO: When writing, think about a backpressure mechanism - track the outgoing buffer size and if it gets too big, put
// a flag in the connection that skips the read - READABLE should still keep coming, but it backpressures the reads
// TODO: Put some buffer instrumentation in to keep track of buffer sizes
// TODO: Should not reuse ConnTokens as connections could easily be replaced, this does mean we can't just index into
// the ID, but we can make the ConnId the index and get the incrementing token from that. Probably better to keep the
// Tokens the same but increment the ConnToken, because there are no issues with keepin the Tokens static
pub struct ConnectionPool<const N: usize> {
    pool: [Option<Connection<Response>>; N],
}

impl<const N: usize> ConnectionPool<N> {
    pub const fn new() -> Self {
        const NONE: Option<Connection<Response>> = None;

        Self { pool: [NONE; N] }
    }

    pub fn get_mut<'a>(&'a mut self, token: ConnToken) -> Option<&'a mut Connection<Response>> {
        self.pool.get_mut(token.0)?.as_mut()
    }

    // TODO: If we rotate mio tokens then this will need to change to finding in the pool
    pub fn get_mut_from_mio<'a>(
        &'a mut self,
        token: Token,
    ) -> Option<&'a mut Connection<Response>> {
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
    /// Mio docs recommend deregistering for cleanup, but re-registering Tokens is fine.
    /// TODO: This currently ignores out of range or non-existent connections, should it at least report an error?
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

    /// Pull all available responses out of the channel and assign them to their appropriate connection.
    ///
    /// This runs until empty.
    pub fn fill_write_queues(
        &mut self,
        response_rx: &Receiver<(ConnToken, Response)>,
        poll: &mut Poll,
    ) -> Result<()> {
        loop {
            match response_rx.try_recv() {
                Ok((token, res)) => match self.get_mut(token) {
                    Some(conn) => {
                        conn.push_write_queue(res);
                        conn.enable_write_interest(poll)?;
                    }
                    None => eprintln!("Connection {} no longer exists", token.0),
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => bail!("Response channel disconnected"),
            }
        }

        Ok(())
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

pub struct Connection<T: MessageStream> {
    token: Token,
    stream: UnixStream,
    kind: ConnectionKind,
    read_state: ReadState,
    write_state: WriteState,
    write_queue: VecDeque<T>,
}

impl<T: MessageStream> Connection<T> {
    /// Make a new Connection.
    ///
    /// This is private as making a client and a server are slightly different.
    /// When inside a [`ConnectionPool`] new shouldn't be used.
    fn new(token: Token, stream: UnixStream, kind: ConnectionKind) -> Self {
        Self {
            token,
            stream,
            kind,
            read_state: ReadState::default(),
            write_state: WriteState::default(),
            write_queue: VecDeque::default(),
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

    pub fn poll_token(&self) -> Token {
        self.token
    }

    pub fn token(&self) -> ConnToken {
        self.token.into()
    }

    pub fn enable_write_interest(&mut self, poll: &mut Poll) -> Result<()> {
        eprintln!("enabling write interest");
        poll.registry().reregister(
            &mut self.stream,
            self.token,
            Interest::READABLE | Interest::WRITABLE,
        )?;
        Ok(())
    }

    pub fn disable_write_interest(&mut self, poll: &mut Poll) -> Result<()> {
        eprintln!("disabling write interest");
        poll.registry()
            .reregister(&mut self.stream, self.token, Interest::READABLE)?;
        Ok(())
    }

    pub fn push_write_queue(&mut self, msg: T) {
        self.write_queue.push_back(msg);
    }

    /// Read off the incoming stream and buffer into [`Request`]s or [`Response`]s.
    ///
    /// The connection decodes the incoming stream into objects, but doesn't dispatch them anywhere or handle
    /// errors/EOFs - this is up for the calling code to do - although we do extract and report WouldBlock to
    /// make the API simpler.
    pub fn read(&mut self) -> ReadResult {
        match &mut self.read_state {
            ReadState::Length {
                len_buf,
                bytes_read,
            } => match self.stream.read(&mut len_buf[*bytes_read..]) {
                Ok(0) if *bytes_read == 0 => ReadResult::Eof,
                Ok(0) => ReadResult::Error(anyhow!("Unexpected EOF")),
                Ok(n) => {
                    eprintln!("reading length {}", n);
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
                        self.read_state = ReadState::Body { buf, bytes_read: 0 };
                    }
                    ReadResult::Continue
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => ReadResult::WouldBlock,
                Err(err) => ReadResult::Error(anyhow![err]),
            },

            ReadState::Body { buf, bytes_read } => {
                match self.stream.read(&mut buf[*bytes_read..]) {
                    Ok(0) => ReadResult::Error(anyhow!("Unexpected EOF")),
                    Ok(n) => {
                        *bytes_read += n;
                        if *bytes_read == buf.len() {
                            // When we've filled the buffer, we should have a complete message, so decode and send on;
                            // also reset the state machine in preparation for the next message by setting back to its
                            // default, which will also drop the buf vector - take() does this cleanly with ownership
                            // The type of decode we attempt depends on whether this is a server or a client connection.
                            if let ReadState::Body { buf, .. } =
                                std::mem::take(&mut self.read_state)
                            {
                                self.decode(&buf)
                            } else {
                                unreachable!()
                            }
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

    /// Write into the outgoing stream from either [`Request`]s or [`Response`]s in the send buffer.
    ///
    /// The connection encodes the object into a byte stream and writes them, tracking state appropriately, but does
    /// not handle errors/EOFs or manage registration interest changes - this must be done by the caller.
    ///
    /// A single call to write will only attempt to write one buffer then return. It is up to the caller to loop and
    /// call again until the data can no longer be sent.
    /// TODO: How do we handle write queue no longer full to re-enable reads if we are doing that? Maybe it will be
    /// better to just manage interests in this method itself?
    /// TODO: Any time through here we should be able to stop reading based on the write_queue size, acutally maybe
    /// either in here or when it hits a threshold in push_write_queue
    pub fn write(&mut self) -> WriteResult {
        match &mut self.write_state {
            // When there is currently nothing being actively written, we check the queue and process a new message if
            // there is one
            WriteState::Awaiting => {
                if let Some(msg) = self.write_queue.pop_front() {
                    match self.encode(msg) {
                        Ok(state) => {
                            self.write_state = state;
                            WriteResult::Continue
                        }
                        // TODO: Currently an encode error is a fatal error for the connection, is this too aggressive?
                        Err(err) => WriteResult::Error(err),
                    }
                } else {
                    WriteResult::Drained
                }
            }

            // Write the length, then when finished pass the buffer over to body writing
            WriteState::Length { len, buf, bytes } => match self.stream.write(&len[*bytes..]) {
                Ok(0) if *bytes == 0 => WriteResult::Eof,
                Ok(0) => WriteResult::Error(anyhow!("Unexpected EOF")),
                Ok(n) => {
                    eprintln!("should be in here {}", n);
                    *bytes += n;
                    if *bytes == 4 {
                        let buf = std::mem::take(buf);
                        self.write_state = WriteState::Body { buf, bytes: 0 };
                        eprintln!("written len");
                    }
                    WriteResult::Continue
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => WriteResult::WouldBlock,
                Err(err) => WriteResult::Error(anyhow![err]),
            },

            WriteState::Body { buf, bytes } => match self.stream.write(&buf[*bytes..]) {
                Ok(0) => WriteResult::Error(anyhow!("Unexpected EOF")),
                Ok(n) => {
                    *bytes += n;
                    if *bytes == buf.len() {
                        self.write_state = WriteState::Awaiting;
                        eprintln!("written body");
                    }
                    WriteResult::Continue
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => WriteResult::WouldBlock,
                Err(err) => WriteResult::Error(anyhow![err]),
            },
        }
    }

    fn decode(&self, buf: &[u8]) -> ReadResult {
        match self.kind {
            ConnectionKind::Server => match Request::decode_from_slice(buf) {
                Ok(req) => ReadResult::Request(req),
                Err(err) => ReadResult::Error(err),
            },
            ConnectionKind::Client => match Response::decode_from_slice(buf) {
                Ok(res) => ReadResult::Response(res),
                Err(err) => ReadResult::Error(err),
            },
        }
    }

    fn encode(&self, msg: T) -> Result<WriteState> {
        let buf = msg.encode_to_vec()?;
        let len = u32::to_le_bytes(buf.len() as u32);

        Ok(WriteState::Length { len, buf, bytes: 0 })
    }
}

/// An opaque token to track the connection of a request/response pair to enable sending back on the right connection.
#[derive(Clone, Copy)]
pub struct ConnToken(usize);

impl From<Token> for ConnToken {
    fn from(value: Token) -> Self {
        Self(value.0)
    }
}

impl From<ConnToken> for Token {
    fn from(value: ConnToken) -> Self {
        Self(value.0)
    }
}

pub enum ReadResult {
    Request(Request),
    Response(Response),
    Continue,
    WouldBlock,
    /// Expected EOF, unexpected will be returned as errors
    Eof,
    Error(anyhow::Error),
}

pub enum WriteResult {
    Continue,
    Drained,
    WouldBlock,
    /// Expected EOF, unexpected will be returned as errors
    Eof,
    Error(anyhow::Error),
}

enum ReadState {
    Length { len_buf: [u8; 4], bytes_read: usize },
    Body { buf: Vec<u8>, bytes_read: usize },
}

impl Default for ReadState {
    fn default() -> Self {
        ReadState::Length {
            len_buf: [0u8; 4],
            bytes_read: 0,
        }
    }
}

#[derive(Default)]
enum WriteState {
    #[default]
    Awaiting,
    Length {
        len: [u8; 4],
        // Generated on encode so stored here to pass to Body
        buf: Vec<u8>,
        bytes: usize,
    },
    Body {
        buf: Vec<u8>,
        bytes: usize,
    },
}

/// The kind of the [`Connection`].
///
/// A server conenction will read [`Request`]s and write [`Response`]s, while the client will do the opposite.
enum ConnectionKind {
    Server,
    Client,
}
