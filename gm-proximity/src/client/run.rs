use std::io::{BufReader, BufWriter};

use anyhow::Result;
use interprocess::local_socket::{prelude::*, GenericNamespaced};

use crate::{args::ClientCommand, client::handle::CommandHandler, SOCKET_NAME};

// TODO: See notes in the handler on alternative flow
pub fn run(cmd: ClientCommand) -> Result<()> {
    let socket_name = SOCKET_NAME.to_ns_name::<GenericNamespaced>()?;
    let stream = LocalSocketStream::connect(socket_name)?;

    let mut reader = BufReader::new(&stream);
    let mut writer = BufWriter::new(&stream);
    let mut handler = CommandHandler::new(&mut reader, &mut writer);

    handler.handle(cmd)
}
