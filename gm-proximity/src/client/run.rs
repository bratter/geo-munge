use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use crossbeam::channel::{self, Receiver, RecvTimeoutError, Sender};
#[cfg(windows)]
use mio::net::TcpStream;
#[cfg(unix)]
use mio::net::UnixStream;
use mio::{Events, Interest, Poll, Token};

use super::{CommandHandler, Tracker};
use crate::{args::ClientCommand, connection::*, ctrlc::*, message::prelude::*, Context};

const SERVER: Token = Token(0);

pub struct Config {
    /// The length of time poll will block before falling through. Higher values mean longer before the system will
    /// check for outgoing responses. Recommended range 10-50ms, but client can be more relaxed (at least toward the
    /// upper end) as checking for outgoing messages is not as time critical.
    pub io_poll_timeout: Duration,

    /// Timeout for when to check whether a shutdown has been triggered. Only use this when the work prevented by
    /// blocking is a shutdown check. The value can be high as manual shutdown is not performance critical.
    pub shutdown_timeout: Duration,

    /// Size of the request channel.
    ///
    /// As this only writes out when writing is available, this capacity will backpressure any upstream io that relies
    /// on sending on this channel.
    pub request_capacity: usize,

    /// Size of the response channel.
    ///
    /// As this channel will only be drawn from when the client can process it, it will backpressure the server.
    pub response_capacity: usize,

    // Number of Mio events to buffer.
    pub event_capacity: usize,

    #[cfg(unix)]
    /// Name of the socket to connect to.
    pub unix_socket_name: &'static str,

    #[cfg(windows)]
    /// Address of the TCP socket to connect to.
    pub tcp_socket_addr: &'static str,
}

// TODO: Tune the values of these config items
impl Default for Config {
    fn default() -> Self {
        Self {
            io_poll_timeout: Duration::from_millis(50),
            shutdown_timeout: Duration::from_millis(500),
            request_capacity: 1024,
            response_capacity: 1024,
            event_capacity: 16,
            #[cfg(unix)]
            unix_socket_name: crate::UNIX_SOCKET_NAME,
            #[cfg(windows)]
            tcp_socket_addr: crate::TCP_SOCKET_ADDR,
        }
    }
}

#[tracing::instrument(skip_all, name = "client")]
pub fn run(cmd: ClientCommand, context: Context<Config>) -> Result<()> {
    // Set up message channels and instrumentation
    let config: &_ = Box::leak(Box::new(context.config));
    let (request_tx, request_rx) = channel::bounded::<(u32, Request)>(config.request_capacity);
    let (response_tx, response_rx) = channel::bounded::<(u32, Response)>(config.response_capacity);
    let traffic: &_ = Box::leak(Box::new(Traffic::default()));
    let mut tracker = Tracker::default();
    let (mut cmd_handler, done) = CommandHandler::new(request_tx, tracker.clone());

    let r = context.running.clone();
    let io_handle = std::thread::spawn(move || {
        match run_client_io_loop(r.clone(), config, traffic, request_rx, response_tx) {
            Ok(_) => tracing::trace!("Client IO loop exit success"),
            Err(err) => tracing::error!("Client IO loop exit error: {}", err),
        }
        r.shutdown();
    });

    // Shift request handling onto its own thread. If this completes fast or is highly blocking (like an interactive
    // terminal) this thread will use minimal resources. If lots of disk/std io is required, it will run in the
    // background while still allowing response handling.
    let cmd_handle = std::thread::spawn(move || match cmd_handler.handle(cmd) {
        Ok(_) => tracing::trace!("Client command handler exit success"),
        Err(err) => tracing::error!("Client command handler exit error: {}", err),
    });

    // Run the response handling in the main thread using an additional flag to track whether the request thread has
    // finished - needs to be its own flag as the channel will report done only once. When done requesting, we know that
    // all reuqest ids have been entered in the tracker, so can wait until these are all cleared before exiting the
    // response loop. Without waiting for done requesting, there may just be no outstanding responses so this would exit
    // spuriously.
    let mut done_requesting = false;
    while context.running.is_running() {
        match response_rx.recv_timeout(config.shutdown_timeout) {
            Ok((id, res)) => tracker.handle(id, res)?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        };

        // Toggle the done flag the first time a message comes through on the channel
        if !done_requesting && done.try_recv().is_ok() {
            tracing::trace!("Setting done flag in response loop");
            done_requesting = true;
        }
        if done_requesting && tracker.outstanding() == 0 {
            break;
        }
    }

    // If we break out the the response loop, we must shut down
    context.running.shutdown();

    // TODO: Better join handling
    cmd_handle
        .join()
        .map_err(|_| anyhow!("Cmd handler thread join failed"))?;
    io_handle
        .join()
        .map_err(|_| anyhow!("IO thread join failed"))?;

    // TODO: Better instrumentation/reporting. Tracing here?
    println!(
        "Client done; bytes sent={}; bytes recv={}",
        traffic.sent(),
        traffic.recv()
    );
    Ok(())
}

/// Run the client IO loop with the server.
///
/// Backpressure is provided in two directions:
/// 1. Requests are only written when the stream is writable, so the request channel will fill, blocking senders.
/// 2. Responses are capped by response channel capacity, so when processing is slow, reading will block. Note that
///    blocked reading will also block writing requests.
pub fn run_client_io_loop(
    running: RunToken,
    config: &Config,
    traffic: &Traffic,
    request_rx: Receiver<(u32, Request)>,
    response_tx: Sender<(u32, Response)>,
) -> Result<()> {
    // TODO: On linux have option of stream or TCP?
    #[cfg(unix)]
    let stream = UnixStream::connect(config.unix_socket_name)?;

    #[cfg(windows)]
    let stream = TcpStream::connect(config.tcp_socket_addr.parse()?)?;

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(config.event_capacity);
    let mut conn = Connection::new_client(&mut poll, SERVER, stream, Some(traffic))?;

    let io_span = tracing::trace_span!("io");
    let _io_span_guard = io_span.enter();
    tracing::trace!("Starting client IO loop");

    while running == true {
        // Re-enable write interest if there are items in the request channel
        // In the client we don't pre-buffer sends, we just process as-ready
        if !request_rx.is_empty() && !conn.is_writeable() {
            tracing::trace!(
                "Request channel populated {}, enabling WRITABLE",
                request_rx.len()
            );
            conn.enable_interest(&mut poll, Interest::WRITABLE)?;
        }

        poll.poll(&mut events, Some(config.io_poll_timeout))?;

        for event in &events {
            if event.token() != SERVER {
                continue;
            }

            // TODO: On windows, we might need to test take_error here if Mio doesn't manage the async connection

            if event.is_readable() {
                tracing::trace!("Readable");
                loop {
                    match conn.read() {
                        ReadResult::Request(_) => unreachable!(),
                        // If the channel send fails, we can't proceed, so shut down
                        // This send call will block if the channel is full, providing backpressure
                        ReadResult::Response(res) => {
                            let res_id = res.0;
                            response_tx.send(res)?;
                            tracing::trace!("Response received:: {}", res_id);
                        }
                        ReadResult::Continue => {}
                        ReadResult::WouldBlock => {
                            tracing::trace!("Read WouldBlock, breaking");
                            break;
                        }
                        // TODO: Not sure why, but in bench, this is triggering. Not sure it should be an error anyway
                        ReadResult::Eof => bail!("Connection closed unexpectedly during read"),
                        ReadResult::Error(err) => bail!(err),
                    }
                }
            }

            // When we are writable, we only write if there is a pending message, taking straight from the channel
            if event.is_writable() {
                tracing::trace!("Writable");
                loop {
                    match conn.write_from_channel(&request_rx) {
                        WriteResult::Continue => {}
                        WriteResult::Drained => {
                            tracing::trace!("Write drained, breaking");
                            // No longer interested in writes until write buffer is refilled
                            // If this fails, stream is unusable, so exit
                            conn.disable_interest(&mut poll, Interest::WRITABLE)?;
                            break;
                        }
                        WriteResult::WouldBlock => {
                            tracing::trace!("Write WouldBlock, breaking");
                            break;
                        }
                        // In the client we assume that a disconnected write channel simply means writes are finished.
                        // We therefore disable writes, and because the channel will now always be empty, nothing will
                        // re-enable write interest. Note that it may disconnect because of an error which will not be
                        // handled.
                        // TODO: If disconnected, should we check the done flag to decide if error or ok? Don't think
                        // it will add much
                        WriteResult::Disconnected => {
                            tracing::trace!("Write channel disconnected, no more need for writes");
                            conn.disable_interest(&mut poll, Interest::WRITABLE)?;
                            break;
                        }
                        WriteResult::Eof => bail!("Connection closed unexpectedly during write"),
                        WriteResult::Error(err) => bail!(err),
                    }
                }
            }
        }
    }

    Ok(())
}
