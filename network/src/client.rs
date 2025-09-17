//! Client io event loop and configuration.

use std::time::Duration;

use anyhow::{bail, Result};
use crossbeam::channel::{Receiver, Sender};
#[cfg(unix)]
use mio::net::UnixStream;
use mio::{net::TcpStream, Events, Interest, Poll, Token};

use super::{
    connection::{Connection, MsgToken, ReadResult, Traffic, WriteResult},
    signals::RunToken,
    stream::SocketMode,
    IoCodec,
};

const SERVER: Token = Token(0);

pub struct IoLoopConfig {
    /// The length of time poll will block before falling through. Higher values mean longer before the system will
    /// check for outgoing responses. Recommended range 10-50ms, but client can be more relaxed (at least toward the
    /// upper end) as checking for outgoing messages is not as time critical.
    pub io_poll_timeout: Duration,

    /// The lower bound of capacity on the response channel when read interest will be re-enabled.
    ///
    /// This should be set to some fraction, say 3/4 of the response channel capacity to effectively throttle incoming
    /// traffic.
    pub response_reenable_limit: usize,

    // Number of Mio events to buffer.
    pub event_capacity: usize,

    /// Socket configuration for the client.
    pub socket_mode: SocketMode,
}

impl IoLoopConfig {
    // TODO: Tune the values of these config items
    pub fn with_reenable_limit(limit: usize) -> Self {
        Self {
            io_poll_timeout: Duration::from_millis(50),
            response_reenable_limit: limit,
            event_capacity: 16,
            socket_mode: SocketMode::default(),
        }
    }
}

/// Run the client IO loop with the server.
///
/// Backpressure is provided in two directions:
/// 1. Requests are only written when the stream is writable, so the request channel will fill, blocking senders.
/// 2. Responses are capped by response channel capacity, so when processing is slow, reading will block. Note that
///    blocked reading will also block writing requests.
pub fn run_io_loop<Req: IoCodec, Res: IoCodec>(
    running: RunToken,
    config: &IoLoopConfig,
    traffic: &Traffic,
    request_rx: Receiver<(u32, Req)>,
    response_tx: Sender<(MsgToken, Res)>,
) -> Result<()> {
    let stream = match &config.socket_mode {
        #[cfg(unix)]
        SocketMode::Unix(path) => super::stream::Stream::Unix(UnixStream::connect(path.as_ref())?),
        SocketMode::Tcp(addr) => super::stream::Stream::Tcp(TcpStream::connect(*addr)?),
    };

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(config.event_capacity);
    // Our Req type is T and Res type is U for clients
    let mut conn = Connection::new_client(&mut poll, SERVER, stream, Some(traffic))?;

    let io_span = tracing::error_span!("io");
    let _io_span_guard = io_span.enter();
    tracing::info!("Starting client IO loop");

    while running == true {
        // Re-enable read interest if the send channel is no longer full
        // If any error condition results from the retry, even a full, something must have gone wrong, so we exit
        if !conn.is_readable() && response_tx.len() < config.response_reenable_limit * 3 / 4 {
            tracing::trace!("Re-enabling reads on response channel");
            match conn.retry_send(&response_tx) {
                ReadResult::Continue => conn.enable_interest(&mut poll, Interest::READABLE)?,
                ReadResult::SendFull => {}
                ReadResult::Error(err) => bail!(err),
                _ => unreachable!(),
            };
        }

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
                    match conn.read_into_channel(&response_tx) {
                        // If the channel send fails, we can't proceed, so shut down
                        // This send call will block if the channel is full, providing backpressure
                        // TODO: When we change the server non-blocking here, we can change the client too
                        ReadResult::Message(_) => unreachable!(),
                        ReadResult::Continue => {}
                        ReadResult::SendFull => {
                            tracing::trace!("Send channel full, disabling reads");
                            conn.disable_interest(&mut poll, Interest::READABLE)?;
                            break;
                        }
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
                        WriteResult::Sent(t) => tracing::trace!("Wrote message {:?}", t),
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
