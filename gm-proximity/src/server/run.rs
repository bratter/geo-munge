//! Server handler for GM-Proximity.

use std::{
    io::{BufReader, BufWriter},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::Result;
use interprocess::local_socket::{
    prelude::*, GenericNamespaced, ListenerNonblockingMode, ListenerOptions,
};
use threadpool::ThreadPool;

use crate::{message::Message, SOCKET_NAME};

const POLL_TIME: u64 = 1000;
const MAX_WORKERS: usize = 4;

// TODO: Consider different ways to manage polling.
// - Currently using non-blocking listeners inside a polling loop so that ctrl-c signals go through for the accept loop.
// - Using blocking reads on the streams for simplicity, but this will not allow graceful shutdown of long-lasting
//   client sockets.
// - Think the simplicity and message responsiveness will far outweigh need to terminate while a client is running.
// - A more robust solution might be to use epoll_rs (nix) and wepoll_binding (win) to avoid delays in polling
// - Still need to set a timeout, but it can afford to be much longer, as it is only waiting for ctrl-c
pub fn run() -> Result<()> {
    let socket_name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new()
        .name(socket_name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .create_sync()?;
    let pool = ThreadPool::new(MAX_WORKERS);

    // Ctrl-c handling
    // TODO: In Ctrl-c handler can do the graceful exit path but a double ctrl-c can exit using std::process::exit
    let term_now = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    ctrlc::set_handler(move || {
        // If we have already entered the handler once and are now back a send time, we want to perform a hard
        // termination. This might happen if one of the streams blocks for an extended period of time.
        // NOTE: `signal_hook` crate uses libc `_exit()` rather than `std::process::exit`, but don't think it is necessary
        // here, see: https://github.com/vorner/signal-hook/blob/master/src/low_level/mod.rs
        if term_now.load(Ordering::SeqCst) {
            std::process::exit(1);
        }
        // If we are not terminating immediately, then try to gracefully exit, but inform the hanlder that another
        // ctrl-c will terminate immediately.
        eprintln!("Attempting graceful shutdown...");
        r.store(false, Ordering::SeqCst);
        term_now.store(true, Ordering::SeqCst);
    })?;

    println!("Geo Munge Proximity server listening...");

    // Main listener loop
    // Stream listener is non-blocking on accepts with a long poll time - clients may take a little while to start up,
    // but allows graceful shutdown as this loop always has to be active
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(stream) => {
                let r = Arc::clone(&running);
                pool.execute(|| {
                    if let Err(e) = handle_stream_blocking(stream, r) {
                        eprintln!("Error handling client: {}", e);
                    }
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(POLL_TIME));
            }
            Err(e) => eprintln!("Accept error {}", e),
        }
    }

    println!("Geo Munge Proximity server shut down successfully");
    Ok(())
}

/// Stream handler with blocking reads.
///
/// Will still check running state each read cycle and gracefully shutdown, but "dormant" clients will block
/// indefinitely. The tradeoff of a blocking stream in terms of reduced ability for graceful shutdown and enforced
/// linear request-response are appropriate for a first pass.
fn handle_stream_blocking(stream: LocalSocketStream, running: Arc<AtomicBool>) -> Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut writer = BufWriter::new(&stream);

    loop {
        if !running.load(Ordering::SeqCst) {
            eprintln!("Gracefully closing client");
            return Ok(());
        }

        match Message::read(&mut reader)? {
            Some(msg) => {
                let ack = match msg {
                    Message::Msg(content) => {
                        eprintln!("Server received: {}", content);
                        Ok(format!("ACK: {}", content))
                    }
                    Message::Ack(_) => {
                        eprintln!("This shouldn't happen");
                        Err("You shouldn't be sending me acks".to_string())
                    }
                };

                let response = Message::Ack(ack);
                response.write(&mut writer)?;
            }
            None => {
                // EOF: client closed the connection
                eprintln!("Client disconnected");
                break;
            }
        }
    }

    Ok(())
}
