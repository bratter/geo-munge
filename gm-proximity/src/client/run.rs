use std::{
    io::{BufReader, BufWriter, ErrorKind, IoSlice, Read, Write},
    sync::mpsc::{Receiver, Sender, TryRecvError},
    time::Duration,
};

use anyhow::{bail, Result};
use interprocess::local_socket::{prelude::*, GenericNamespaced};
use mio::{net::UnixStream, Events, Interest, Poll, Token};

use crate::{
    args::ClientCommand, client::handle::CommandHandler, connection::*, message::prelude::*,
    SOCKET_NAME, UNIX_SOCKET_NAME,
};

// TODO: See notes in the handler on alternative flow
pub fn run(cmd: ClientCommand) -> Result<()> {
    // TODO: Just testing out the mio-based loop

    let (request_tx, request_rx) = std::sync::mpsc::channel::<Request>();
    let (response_tx, response_rx) = std::sync::mpsc::channel::<Response>();
    // TODO: What is behavior if this returns an error? Should we at least log/print?
    let io_handle = std::thread::spawn(|| run_io_loop(request_rx, response_tx).unwrap());

    // TODO: Actually get the right request type (or loop if using CLI)
    for i in 0..10 {
        eprintln!("sending req {}", i);
        request_tx.send(Request::Stats)?;
        std::thread::sleep(std::time::Duration::from_millis(1000));
        // This should just recieve responses 1 by 1... not sophisticated
        let res = response_rx.recv()?;
        eprintln!("got response: {:?}", res);
    }

    // TODO: Better join handling
    io_handle.join().unwrap();

    /*
        let socket_name = UNIX_SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
        let stream = LocalSocketStream::connect(socket_name)?;

        let mut reader = BufReader::new(&stream);
        let mut writer = BufWriter::new(&stream);
        let mut handler = CommandHandler::new(&mut reader, &mut writer);

        handler.handle(cmd)
    */
    Ok(())
}

// TODO: Building mio event loop, consider moving out into its own file... but much simple so probably works just in run

const CLIENT: Token = Token(0);
// TODO: Tune, maybe set same as server and push to parent module (target 10-50ms)
const POLL_TIMEOUT: Duration = Duration::from_millis(1000);

pub fn run_io_loop(request_rx: Receiver<Request>, response_tx: Sender<Response>) -> Result<()> {
    let stream = UnixStream::connect(UNIX_SOCKET_NAME)?;
    let mut poll = Poll::new()?;
    // TODO: Tune this, put in const or make dynamic
    let mut events = Events::with_capacity(16);
    let mut conn = Connection::new_client(&mut poll, CLIENT, stream)?;

    // TODO: Consider graceful exit of clients also? Probably worth it for some types of long-lived clients
    loop {
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
                    conn.enable_write_interest(&mut poll);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => bail!("Request channel disconnected"),
            }
        }

        poll.poll(&mut events, Some(POLL_TIMEOUT))?;

        for event in &events {
            if event.token() != CLIENT {
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
}
