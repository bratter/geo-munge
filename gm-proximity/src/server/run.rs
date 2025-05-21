//! Server handler for GM-Proximity.

use std::{
    io::ErrorKind,
    sync::{
        mpsc::{Receiver, RecvTimeoutError, Sender},
        Arc, RwLock,
    },
    time::Duration,
};

use anyhow::Result;
use geo::Rect;
use mio::{net::UnixListener, Events, Interest, Poll, Token};
use threadpool::ThreadPool;

use geolib::qt::{QtData, Quadtree, ToRadians};

use crate::{connection::*, ctrlc::*, message::prelude::*, MAX_CONNECTIONS, UNIX_SOCKET_NAME};

use super::handle::Handler;

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

    /// The size of the Mio event queue.
    pub event_capacity: usize,

    /// Name of the socket to listen on.
    /// TODO: In test and bench can have a separate config item for an unnamed socket half that can be used in testing
    pub unix_socket_name: &'static str,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pool_size: 8,
            io_poll_timeout: Duration::from_millis(10),
            shutdown_timeout: Duration::from_millis(500),
            event_capacity: 1024,
            unix_socket_name: UNIX_SOCKET_NAME,
        }
    }
}

pub fn run(config: Config) -> Result<()> {
    // Leak the config for a static lifetime
    let config = Box::leak(Box::new(config));

    // Set up graceful ctrl-c handling
    let running = set_ctrlc_handler()?;

    // TODO: Redo threadpool with Rayon or something else
    let pool = ThreadPool::new(4);
    let (request_tx, request_rx) = std::sync::mpsc::channel::<(MsgToken, Request)>();
    let (response_tx, response_rx) = std::sync::mpsc::channel::<(MsgToken, Response)>();

    // Start the IO loop
    let r = running.clone();
    let io_handle =
        std::thread::spawn(|| run_server_io_loop(config, r, request_tx, response_rx).unwrap());
    println!("Geo Munge Proximity server listening...");

    // Initialize the quadtree and start the main processing loop
    // On the main thread we block on listening for messages on the request channel with a timeout to capture the
    // graceful shutdown - this timeout can be relatively long as the shutdown is not time-critical
    // Note that the handle function takes the channel rather than just returning the response as the server may choose
    // to chunk responses
    // TODO: See todo notes in the function implementation
    let quadtree = build_qt(Reset::default());
    let quadtree = Arc::new(RwLock::new(quadtree));
    let handler = Handler::new(quadtree, response_tx);

    while running.is_running() {
        match request_rx.recv_timeout(config.shutdown_timeout) {
            Ok(req) => {
                // TODO: Currently cloning the handler, but could/should this just be leaked instead?
                let h = handler.clone();
                // Push each request as a job onto the threadpool
                pool.execute(move || {
                    // TODO: Currently we ignore if the channel is shut down, when this changes should see if we
                    // propagate the error
                    let _ = h.handle(req);
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                eprintln!("Request channel disconnected, shutting down");
                break;
            }
        }
    }

    pool.join();
    io_handle.join().expect("Couldn't join io handle");
    println!("Geo Munge Proximity server shut down successfully");
    Ok(())
}

/// Make a basic quadtree.
///
/// TODO: In this simple setup we are passing a single-access qt to each of the io threads that will also manage the
/// calculation. Once a basic version is working this needs to be upgraded, and the make_bbox function improved.
/// TODO: The naming of the is_bounds argument is wrong, it should be called is_point_qt
pub fn build_qt(reset: Reset) -> Quadtree {
    let mut bounds: Rect = reset.bbox.unwrap_or_default().into();
    bounds.to_radians_in_place();

    let qt_opts = QtData::new(false, bounds, None, None);

    Quadtree::new(qt_opts)
}

/// Spawn a thread and start the main event loop.
pub fn run_server_io_loop(
    config: &Config,
    running: RunToken,
    request_tx: Sender<(MsgToken, Request)>,
    response_rx: Receiver<(MsgToken, Response)>,
) -> Result<()> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(config.event_capacity);
    let mut connection_pool = ConnectionPool::new(config.pool_size)?;

    // Set up the socket server and register
    // TODO: o/s flags
    // TODO: Custom fd on linux as a setting, ability to do anonymous for testing
    // Remove the socket file before binding, ignoring errors (its fine if it doesn't exist)
    let _ = std::fs::remove_file(config.unix_socket_name);
    let mut listener = UnixListener::bind(config.unix_socket_name)?;

    poll.registry()
        .register(&mut listener, ACCEPT, Interest::READABLE)?;

    // Start the main event loop, exiting if we are shutting down
    while running == true {
        // First drain outgoing responses into the connection's queues
        connection_pool.fill_write_queues(&response_rx, &mut poll)?;

        poll.poll(&mut events, Some(config.io_poll_timeout))?;

        for event in &events {
            match event.token() {
                ACCEPT => {
                    loop {
                        // TODO: o/s flag
                        match listener.accept() {
                            Ok((stream, _)) => {
                                // When we have an incoming stream, test if there is room in the connection pool to
                                // accept it, otherwise reject; If there is no room or the registration fails no
                                // slots are taken up and the returned stream is dropped which rejects the
                                // connection
                                // TODO: Better logging - tracing?
                                match connection_pool.register(&mut poll, stream) {
                                    AddResult::Success => {
                                        eprintln!("Connection successful")
                                    }
                                    AddResult::NoSpace(_) => {
                                        eprintln!("Connection rejected: Out of capacity")
                                    }
                                    AddResult::RegistrationFailure(_) => {
                                        eprintln!("Connection rejected: Registration failure")
                                    }
                                }
                            }
                            // On WouldBlock we are done processing this event
                            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                            // Other errors we just log and ignore the connection attempt
                            Err(err) => eprintln!("Accept error: {:?}", err),
                        }
                    }
                }

                token => {
                    if let Some(conn) = connection_pool.get_mut_by_conn(token) {
                        if event.is_readable() {
                            loop {
                                match conn.read() {
                                    ReadResult::Request((msg_id, req)) => {
                                        // If the send fails, we just drop the connection - it shouldn't unless the
                                        // system crashes
                                        // TODO: If the channel fails, the whole system is dead, right? So this
                                        // should be a complete exit?
                                        // TODO: Consider implementing backpressure here - simply converting to a
                                        // sync_channel or using crossbeam and blocking on capacity would work as
                                        // the easiest option, but also blocks messaging
                                        // Could also split writing into another thread, but this is more complex
                                        // and probably not required as output writing should be fine to drain
                                        // quickly when the reading frees up, but should check this
                                        let msg_token = conn.msg_token(msg_id);
                                        if let Err(_) = request_tx.send((msg_token, req)) {
                                            eprintln!(
                                                "Request channel error on connection {}",
                                                token.0
                                            );
                                            connection_pool.cleanup(&mut poll, token);
                                            break;
                                        }
                                    }
                                    ReadResult::Response(_) => unreachable!(),
                                    ReadResult::Continue => {}
                                    ReadResult::WouldBlock => break,
                                    ReadResult::Eof => {
                                        connection_pool.cleanup(&mut poll, token);
                                        break;
                                    }
                                    // On any form of read error we just drop the connection rather than trying to
                                    // recover
                                    ReadResult::Error(err) => {
                                        eprintln!("Read error on connection {}: {}", token.0, err);
                                        connection_pool.cleanup(&mut poll, token);
                                        break;
                                    }
                                };
                            }
                        } else if event.is_writable() {
                            loop {
                                match conn.write() {
                                    WriteResult::Continue => {}
                                    WriteResult::Drained => {
                                        // No longer interested in writes until write buffer is refilled
                                        // If this fails, stream is unusable, so cleanup
                                        // TODO: What about re-registering read interest here too? Probably not in
                                        // drained, but somewhere in the write loop
                                        match conn.disable_write_interest(&mut poll) {
                                            Ok(_) => {}
                                            Err(err) => {
                                                // TODO: Very similar error text appears multiple times - make a
                                                // function, for example "log err and cleanup"
                                                eprintln!(
                                                    "Write error on connection {}: {}",
                                                    token.0, err
                                                );
                                                connection_pool.cleanup(&mut poll, token);
                                            }
                                        }
                                        break;
                                    }
                                    WriteResult::WouldBlock => break,
                                    WriteResult::Eof => {
                                        connection_pool.cleanup(&mut poll, token);
                                        break;
                                    }
                                    WriteResult::Error(err) => {
                                        eprintln!("Write error on connection {}: {}", token.0, err);
                                        connection_pool.cleanup(&mut poll, token);
                                        break;
                                    }
                                }
                            }
                        } else {
                            // Given we have only registered interest in the two events, this should be true
                            unreachable!("No other event types");
                        }
                    } else {
                        eprintln!("No connection in pool with token: {}", token.0);
                    }
                }
            }
        }
    }

    Ok(())
}
