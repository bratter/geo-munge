use std::io::{BufReader, BufWriter};

use anyhow::Result;
use interprocess::local_socket::{prelude::*, GenericNamespaced};

use crate::{message::Message, SOCKET_NAME};

// TODO: This currently just sends one message, receives a response, then closes. Is this what we want?
pub fn run(msg: &str) -> Result<()> {
    let socket_name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let stream = LocalSocketStream::connect(socket_name)?;

    let mut reader = BufReader::new(&stream);
    let mut writer = BufWriter::new(&stream);

    // Send message stage
    println!("Attempting to write to socket");
    let message = Message::Msg(msg.to_string());
    message.write(&mut writer)?;
    println!("Wrote to socket, awaiting response");

    // Receive reply stage
    if let Some(res) = Message::read(&mut reader)? {
        match res {
            Message::Ack(ack) => println!("Client received: {:?}", ack),
            _ => println!("Shouldn't be here, this is an error"),
        }
    }

    Ok(())
}
