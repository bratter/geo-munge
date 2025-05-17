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
    let io_handle = std::thread::spawn(|| run_io_loop(request_rx, response_tx));

    // TODO: Actually get the right request type (or loop if using CLI)
    for i in 0..10 {
        eprintln!("sending req {}", i);
        request_tx.send(Request::Stats)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        // This should just recieve responses 1 by 1... not sophisticated
        let res = response_rx.recv()?;
        eprintln!("got response: {:?}", res);
    }

    // TODO: Better join handling
    io_handle.join().unwrap().unwrap();

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

    // TODO: This will be removed
    let mut pending_write: Option<Vec<u8>> = None;
    let mut write_cursor = 0;

    // TODO: Consider graceful exit of clients also? Probably worth it for some types of long-lived clients
    // TODO: Consider making the pending writes a VecDeq and piling more up to write
    loop {
        // First encode and queue up message to send
        // Only need to queue a single message at a time
        if pending_write.is_none() {
            match request_rx.try_recv() {
                // TODO: There is some way of doing the pending write with write_vectored and IoSlice::advance_slices to
                // appropriately manage prepending the length in a more efficienct way than re-allocating a Vec, but
                // will be difficult to get right
                Ok(req) => match req.encode() {
                    Ok(bytes) => {
                        // We are now interested in listening for writes
                        // If this fails the stream is unusable (could retry, but too complex given liklihood)
                        conn.enable_write_interest(&mut poll)?;

                        let mut bytes_with_len: Vec<_> = (bytes.len() as u32).to_le_bytes().into();
                        bytes_with_len.extend_from_slice(&bytes);
                        pending_write = Some(bytes_with_len);
                        write_cursor = 0;
                    }
                    Err(err) => eprintln!("Failed to encode request: {}", err),
                },
                Err(TryRecvError::Disconnected) => {
                    bail!("Request channel disconnected");
                }
                Err(TryRecvError::Empty) => {}
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
                        // TODO: See note in is_writeable about this indeed being unexpected
                        ReadResult::Eof => bail!("Connection closed unexpectedly"),
                        ReadResult::Error(err) => bail!(err),
                    }
                }
            }

            // When we are writable, we only write if there is a pending message
            // TODO: Given the need to re-register, should probably outsource to a writer struct, that also should
            // manage the write_vectored (if possible) and a larger buffer that pulls more items out of the channel at
            // once; could also be a state machine if we can't get the write vectored working
            if event.is_writable() {
                while let Some(buf) = &pending_write {
                    match conn.write(&buf[write_cursor..]) {
                        // TODO: Is this really a connection closed? Is it really unexpected (I think so as the client
                        // should be the one hanging up?
                        Ok(0) => bail!("Connection closed unexpectedly"),
                        Ok(n) => {
                            write_cursor += n;
                            if write_cursor >= buf.len() {
                                // Reset the write buffer
                                pending_write = None;
                                write_cursor = 0;

                                // No longer interested in writes until write buffer is refilled
                                // If this fails, stream is unusable, so exist
                                conn.disable_write_interest(&mut poll)?;
                                break;
                            }
                        }
                        Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                        Err(err) => {
                            bail!("Stream write error: {}", err);
                        }
                    }
                }
            }
        }
    }
}
