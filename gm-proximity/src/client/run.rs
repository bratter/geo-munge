use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use crossbeam::channel::{self, Receiver, Sender};
#[cfg(windows)]
use mio::net::TcpStream;
#[cfg(unix)]
use mio::net::UnixStream;
use mio::{Events, Interest, Poll, Token};
use tracing::Level;

use super::CommandHandler;
use crate::{args::ClientCommand, connection::*, ctrlc::*, message::prelude::*};

const SERVER: Token = Token(0);

pub struct Config {
    /// The length of time poll will block before falling through. Higher values mean longer before the system will
    /// check for outgoing responses. Recommended range 10-50ms, but client can be more relaxed (at least toward the
    /// upper end) as checking for outgoing messages is not as time critical.
    pub io_poll_timeout: Duration,

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

pub fn run(cmd: ClientCommand, config: Config) -> Result<()> {
    // Enable tracing
    // TODO: Configure better
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_writer(std::io::stderr)
        .init();

    // Set up ctrlc handling
    let running = set_ctrlc_handler()?;

    // Set up message channels and instrumentation
    let (request_tx, request_rx) = channel::bounded::<(u32, Request)>(config.request_capacity);
    let (response_tx, response_rx) = channel::bounded::<(u32, Response)>(config.response_capacity);
    let traffic: &_ = Box::leak(Box::new(Traffic::default()));
    let mut cmd_handler = CommandHandler::new(request_tx, response_rx);

    let r = running.clone();
    let io_handle = std::thread::spawn(move || {
        match run_client_io_loop(r.clone(), &config, traffic, request_rx, response_tx) {
            Ok(_) => tracing::trace!("Client IO loop exit success"),
            Err(err) => tracing::error!("Client IO loop exit error: {}", err),
        }
        r.shutdown();
    });

    // TODO: Make the handler structured more like the server one, but here we should have a loop that can deal with
    // responses at the same time the handler is sending requests - this might require another thread, but for now just
    // block here. Also note that the method of checking requests for done state probably needs to be better - either
    // report on the Request itself so doesn't need to be passed, or report some more tracker info, or just require for
    // everything.
    cmd_handler.handle(cmd)?;
    loop {
        let res = cmd_handler.recv()?;
        cmd_handler.print_response(&res);
        if cmd_handler.outstanding_reqs() == 0 {
            break;
        }
    }

    // TODO: Better join handling
    running.shutdown();
    io_handle
        .join()
        .map_err(|_| anyhow!("IO thread join failure"))?;

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
                        ReadResult::Eof => bail!("Connection closed unexpectedly"),
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
                        WriteResult::Eof => bail!("Connection closed unexpectedly"),
                        WriteResult::Error(err) => bail!(err),
                    }
                }
            }
        }
    }

    Ok(())
}
