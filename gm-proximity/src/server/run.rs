//! Server handler for GM-Proximity.

use std::{io::ErrorKind, sync::Arc, time::Duration};

use anyhow::Result;
use arc_swap::ArcSwap;
use crossbeam::channel::{self, Receiver, RecvTimeoutError, Sender};
#[cfg(windows)]
use mio::net::TcpListener;
#[cfg(unix)]
use mio::net::UnixListener;
use mio::{Events, Interest, Poll, Token};

use crate::{connection::*, ctrlc::*, message::prelude::*, Context, MAX_CONNECTIONS};

use super::{geo_store::GeoStore, handle::Handler};

/// Set the listener to be the next index above the max connections to avoid collisions
/// With a fixed connection pool this is easier than making the first connection 1
const ACCEPT: Token = Token(MAX_CONNECTIONS + 1);

pub struct Config {
    /// The max number of simultaneous client connections.
    pub pool_size: usize,

    /// The length of time poll will block before falling through. Higher values mean longer before the system will
    /// check for outgoing responses. Recommended range 10-50ms.
    pub io_poll_timeout: Duration,

    /// Timeout for when to check whether a shutdown has been triggered. Only use this when the work prevented by
    /// blocking is a shutdown check. The value can be high as manual shutdown is not performance critical.
    pub shutdown_timeout: Duration,

    /// The size of the request channel.
    ///
    /// Governs how many requests can be queued at a time, providing backpressure to the readers by not allowing
    /// progress when the queue is full - this prevents memory overconsumption by stacking up the size of the read
    /// channel when processing or sending is slow. Should be large enough to not inhibit processing speed.
    /// TODO: If batching into processing, this should be a multiple of the batch size
    pub request_capacity: usize,

    /// The size of the response channel.
    ///
    /// Governs how many responses can be queued from processing, providing backpressure to the calculation. If the
    /// calculations outpace writing out, this will prevent memory explosion. Generally this should be large enough to
    /// not get held up by draining to the write queue, as this only happens once each run through the loop.
    /// Note that this capacity is in addition to the capacity in each connection's write queue (which is unbounded but
    /// will turn off reading when too large).
    /// TODO: Should we drain the response channel in a couple of other places to help keep the size of this down? Maybe
    /// test when tuning io
    pub response_capacity: usize,

    /// The trigger point for stopping reads on a connection.
    ///
    /// In order to not block the response channel, this is only a soft cap - it is used to signal to stop reading, but
    /// doesn't stop the buffer filling (which is required to not block the response channel). Restarting will happen
    /// when the write queue hits half the cap size to avoid thrashing.
    /// TODO: Tunable restart?
    pub write_queue_soft_cap: usize,

    /// The size of the Mio event queue.
    pub event_capacity: usize,

    #[cfg(unix)]
    /// Name of the socket to listen on.
    /// TODO: In test and bench can have a separate config item for an unnamed socket half that can be used in testing
    pub unix_socket_name: &'static str,

    #[cfg(windows)]
    /// Address of the socket to listen on.
    pub tcp_socket_addr: &'static str,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pool_size: 8,
            io_poll_timeout: Duration::from_millis(10),
            shutdown_timeout: Duration::from_millis(500),
            request_capacity: 1024,
            response_capacity: 1024,
            write_queue_soft_cap: 256,
            event_capacity: 128,
            #[cfg(unix)]
            unix_socket_name: crate::UNIX_SOCKET_NAME,
            #[cfg(windows)]
            tcp_socket_addr: crate::TCP_SOCKET_ADDR,
        }
    }
}

pub fn run(context: Context<Config>) -> Result<()> {
    let run_span = tracing::error_span!("server");
    let _enter = run_span.enter();

    let running = context.running;
    let config: &_ = Box::leak(Box::new(context.config));
    let traffic: &_ = Box::leak(Box::new(Traffic::default()));

    let (request_tx, request_rx) = channel::bounded::<(MsgToken, Request)>(config.request_capacity);
    let (response_tx, response_rx) =
        channel::bounded::<(MsgToken, Response)>(config.response_capacity);

    // Start the IO loop
    let io_run_span = run_span.clone();
    let r = running.clone();
    let io_handle = std::thread::spawn(move || {
        let _enter = io_run_span.enter();
        match run_server_io_loop(
            r.clone(),
            &context.ready,
            &config,
            traffic,
            request_tx,
            response_rx,
        ) {
            Ok(_) => tracing::trace!("Server IO loop exited success"),
            Err(err) => tracing::error!("Server IO loop exit error: {}", err),
        };
        r.shutdown();
    });

    #[cfg(unix)]
    let listen_on = config.unix_socket_name;
    #[cfg(windows)]
    let listen_on = config.tcp_socket_addr;
    tracing::info!("Geo Munge Proximity server listening on: {}", listen_on);

    // Initialize the quadtree and start the main processing loop
    // On the main thread we block on listening for messages on the request channel with a timeout to capture the
    // graceful shutdown - this timeout can be relatively long as the shutdown is not time-critical
    // Note that the handle function takes the channel rather than just returning the response as the server may choose
    // to chunk responses
    // TODO: Initializing with the default GeoStore options. This should be considered and aligned with bounding box and
    // key mode before finalizing (esp. given key mode is stored in the handler)
    let geo_store = ArcSwap::from(Arc::new(GeoStore::new()));
    let handler = Handler::new(geo_store, response_tx);

    // TODO: Add parallelism back with better threading mechanism, note need to keep handler lightweight and clonable
    // TODO: Improve and instrument this loop - should the handler be cloned? Should we ignore channel shutdown?
    while running.is_running() {
        match request_rx.recv_timeout(config.shutdown_timeout) {
            Ok(req) => handler.handle(req, traffic),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if running.is_running() {
                    tracing::error!("Request channel disconnected, shutting down");
                    running.shutdown();
                    break;
                }
            }
        }
    }

    io_handle.join().expect("Couldn't join io handle");
    tracing::info!("Geo Munge Proximity server shut down successfully");
    Ok(())
}

/// Spawn a thread and start the main event loop.
///
/// Back presssure on input is managed by blocking the send channel when reading. This prevents unbounded request channel
/// growth, but does block writes. Write back pressure is managed with a capacity on the write queue, but this doesn't
/// stop full drains into the individual unbounded queues. This is managed by switching off reads on full connections.
pub fn run_server_io_loop(
    running: RunToken,
    ready: &Option<Sender<()>>,
    config: &Config,
    traffic: &Traffic,
    request_tx: Sender<(MsgToken, Request)>,
    response_rx: Receiver<(MsgToken, Response)>,
) -> Result<()> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(config.event_capacity);
    let mut connection_pool =
        ConnectionPool::new(config.pool_size, traffic, config.write_queue_soft_cap)?;

    // Set up the socket server and register
    // Remove the socket file before binding, ignoring errors (its fine if it doesn't exist)
    // TODO: Custom fd on linux as a setting, ability to do anonymous for testing
    #[cfg(unix)]
    let listener = {
        let _ = std::fs::remove_file(config.unix_socket_name);
        let mut listener = UnixListener::bind(config.unix_socket_name)?;
        poll.registry()
            .register(&mut listener, ACCEPT, Interest::READABLE)?;
        listener
    };

    #[cfg(windows)]
    let listener = {
        let mut listener = TcpListener::bind(config.tcp_socket_addr.parse()?)?;
        poll.registry()
            .register(&mut listener, ACCEPT, Interest::READABLE)?;
        listener
    };

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
                            Ok((stream, _)) => {
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
