//! Server io event loop and configuration.

use std::{io::ErrorKind, time::Duration};

use anyhow::Result;
use crossbeam::channel::{Receiver, Sender};
use mio::{Events, Interest, Poll, Token};

use super::{connection::*, signals::RunToken, stream::SocketMode, IoCodec};

/// Set the listener to be the next index above the max connections to avoid collisions
/// With a fixed connection pool this is easier than making the first connection 1
const ACCEPT: Token = Token(MAX_POOL_CONNECTIONS + 1);

pub struct IoLoopConfig {
    /// The max number of simultaneous client connections.
    pub pool_size: usize,

    /// The length of time poll will block before falling through. Higher values mean longer before the system will
    /// check for outgoing responses. Recommended range 10-50ms.
    pub io_poll_timeout: Duration,

    /// The trigger point for stopping reads on a connection.
    ///
    /// In order to not block the response channel, this is only a soft cap - it is used to signal to stop reading, but
    /// doesn't stop the buffer filling (which is required to not block the response channel). Restarting will happen
    /// when the write queue hits half the cap size to avoid thrashing.
    /// TODO: Tunable restart?
    pub write_queue_soft_cap: usize,

    /// The size of the Mio event queue.
    pub event_capacity: usize,

    /// Socket configuration for the server.
    pub socket_mode: SocketMode,
}

impl Default for IoLoopConfig {
    fn default() -> Self {
        Self {
            pool_size: MAX_POOL_CONNECTIONS,
            io_poll_timeout: Duration::from_millis(10),
            write_queue_soft_cap: 256,
            event_capacity: 128,
            socket_mode: SocketMode::default(),
        }
    }
}

/// Spawn a thread and start the main event loop.
///
/// Back presssure on input is managed by blocking the send channel when reading. This prevents unbounded request channel
/// growth, but does block writes. Write back pressure is managed with a capacity on the write queue, but this doesn't
/// stop full drains into the individual unbounded queues. This is managed by switching off reads on full connections.
pub fn run_io_loop<Req: IoCodec, Res: IoCodec>(
    running: RunToken,
    ready: &Option<Sender<()>>,
    config: &IoLoopConfig,
    traffic: &Traffic,
    request_tx: Sender<(MsgToken, Req)>,
    response_rx: Receiver<(MsgToken, Res)>,
) -> Result<()> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(config.event_capacity);
    let mut connection_pool =
        ConnectionPool::new(config.pool_size, traffic, config.write_queue_soft_cap)?;

    // Set up the socket server and register
    let listener = config
        .socket_mode
        .bind_listener_and_register(&poll, ACCEPT)?;

    let io_span = tracing::error_span!("io");
    let _io_guard = io_span.enter();
    tracing::trace!("starting io loop");
    ready.as_ref().map(|s| s.send(()));

    // Start the main event loop, exiting if we are shutting down
    while running == true {
        // First enable any readers that need to be enabled, this captures readers disabled from both full send channel
        // and full outgoing cap
        connection_pool.enable_reads(&mut poll, &request_tx)?;

        // Then drain outgoing responses into the connection's queues
        connection_pool.fill_write_queues(&response_rx, &mut poll)?;

        poll.poll(&mut events, Some(config.io_poll_timeout))?;

        for event in &events {
            match event.token() {
                ACCEPT => {
                    tracing::trace!("ACCEPT token received");
                    loop {
                        match listener.accept() {
                            Ok(stream) => {
                                // When we have an incoming stream, test if there is room in the connection pool to
                                // accept it, otherwise reject; If there is no room or the registration fails no
                                // slots are taken up and the returned stream is dropped which rejects the
                                // connection
                                match connection_pool.register(&mut poll, stream) {
                                    AddStreamResult::Success(id) => {
                                        tracing::info!("Connection successful: id={}", id)
                                    }
                                    AddStreamResult::NoSpace(_) => {
                                        tracing::warn!("Connection rejected: Out of capacity")
                                    }
                                    AddStreamResult::RegistrationFailure(_) => {
                                        tracing::error!("Connection rejected: Registration failure")
                                    }
                                }
                            }
                            // On WouldBlock we are done processing this event
                            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                            // Other errors we just log and ignore the connection attempt
                            Err(err) => tracing::error!("Accept error: {:?}", err),
                        }
                    }
                }

                token => {
                    if event.is_readable() {
                        if let Some(conn) = connection_pool.get_mut_by_token(token) {
                            let id = conn.id();
                            let read_span = tracing::error_span!("conn_read", id);
                            let _read_guard = read_span.enter();
                            tracing::trace!("Readable: token={}", token.0);

                            loop {
                                match conn.read_into_channel(&request_tx) {
                                    ReadResult::Message(_) => unreachable!(),
                                    ReadResult::Continue => {}
                                    ReadResult::SendFull => {
                                        tracing::trace!("Send channel full, disabling reads");
                                        if let Err(err) =
                                            conn.disable_interest(&mut poll, Interest::READABLE)
                                        {
                                            tracing::error!("Error disabling interest: {}", err);
                                            connection_pool.cleanup(&mut poll, token);
                                        }
                                        break;
                                    }
                                    ReadResult::WouldBlock => {
                                        tracing::trace!("Read WouldBlock, breaking");
                                        break;
                                    }
                                    ReadResult::Eof => {
                                        tracing::info!("EOF, cleaning up");
                                        connection_pool.cleanup(&mut poll, token);
                                        break;
                                    }
                                    // On any form of read error we just drop the connection rather than trying to
                                    // recover
                                    ReadResult::Error(err) => {
                                        tracing::error!("Error, cleaning up: {}", err);
                                        connection_pool.cleanup(&mut poll, token);
                                        break;
                                    }
                                };
                            }
                        }
                    }

                    if event.is_writable() {
                        if let Some(conn) = connection_pool.get_mut_by_token(token) {
                            let id = conn.id();
                            let write_span = tracing::error_span!("conn_write", id);
                            let _write_guard = write_span.enter();
                            tracing::trace!("Writable: token={}", token.0);

                            loop {
                                match conn.write() {
                                    WriteResult::Continue => {}
                                    WriteResult::Sent(msg_token) => {
                                        tracing::trace!("Wrote message {:?}", msg_token)
                                    }
                                    WriteResult::Drained => {
                                        // No longer interested in writes until write buffer is refilled
                                        // If this fails, stream is unusable, so cleanup
                                        tracing::trace!("Drained");
                                        if let Err(err) =
                                            conn.disable_interest(&mut poll, Interest::WRITABLE)
                                        {
                                            tracing::error!("Error disabling interest: {}", err);
                                            connection_pool.cleanup(&mut poll, token);
                                        }
                                        break;
                                    }
                                    WriteResult::WouldBlock => break,
                                    WriteResult::Eof => {
                                        tracing::info!("EOF, cleaning up");
                                        connection_pool.cleanup(&mut poll, token);
                                        break;
                                    }
                                    WriteResult::Error(err) => {
                                        tracing::error!("Error, cleaning up: {}", err);
                                        connection_pool.cleanup(&mut poll, token);
                                        break;
                                    }
                                    WriteResult::Disconnected => unreachable!(),
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
