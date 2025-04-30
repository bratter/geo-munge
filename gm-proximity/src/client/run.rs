use std::io::{BufReader, BufWriter};

use anyhow::Result;
use interprocess::local_socket::{prelude::*, GenericNamespaced};

use crate::{
    message::{read_message, write_message},
    SOCKET_NAME,
};

// TODO: This currently just sends one message, receives a response, then closes. Is this what we want?
pub fn run(msg: &str) -> Result<()> {
    let socket_name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let stream = LocalSocketStream::connect(socket_name)?;

    let mut reader = BufReader::new(&stream);
    let mut writer = BufWriter::new(&stream);

    // Send message stage
    println!("Attempting to write to socket");
    write_message(&mut writer, msg.as_bytes())?;
    println!("Wrote to socket, awaiting response");

    // Receive reply stage
    if let Some(buf) = read_message(&mut reader)? {
        let reply = String::from_utf8_lossy(&buf);
        println!("Client received: {}", reply);
    }

    Ok(())
}
