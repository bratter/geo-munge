//! Message protocol for GM-Proximity.

use std::io::{ErrorKind, Read, Write};

use anyhow::{bail, Result};
use bincode::{Decode, Encode};

// TODO: Document the semantics of read and write
// TODO: Improve read/write handling (maybe not use read_exact, maybe make use of borrowing
pub trait MessageStream
where
    Self: Decode<()> + Encode + Sized,
{
    fn read<R: Read>(reader: &mut R) -> Result<Option<Self>> {
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(()) => { /* good */ }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                return Ok(None); // EOF, no message
            }
            Err(e) => bail!("Failed to read message length: {}", e),
        }

        let msg_len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; msg_len];

        match reader.read_exact(&mut buf) {
            Ok(()) => {
                let config = bincode::config::standard();
                let (msg, _) = bincode::decode_from_slice::<Self, _>(&buf, config)?;
                Ok(Some(msg))
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                bail!("Unexpected EOF while reading message body");
            }
            Err(e) => bail!("Failed to read message body with error: {}", e),
        }
    }

    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        let config = bincode::config::standard();
        let serialized = bincode::encode_to_vec(self, config)?;
        let len = (serialized.len() as u32).to_be_bytes();

        writer.write_all(&len)?;
        writer.write_all(&serialized)?;
        writer.flush()?;

        Ok(())
    }
}
