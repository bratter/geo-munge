use std::io::{BufReader, BufWriter, Read};

use anyhow::{bail, Result};
use interprocess::local_socket::{prelude::*, GenericNamespaced};

use crate::{
    args::ClientCommand,
    message::{MessageStream, Request, Response},
    SOCKET_NAME,
};

// TODO: This currently just sends one message, receives a response, then closes. Is this what we want?
pub fn run(cmd: ClientCommand) -> Result<()> {
    let socket_name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let stream = LocalSocketStream::connect(socket_name)?;

    let mut reader = BufReader::new(&stream);
    let mut writer = BufWriter::new(&stream);

    // TODO: The msg type is only temporary, drop the returns too.
    match cmd {
        ClientCommand::Stats => {
            Request::Stats.write(&mut writer)?;
            return read_results(&mut reader);
        }
        ClientCommand::Msg { message } => println!("Recieved {}... continuing...", message),
    }

    // We should be able to do this multiple times
    println!("Attempting to write to socket");
    Request::Stats.write(&mut writer)?;
    println!("Wrote to socket, awaiting response");
    read_results(&mut reader)?;

    Request::Insert(
        br#"{"type": "Feature", "geometry": {"type": "Point", "coordinates":[0,0]}}"#
            .to_vec()
            .into(),
    )
    .write(&mut writer)?;
    read_results(&mut reader)?;

    Request::Stats.write(&mut writer)?;
    read_results(&mut reader)?;

    Ok(())
}

// TODO: This is only a tmp function for use during build
fn read_results(reader: &mut impl Read) -> Result<()> {
    let res = Response::read(reader)?;

    if res.is_none() {
        bail!("None when something was expected");
    }

    match res.unwrap() {
        Response::Success(Some(msg)) => println!("{}", msg),
        Response::Success(None) => println!("success"),
        Response::Stats(n) => println!("The qt has {} items", n),
        Response::Data => println!("data"),
        Response::Error(msg) => println!("There was an error: {}", msg),
    };

    Ok(())
}
