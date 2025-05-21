//! Request/response encoding and decoding trait for GM-Proximity.

use anyhow::Result;
use bincode::{Decode, Encode};

pub trait IoEncode
where
    Self: Decode<()> + Encode + Sized,
{
    fn decode_from_slice(buf: &[u8]) -> Result<Self> {
        let config = bincode::config::standard();
        let (res, _) = bincode::decode_from_slice::<Self, _>(&buf, config)?;

        Ok(res)
    }

    fn encode_to_vec(&self) -> Result<Vec<u8>> {
        let config = bincode::config::standard();
        let bytes = bincode::encode_to_vec(self, config)?;

        Ok(bytes)
    }
}
