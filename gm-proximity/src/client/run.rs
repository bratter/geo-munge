use std::time::Duration;

#[cfg(windows)]
use std::{
    fs::OpenOptions,
    os::windows::{
        fs::OpenOptionsExt,
        io::{FromRawHandle, IntoRawHandle},
    },
};

use anyhow::{anyhow, bail, Result};
use crossbeam::channel::{self, Receiver, Sender};
#[cfg(unix)]
use mio::net::UnixStream;
#[cfg(windows)]
use mio::windows::NamedPipe;
use mio::{Events, Interest, Poll, Token};

use crate::{
    args::ClientCommand, client::handle::CommandHandler, connection::*, ctrlc::*,
    message::prelude::*,
};

const SERVER: Token = Token(0);

#[cfg(windows)]
const FILE_FLAG_OVERLAPPED: u32 = 0x40000000;

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
    /// Name of the socket to listen on.
    pub unix_socket_name: &'static str,

    #[cfg(windows)]
    /// Name of the socket to listen on.
    pub windows_pipe_name: &'static str,
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
            windows_pipe_name: crate::WINDOWS_PIPE_NAME,
        }
    }
}

pub fn run(cmd: ClientCommand, config: Config) -> Result<()> {
    // Set up ctrlc handling
    let running = set_ctrlc_handler()?;

    // Set up message channels
    let (request_tx, request_rx) = channel::bounded::<(u32, Request)>(config.request_capacity);
    let (response_tx, response_rx) = channel::bounded::<(u32, Response)>(config.response_capacity);
    let mut cmd_handler = CommandHandler::new(request_tx, response_rx);

    // TODO: What is behavior if this returns an error? Should we do something other than unwrap?
    let r = running.clone();
    let io_handle =
        std::thread::spawn(|| run_client_io_loop(r, config, request_rx, response_tx).unwrap());

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

    Ok(())
}

/// Run the client IO loop with the server.
///
/// Backpressure is provided in two directions:
/// 1. Requests are only written when the stream is writable, so the request channel will fill, blocking senders.
/// 2. Responses are capped by response channel capacity, so when processing is slow, reading will block. Note that
///    blocked reading will also block writing requests.
/// TODO: Consider changing read interest rather than blocking everything in response_tx.send(), then doing a re-enable
/// check at the top (check after adding interests to the connection)
pub fn run_client_io_loop(
    running: RunToken,
    config: Config,
    request_rx: Receiver<(u32, Request)>,
    response_tx: Sender<(u32, Response)>,
) -> Result<()> {
    #[cfg(unix)]
    let stream = UnixStream::connect(config.unix_socket_name)?;

    // TODO: Likely move this to its own file, also work out why WRITABLE is not being disabled properly
    // The call seems to be going through, but write events are still coming through
    #[cfg(windows)]
    let stream = {
        eprintln!("starting open");
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .open(config.windows_pipe_name)?
            .into_raw_handle();
        eprintln!("finished open");
        // TODO: Add safety notes, see https://doc.rust-lang.org/nightly/std/os/windows/io/trait.FromRawHandle.html
        // SAFETY:
        unsafe { NamedPipe::from_raw_handle(handle) }
    };

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(config.event_capacity);
    let mut conn = Connection::new_client(&mut poll, SERVER, stream)?;

    while running == true {
        // Re-enable write interest if there are items in the request channel
        // In the client we don't pre-buffer sends, we just process as-ready
        if !request_rx.is_empty() {
            //conn.enable_interest(&mut poll, Interest::WRITABLE)?;
        }

        poll.poll(&mut events, Some(config.io_poll_timeout))?;

        for event in &events {
            if event.token() != SERVER {
                continue;
            }

            if event.is_readable() {
                loop {
                    match conn.read() {
                        ReadResult::Request(_) => unreachable!(),
                        // If the channel send fails, we can't proceed, so shut down
                        // This send call will block if the channel is full, providing backpressure
                        ReadResult::Response(res) => response_tx.send(res)?,
                        ReadResult::Continue => {}
                        ReadResult::WouldBlock => break,
                        ReadResult::Eof => bail!("Connection closed unexpectedly"),
                        ReadResult::Error(err) => bail!(err),
                    }
                }
            }

            // When we are writable, we only write if there is a pending message, taking straight from the channel
            if event.is_writable() {
                #[cfg(windows)]
                // The named pipe does not appear to be properly re-registering without write interest in the below. So
                // we control it manually instead using our internal tracking
                // BUG: Mio seems to be not re-registering properly
                if !conn.is_writeable() {
                    break;
                }
                loop {
                    match conn.write_from_channel(&request_rx) {
                        WriteResult::Continue => {}
                        WriteResult::Drained => {
                            // No longer interested in writes until write buffer is refilled
                            // If this fails, stream is unusable, so exit
                            conn.disable_interest(&mut poll, Interest::WRITABLE)?;
                            break;
                        }
                        WriteResult::WouldBlock => break,
                        WriteResult::Eof => bail!("Connection closed unexpectedly"),
                        WriteResult::Error(err) => bail!(err),
                    }
                }
            }
        }
    }

    Ok(())
}
