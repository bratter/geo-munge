//! Connection pool for working with the Mio-based event loop on the server.

use std::{
    io::{Read, Write},
    usize,
};

use anyhow::{bail, Result};
use crossbeam::channel::{Receiver, Sender, TryRecvError};
use mio::{event::Source, Interest, Poll, Token};

use super::{Connection, IoCodec, MsgToken, ReadResult, Traffic, WriteQueueState};

/// The maximum connection pool size for client connections - required to ensure that the SERVER token stays separated
pub const MAX_POOL_CONNECTIONS: usize = 8;

pub enum AddStreamResult<S> {
    Success(u32),
    #[allow(dead_code)]
    NoSpace(S),
    #[allow(dead_code)]
    RegistrationFailure(S),
}

// TODO: Put some buffer instrumentation in to keep track of buffer sizes
// TODO: Could introduce a PoolConnection that gets returned instead of a Connection, that can also wrap a mutable
// reference to the pool to make working with the pool easier
// TODO: Consider disabled read tracking so we don't loop through the whole lot at the top of each io loop
pub struct ConnectionPool<'a, S, Req: IoCodec, Res: IoCodec> {
    pool: Vec<Option<Connection<'a, S, Res, Req>>>,
    write_queue_soft_cap: usize,
    next_conn_id: u32,
    traffic: &'a Traffic,
}

impl<'a, S: Source + Read + Write, Req: IoCodec, Res: IoCodec> ConnectionPool<'a, S, Req, Res> {
    pub fn new(size: usize, traffic: &'a Traffic, write_queue_soft_cap: usize) -> Result<Self> {
        if size > MAX_POOL_CONNECTIONS {
            bail!(
                "Attempted to create a {} connection pool, but max size is {}",
                size,
                MAX_POOL_CONNECTIONS
            );
        }

        let mut pool = Vec::with_capacity(size);
        pool.resize_with(size, Default::default);

        Ok(Self {
            pool,
            write_queue_soft_cap,
            next_conn_id: 0,
            traffic,
        })
    }

    pub fn get_mut_by_msg(&mut self, token: MsgToken) -> Option<&mut Connection<'a, S, Res, Req>> {
        self.pool
            .iter_mut()
            .find(|conn| match conn {
                Some(conn) => conn.id == token.conn_id,
                None => false,
            })?
            .as_mut()
    }

    pub fn get_mut_by_token(&mut self, token: Token) -> Option<&mut Connection<'a, S, Res, Req>> {
        self.pool.get_mut(token.0)?.as_mut()
    }

    /// Try to register and store an incoming stream.
    ///
    /// First checks if a connection slot is available, rejecting if not. Then tries to register with the poll. If
    /// either fail returns the stream to give the caller the ability to handle (e.g., retry) rather than dropping the
    /// connection itself.
    pub fn register(&mut self, poll: &mut Poll, mut stream: S) -> AddStreamResult<S> {
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
                        let result = AddStreamResult::Success(self.next_conn_id);
                        // Must increment the connection identifier
                        self.next_conn_id += 1;
                        result
                    }
                    Err(_) => AddStreamResult::RegistrationFailure(stream),
                }
            }
            None => AddStreamResult::NoSpace(stream),
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
        response_rx: &Receiver<(MsgToken, Res)>,
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
                        if conn.push_write_queue((msg_token, res)) == WriteQueueState::Full
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

    /// Loop through connections in the pool, re-enabling reads.
    ///
    /// An Ok result doesn't mean that all have been re-enabled (just in case the channel filled up somehow), but does
    /// mean there were no errors.
    #[tracing::instrument(skip_all)]
    pub fn enable_reads(
        &mut self,
        poll: &mut Poll,
        sender: &Sender<(MsgToken, Req)>,
    ) -> Result<()> {
        for conn in &mut self.pool {
            if let Some(conn) = conn {
                // Before trying to push, check that there is space available
                if !conn.should_read(sender) {
                    continue;
                }

                tracing::trace!("Re-enabling reads on request channel");
                match conn.retry_send(sender) {
                    ReadResult::Continue => {
                        if let Err(err) = conn.enable_interest(poll, Interest::READABLE) {
                            tracing::error!("Error enabling interest: {}", err);
                            let token = conn.token;
                            self.cleanup(poll, token);
                            // FIX: Breaking here to avoid double use of &mut connection_pool in the next loop
                            // iteration, this can be fixed with an entry-like API for connections
                            break;
                        }
                    }
                    ReadResult::SendFull => {}
                    // This will only happen when the channel is disconnected, so can error out of the whole method
                    ReadResult::Error(err) => bail!(err),
                    _ => unreachable!(),
                };
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
