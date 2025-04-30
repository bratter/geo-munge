//! Message protocol for GM-Proximity.

use std::io::{ErrorKind, Read, Write};

use anyhow::{bail, Context, Result};

pub fn read_message<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => { /* good */ }
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None), // EOF, no message
        Err(e) => return Err(e).context("Failed to read message length"),
    }

    let msg_len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; msg_len];

    match reader.read_exact(&mut buf) {
        Ok(()) => Ok(Some(buf)),
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
            bail!("Unexpected EOF while reading message body");
        }
        Err(e) => bail!("Failed to read message body with error: {}", e),
    }
}

pub fn write_message<W: Write>(writer: &mut W, msg: &[u8]) -> Result<()> {
    let len = (msg.len() as u32).to_be_bytes();

    writer.write_all(&len)?;
    writer.write_all(msg)?;
    writer.flush()?;

    Ok(())
}
