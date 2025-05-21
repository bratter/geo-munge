use std::{
    sync::mpsc::{Receiver, Sender, TryRecvError},
    time::Duration,
};

use anyhow::{anyhow, bail, Result};
use mio::{net::UnixStream, Events, Poll, Token};

use crate::{
    args::ClientCommand, client::handle::CommandHandler, connection::*, ctrlc::*,
    message::prelude::*, UNIX_SOCKET_NAME,
};

const SERVER: Token = Token(0);

pub struct Config {
    /// The length of time poll will block before falling through. Higher values mean longer before the system will
    /// check for outgoing responses. Recommended range 10-50ms, but client can be more relaxed (at least toward the
    /// upper end) as checking for outgoing messages is not as time critical.
    pub io_poll_timeout: Duration,

    /// Name of the socket to listen on.
    pub unix_socket_name: &'static str,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            io_poll_timeout: Duration::from_millis(50),
            unix_socket_name: UNIX_SOCKET_NAME,
        }
    }
}

pub fn run(cmd: ClientCommand, config: Config) -> Result<()> {
    // Set up ctrlc handling
    let running = set_ctrlc_handler()?;

    // Set up message channels
    let (request_tx, request_rx) = std::sync::mpsc::channel::<(u32, Request)>();
    let (response_tx, response_rx) = std::sync::mpsc::channel::<(u32, Response)>();
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

// TODO: Would it be better to register any file or stdio here also rather than handling separately in the app?
pub fn run_client_io_loop(
    running: RunToken,
    config: Config,
    request_rx: Receiver<(u32, Request)>,
    response_tx: Sender<(u32, Response)>,
) -> Result<()> {
    let stream = UnixStream::connect(config.unix_socket_name)?;
    let mut poll = Poll::new()?;
    // TODO: Tune this, put in const or make dynamic
    let mut events = Events::with_capacity(16);
    let mut conn = Connection::new_client(&mut poll, SERVER, stream)?;

    while running == true {
        // In the client we don't need to transfer from the channel to the write queue, but it is easier to just do so
        // TODO: This likely means we have two backpressure mechanisms if we cap the queue size - check what works so we
        // are not slowing things down - likely should not modify read interest ever as the server will only send back
        // what we give them, but if we are slow processing the reads, eventually the server will just backpressure our
        // writes - client therefore just needs to manage the size of its write queue, so probably set a max length here
        // and in the channel
        loop {
            match request_rx.try_recv() {
                Ok(req) => {
                    conn.push_write_queue(req);
                    // We are now interested in listening for writes
                    // If this fails the stream is unusable (could retry, but too complex given liklihood)
                    conn.enable_write_interest(&mut poll)?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => bail!("Request channel disconnected"),
            }
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
                        // If the channel send fails, we can't procedd, so shut down
                        ReadResult::Response(res) => response_tx.send(res)?,
                        ReadResult::Continue => {}
                        ReadResult::WouldBlock => break,
                        // TODO: Is this really a connection closed? Is it really unexpected (I think so as the client
                        // should be the one hanging up? Just delete this when comfortable
                        ReadResult::Eof => bail!("Connection closed unexpectedly"),
                        ReadResult::Error(err) => bail!(err),
                    }
                }

            // When we are writable, we only write if there is a pending message
            } else if event.is_writable() {
                loop {
                    match conn.write() {
                        WriteResult::Continue => {}
                        // TODO: Somewhere in here we should think about re-enabling reads, if we are using this as
                        // throttling on the client
                        WriteResult::Drained => {
                            // No longer interested in writes until write buffer is refilled
                            // If this fails, stream is unusable, so exit
                            conn.disable_write_interest(&mut poll)?;
                            break;
                        }
                        WriteResult::WouldBlock => break,
                        // TODO: See note in is_readable about this indeed being unexpected
                        WriteResult::Eof => bail!("Connection closed unexpectedly"),
                        WriteResult::Error(err) => bail!(err),
                    }
                }
            }
        }
    }

    Ok(())
}
