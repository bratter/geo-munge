//! Mio-based cross-platform event loop.
//!
//! The loop is non-blocking, single threaded event loop that manages connections and messages.

use std::{
    io::ErrorKind,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

use mio::{net::UnixListener, Events, Interest, Poll, Token};

use crate::{connection::*, message::prelude::*, UNIX_SOCKET_NAME};

// TODO: Reduce this. If this is required for responses, it needs to be much shorter (maybe 10-50ms)
const POLL_TIMEOUT: Duration = Duration::from_millis(1000);
const MAX_CONNECTIONS: usize = 8;
// Set the listener to be the next index above the max connections to avoid collisions
// With a fixed connection pool this is easier than making the first connection 1
const SERVER: Token = Token(MAX_CONNECTIONS + 1);

/// Spawn a thread and start the main event loop.
// TODO: This has to take both channels as inputs
pub fn spawn_event_loop(running: Arc<AtomicBool>, request_tx: Sender<Request>) -> JoinHandle<()> {
    // TODO: Might like to have the spawn outside the function so we can use ? instead of unwrap
    std::thread::spawn(move || {
        let mut poll = match Poll::new() {
            Ok(poll) => poll,
            Err(err) => panic!("Failed to create poll instance: {}", err),
        };
        // TODO: Tune this; put in const or make a setting
        let mut events = Events::with_capacity(1024);
        let mut connection_pool = ConnectionPool::<MAX_CONNECTIONS>::new();

        // Set up the socket server and register
        // TODO: o/s flag
        // TODO: Custom fd on linux as a setting?
        // Remove the socket file before binding, ignoring errors (its fine if it doesn't exist)
        let _ = std::fs::remove_file(UNIX_SOCKET_NAME);
        let mut listener = UnixListener::bind(UNIX_SOCKET_NAME).unwrap();

        poll.registry()
            .register(&mut listener, SERVER, Interest::READABLE)
            .unwrap();

        // Start the main event loop, exiting if we are shutting down
        while running.load(Ordering::SeqCst) {
            // TODO: Don't think that the unwrap (or even loop breaking with ?) is right here... investigate
            poll.poll(&mut events, Some(POLL_TIMEOUT)).unwrap();

            for event in &events {
                match event.token() {
                    SERVER => {
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
                        if let Some(conn) = connection_pool.get_mut(token) {
                            if event.is_readable() {
                                // TODO: Is there some way to just operate on the pool and not expose the connection?
                                // Maybe a waste to do that
                                loop {
                                    match conn.read() {
                                        ReadResult::Request(req) => {
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
                                            if let Err(_) = request_tx.send(req) {
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
                                            eprintln!(
                                                "Read error on connection {}: {}",
                                                token.0, err
                                            );
                                            connection_pool.cleanup(&mut poll, token);
                                            break;
                                        }
                                    };
                                }

                            // TODO: Write events
                            } else if event.is_writable() {
                                eprintln!("write event");
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
    })
}
