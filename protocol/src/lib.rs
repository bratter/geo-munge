//! Geo-munge protocol library.
//!
//! Contains shared types and conversion logic for client-server communication. Includes request and response types and
//! wire encoding logic. To be shared between the server and all clients.

mod feature;
mod properties;
mod request;
mod response;

pub mod prelude {
    pub use super::feature::JsonFeature;
    pub use super::properties::Properties;
    pub use super::request::*;
    pub use super::response::*;
    pub use super::CustomKey;
    pub use super::IoCodec;
    pub use super::Uid;
}

use std::fmt::LowerHex;

use anyhow::{anyhow, bail, Result};
use bincode::{Decode, Encode};

/// Request/response encoding and decoding trait.
///
/// TODO: Revisit this when we change transport formats, maybe we don't need a trait at all
/// If we do want a trait, then see if we can arrange it such that it is built in the network crate
pub trait IoCodec
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

/// Representation of a unique identifier for a record.
///
/// TODO: Should this be a newtype instead?
/// TODO: Should this just be 64bit?
pub type Uid = u32;

/// Representation of a fixed-width custom key for a record.
///
/// TODO: Push the bytes into the lower end of the custom key?
/// TODO: Make this work with numeric JSON values, or at least not insert them?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub struct CustomKey([u8; 16]);

impl TryFrom<&[u8]> for CustomKey {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() <= 16 {
            let mut bytes = [0u8; 16];
            bytes[0..value.len()].copy_from_slice(value);
            Ok(CustomKey(bytes))
        } else {
            bail!("Key should be 16 bytes or less");
        }
    }
}

impl TryFrom<&geojson::JsonValue> for CustomKey {
    type Error = anyhow::Error;

    fn try_from(value: &geojson::JsonValue) -> Result<Self, Self::Error> {
        match value {
            geojson::JsonValue::Number(n) => n
                .as_i64()
                .ok_or(anyhow!("Cannot cast to i64"))?
                .to_le_bytes()
                .as_slice()
                .try_into(),
            geojson::JsonValue::String(s) => s.as_bytes().try_into(),
            _ => bail!("Field is not a string or i64"),
        }
    }
}

impl LowerHex for CustomKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for bytes in self.0.chunks_exact(4) {
            if !first {
                write!(f, " ")?;
            }
            first = false;

            write!(
                f,
                "{:02x}{:02x}{:02x}{:02x}",
                bytes[0], bytes[1], bytes[2], bytes[3]
            )?;
        }
        Ok(())
    }
}
