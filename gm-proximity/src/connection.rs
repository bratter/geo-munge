//! Connection wrapper for working with the Mio-based event loop on both client and server.

use std::{
    collections::VecDeque,
    io::{ErrorKind, Read, Write},
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::{anyhow, bail, Result};
use crossbeam::channel::{Receiver, TryRecvError};
use mio::{event::Source, Interest, Poll, Token};

use crate::{message::prelude::*, MAX_CONNECTIONS};

// TODO: Put some buffer instrumentation in to keep track of buffer sizes
pub struct ConnectionPool<'a, S> {
    pool: Vec<Option<Connection<'a, S, Response>>>,
    write_queue_soft_cap: usize,
    next_conn_id: u32,
    traffic: &'a Traffic,
}

impl<'a, S: Source + Read + Write> ConnectionPool<'a, S> {
    pub fn new(size: usize, traffic: &'a Traffic, write_queue_soft_cap: usize) -> Result<Self> {
        if size > MAX_CONNECTIONS {
            bail!(
                "Attempted to create a {} connection pool, but max size is {}",
                size,
                MAX_CONNECTIONS
            );
        }

        let mut pool = Vec::with_capacity(size);
        pool.resize_with(size, Default::default);

        Ok(Self {
            pool,
            traffic,
            write_queue_soft_cap,
            next_conn_id: 0,
        })
    }

    pub fn get_mut_by_msg(&mut self, token: MsgToken) -> Option<&mut Connection<'a, S, Response>> {
        self.pool
            .iter_mut()
            .find(|conn| match conn {
                Some(conn) => conn.id == token.conn_id,
                None => false,
            })?
            .as_mut()
    }

    pub fn get_mut_by_conn(&mut self, token: Token) -> Option<&mut Connection<'a, S, Response>> {
        self.pool.get_mut(token.0)?.as_mut()
    }

    /// Try to register and store an incoming stream.
    ///
    /// First checks if a connection slot is available, rejecting if not. Then tries to register with the poll. If
    /// either fail returns the stream to give the caller the ability to handle (e.g., retry) rather than dropping the
    /// connection itself.
    pub fn register(&mut self, poll: &mut Poll, mut stream: S) -> AddResult<S> {
        match self.find_slot_idx() {
            Some(idx) => {
                match poll.registry().register(
                    &mut stream,
                    Token(idx),
                    Interest::READABLE | Interest::WRITABLE,
                ) {
                    Ok(_) => {
                        let conn = Connection::new_server(
                            self.next_conn_id,
                            Token(idx),
                            stream,
                            Some(self.traffic),
                            self.write_queue_soft_cap,
                        );

                        self.pool[idx] = Some(conn);
                        let result = AddResult::Success(self.next_conn_id);
                        // Must increment the connection identifier
                        self.next_conn_id += 1;
                        result
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
    #[tracing::instrument(skip_all)]
    pub fn cleanup(&mut self, poll: &mut Poll, token: Token) {
        match self.pool.get_mut(token.0).and_then(std::mem::take) {
            Some(mut conn) => {
                // TODO: Should we do anything else if deregister fails?
                if let Err(err) = poll.registry().deregister(&mut conn.stream) {
                    tracing::error!("Failed to deregister {}", err);
                }
            }
            None => tracing::warn!(
                "Attempting to clean up non-existent connection with {:?}",
                token
            ),
        }
    }

    /// Pull all available responses out of the channel and assign them to their appropriate connection.
    ///
    /// This runs until empty.
    #[tracing::instrument(skip_all)]
    pub fn fill_write_queues(
        &mut self,
        response_rx: &Receiver<(MsgToken, Response)>,
        poll: &mut Poll,
    ) -> Result<()> {
        loop {
            match response_rx.try_recv() {
                Ok((msg_token, res)) => match self.get_mut_by_msg(msg_token) {
                    Some(conn) => {
                        if !conn.is_writeable() {
                            tracing::trace!("Enabling writes on {}", conn.id);
                            conn.enable_interest(poll, Interest::WRITABLE)?;
                        }
                        if conn.push_write_queue((msg_token.msg_id, res)) == WriteQueueState::Full
                            && conn.is_readable()
                        {
                            tracing::trace!("Write queue full for connection {}", conn.id);
                            conn.disable_interest(poll, Interest::READABLE)?;
                        }
                    }
                    None => tracing::warn!("Connection {} no longer exists", msg_token.conn_id),
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    tracing::error!("Response channel disconnected");
                    bail!(TryRecvError::Disconnected);
                }
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

pub enum AddResult<S> {
    Success(u32),
    #[allow(dead_code)]
    NoSpace(S),
    #[allow(dead_code)]
    RegistrationFailure(S),
}

pub struct Connection<'a, S, T: IoEncode> {
    id: u32,
    token: Token,
    stream: S,
    kind: ConnectionKind,
    interests: Option<Interest>,
    read_state: ReadState,
    write_state: WriteState,
    write_queue: VecDeque<(u32, T)>,
    traffic: Option<&'a Traffic>,
    write_queue_soft_cap: usize,
}

impl<'a, S: Source + Read + Write, T: IoEncode> Connection<'a, S, T> {
    /// Make a new Connection.
    ///
    /// This is private as making a client and a server are slightly different.
    /// When inside a [`ConnectionPool`] new shouldn't be used.
    fn new(
        id: u32,
        token: Token,
        stream: S,
        kind: ConnectionKind,
        traffic: Option<&'a Traffic>,
        write_queue_soft_cap: usize,
    ) -> Self {
        Self {
            id,
            token,
            stream,
            kind,
            interests: Some(Interest::READABLE | Interest::WRITABLE),
            read_state: ReadState::default(),
            write_state: WriteState::default(),
            write_queue: VecDeque::default(),
            traffic,
            write_queue_soft_cap,
        }
    }

    /// Make a new server connection. This should only be called inside a conenction pool.
    fn new_server(
        id: u32,
        token: Token,
        stream: S,
        traffic: Option<&'a Traffic>,
        write_queue_soft_cap: usize,
    ) -> Self {
        Self::new(
            id,
            token,
            stream,
            ConnectionKind::Server,
            traffic,
            write_queue_soft_cap,
        )
    }

    /// Create a new connection from the provided stream, and register with the poll.
    pub fn new_client(
        poll: &mut Poll,
        token: Token,
        mut stream: S,
        traffic: Option<&'a Traffic>,
    ) -> Result<Self> {
        poll.registry()
            .register(&mut stream, token, Interest::READABLE | Interest::WRITABLE)?;

        Ok(Self::new(
            0,
            token,
            stream,
            ConnectionKind::Client,
            traffic,
            // No soft cap on clients
            std::usize::MAX,
        ))
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn msg_token(&self, msg_id: u32) -> MsgToken {
        MsgToken::new(self.id, msg_id)
    }

    pub fn is_readable(&self) -> bool {
        match self.interests {
            Some(i) => i.is_readable(),
            None => false,
        }
    }

    pub fn is_writeable(&self) -> bool {
        match self.interests {
            Some(i) => i.is_writable(),
            None => false,
        }
    }

    // TODO: This logic is quite complex. Check to see that we don't miss events in testing due to epoll behavior
    // TODO: Consider doing the logic checks here as to whether we call register or not - register is not cheap
    pub fn enable_interest(&mut self, poll: &mut Poll, interest: Interest) -> Result<()> {
        tracing::trace!("Enabling interest {:?}", interest);
        if let Some(interests) = self.interests {
            let interests = interests.add(interest);
            self.interests = Some(interests);
            poll.registry()
                .reregister(&mut self.stream, self.token, interests)?;
        } else {
            self.interests = Some(interest);
            poll.registry()
                .register(&mut self.stream, self.token, interest)?;
        }
        Ok(())
    }

    pub fn disable_interest(&mut self, poll: &mut Poll, interest: Interest) -> Result<()> {
        tracing::trace!("Disabling interest {:?}", interest);
        // The connection interest tracking must be up to date or this won't work
        if let Some(interests) = self.interests {
            match interests.remove(interest) {
                Some(interests) => {
                    self.interests = Some(interests);
                    poll.registry()
                        .reregister(&mut self.stream, self.token, interests)?
                }
                None => {
                    self.interests = None;
                    poll.registry().deregister(&mut self.stream)?;
                }
            }
        }
        Ok(())
    }

    pub fn write_queue_state(&self) -> WriteQueueState {
        let len = self.write_queue.len();
        if len >= self.write_queue_soft_cap {
            WriteQueueState::Full
        } else if len >= self.write_queue_soft_cap << 1 {
            WriteQueueState::Draining
        } else {
            WriteQueueState::Clear
        }
    }

    /// Push a message onto the queue for writing.
    ///
    /// This queue is soft capped, and will return a [`WriteQueueCapacity::Full`] when it is at its intended capacity.
    /// Callers should use this hint to throttle appropriately.
    pub fn push_write_queue(&mut self, msg_with_id: (u32, T)) -> WriteQueueState {
        self.write_queue.push_back(msg_with_id);
        self.write_queue_state()
    }

    /// Read off the incoming stream and buffer into [`Request`]s or [`Response`]s.
    ///
    /// The connection decodes the incoming stream into objects, but doesn't dispatch them anywhere or handle
    /// errors/EOFs - this is up for the calling code to do - although we do extract and report WouldBlock to
    /// make the API simpler.
    /// TODO: Is there some easy way of pulling out all domain specific reading and writing into a trait, not just the
    /// decode and encode? This way the whole io loop is reuseable
    pub fn read(&mut self) -> ReadResult {
        match &mut self.read_state {
            ReadState::Header {
                header_buf,
                bytes_read,
            } => match self.stream.read(&mut header_buf[*bytes_read..]) {
                Ok(0) if *bytes_read == 0 => ReadResult::Eof,
                Ok(0) => ReadResult::Error(anyhow!("Unexpected EOF")),
                Ok(n) => {
                    self.traffic.map(|t| t.add_recv(n));
                    *bytes_read += n;
                    if *bytes_read == 8 {
                        // When we are transitioning states, we pre-prepare a correctly sized vector that has been set
                        // with resize to ensure that reading into it works correctly and requires no further
                        // allocations for this message; afterwards the len is encoded in the Vec so doesn't need to be
                        // retained
                        // TODO: What to do if this is zero?
                        let len = u32_from_le_slice(&header_buf[..4]) as usize;
                        let msg_id = u32_from_le_slice(&header_buf[4..]);
                        let mut buf = Vec::with_capacity(len);
                        buf.resize(len, 0);

                        self.read_state = ReadState::Body {
                            buf,
                            msg_id,
                            bytes_read: 0,
                        };
                    }
                    ReadResult::Continue
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => ReadResult::WouldBlock,
                Err(err) => ReadResult::Error(anyhow![err]),
            },

            ReadState::Body {
                buf, bytes_read, ..
            } => {
                match self.stream.read(&mut buf[*bytes_read..]) {
                    Ok(0) => ReadResult::Error(anyhow!("Unexpected EOF")),
                    Ok(n) => {
                        self.traffic.map(|t| t.add_recv(n));
                        *bytes_read += n;
                        if *bytes_read == buf.len() {
                            // When we've filled the buffer, we should have a complete message, so decode and send on;
                            // also reset the state machine in preparation for the next message by setting back to its
                            // default, which will also drop the buf vector - take() does this cleanly with ownership
                            // The type of decode we attempt depends on whether this is a server or a client connection.
                            if let ReadState::Body { buf, msg_id, .. } =
                                std::mem::take(&mut self.read_state)
                            {
                                self.decode(msg_id, &buf)
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
                        Err(err) => WriteResult::Error(err),
                    }
                } else {
                    WriteResult::Drained
                }
            }

            // Write the length, then when finished pass the buffer over to body writing
            WriteState::Header {
                header_buf: len,
                buf,
                bytes,
            } => match self.stream.write(&len[*bytes..]) {
                Ok(0) if *bytes == 0 => WriteResult::Eof,
                Ok(0) => WriteResult::Error(anyhow!("Unexpected EOF")),
                Ok(n) => {
                    self.traffic.map(|t| t.add_send(n));
                    *bytes += n;
                    if *bytes == 8 {
                        let buf = std::mem::take(buf);
                        self.write_state = WriteState::Body { buf, bytes: 0 };
                    }
                    WriteResult::Continue
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => WriteResult::WouldBlock,
                Err(err) => WriteResult::Error(anyhow![err]),
            },

            WriteState::Body { buf, bytes } => match self.stream.write(&buf[*bytes..]) {
                Ok(0) => WriteResult::Error(anyhow!("Unexpected EOF")),
                Ok(n) => {
                    self.traffic.map(|t| t.add_send(n));
                    *bytes += n;
                    if *bytes == buf.len() {
                        self.write_state = WriteState::Awaiting;
                    }
                    WriteResult::Continue
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => WriteResult::WouldBlock,
                Err(err) => WriteResult::Error(anyhow![err]),
            },
        }
    }

    /// Write into the outgoing stream directly from a channel.
    ///
    /// This method doesn't buffer in the write queue, therefore avoiding multiple allocations. Otherwise it behaves the
    /// same as the standard write.
    pub fn write_from_channel(&mut self, channel: &Receiver<(u32, T)>) -> WriteResult {
        match self.write_state {
            WriteState::Awaiting => match channel.try_recv() {
                Ok(msg) => match self.encode(msg) {
                    Ok(state) => {
                        self.write_state = state;
                        WriteResult::Continue
                    }
                    Err(err) => WriteResult::Error(err),
                },
                Err(TryRecvError::Empty) => WriteResult::Drained,
                Err(TryRecvError::Disconnected) => WriteResult::Disconnected,
            },
            // Because write is being called here, we intercept drained as we are not using the queue and only pass it
            // on it the channel is empty, otherwise we go around again
            _ => match self.write() {
                WriteResult::Drained if !channel.is_empty() => WriteResult::Continue,
                wr => wr,
            },
        }
    }

    fn decode(&self, msg_id: u32, buf: &[u8]) -> ReadResult {
        match self.kind {
            ConnectionKind::Server => match Request::decode_from_slice(buf) {
                Ok(req) => ReadResult::Request((msg_id, req)),
                Err(err) => ReadResult::Error(err),
            },
            ConnectionKind::Client => match Response::decode_from_slice(buf) {
                Ok(res) => ReadResult::Response((msg_id, res)),
                Err(err) => ReadResult::Error(err),
            },
        }
    }

    fn encode(&self, (msg_id, msg): (u32, T)) -> Result<WriteState> {
        let buf = msg.encode_to_vec()?;
        let len = u32::to_le_bytes(buf.len() as u32);
        let msg_id = u32::to_le_bytes(msg_id);
        let mut header_buf = [0u8; 8];

        header_buf[0..4].copy_from_slice(&len);
        header_buf[4..8].copy_from_slice(&msg_id);

        Ok(WriteState::Header {
            header_buf,
            buf,
            bytes: 0,
        })
    }
}

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
}

pub enum ReadResult {
    Request((u32, Request)),
    Response((u32, Response)),
    Continue,
    WouldBlock,
    /// Expected EOF, unexpected will be returned as errors.
    Eof,
    Error(anyhow::Error),
}

pub enum WriteResult {
    Continue,
    Drained,
    WouldBlock,
    /// For a direct-from-channel write indicates that that channel has been disconnected.
    Disconnected,
    /// Expected EOF, unexpected will be returned as errors.
    Eof,
    Error(anyhow::Error),
}

enum ReadState {
    Header {
        header_buf: [u8; 8],
        bytes_read: usize,
    },
    Body {
        buf: Vec<u8>,
        msg_id: u32,
        bytes_read: usize,
    },
}

impl Default for ReadState {
    fn default() -> Self {
        ReadState::Header {
            header_buf: [0u8; 8],
            bytes_read: 0,
        }
    }
}

#[derive(Default)]
enum WriteState {
    #[default]
    Awaiting,
    Header {
        header_buf: [u8; 8],
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

#[derive(Clone, Copy, PartialEq)]
pub enum WriteQueueState {
    /// Queue is equal to or above soft cap, take action to throttle.
    Full,

    /// Queue is between clear and full, no actions required.
    Draining,

    /// Queue is below threshold, take action to unthrottle.
    Clear,
}

/// Global tracker for traffic on a [`ConnectionPool`].
///
/// Uses [`AtomicUsize`] internally to enable use through shared references.
#[derive(Default)]
pub struct Traffic {
    sent: AtomicUsize,
    recv: AtomicUsize,
}

impl Traffic {
    pub fn sent(&self) -> usize {
        self.sent.load(Ordering::Relaxed)
    }

    pub fn recv(&self) -> usize {
        self.recv.load(Ordering::Relaxed)
    }

    /// Add sent traffic in bytes.
    ///
    /// Uses [`AtomicUsize::fetch_add`], so returns the previous value.
    pub fn add_send(&self, bytes: usize) -> usize {
        self.sent.fetch_add(bytes, Ordering::Relaxed)
    }

    /// Add received traffic in bytes.
    ///
    /// Uses [`AtomicUsize::fetch_add`], so returns the previous value.
    pub fn add_recv(&self, bytes: usize) -> usize {
        self.recv.fetch_add(bytes, Ordering::Relaxed)
    }
}

/// Convert a byte slice to a u32. The bytes must be the correct length or will panic.
fn u32_from_le_slice(slice: &[u8]) -> u32 {
    let arr = slice.try_into().expect("Exact size provided");
    u32::from_le_bytes(arr)
}
