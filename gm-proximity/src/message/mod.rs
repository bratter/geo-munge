//! Message logic for GM-Proximity.
//!
//! Module for message handling between client and server. Includes message types, serialization/deserialization, and
//! structure of inner message types.

use std::{
    fmt::{Debug, Display, LowerHex},
    str::FromStr,
};

use anyhow::{anyhow, bail};
use bincode::{BorrowDecode, Decode, Encode};

mod encode;
mod request;
mod response;

pub mod prelude {
    pub use super::encode::IoCodec;
    pub use super::request::*;
    pub use super::response::*;
    pub use super::Feature;
    pub use super::NodeId;
}

// TODO: These may be better exported from GeoStore, but should probably use a newtype or just the inner to transport so
// we don't pollute GeoStore with Enc/Dec
// TODO: Push the bytes into the lower end of the custom key?
// TODO: Make this work with numeric JSON values, or at least not insert them?
pub type NodeId = u32;

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
            bail!("Key should be 16 characters or less");
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
        for byte in self.0 {
            write!(f, "{:x}", byte)?;
        }
        Ok(())
    }
}

/// Newtype wrapper to enable codec on geojson Features.
pub struct Feature(pub geojson::Feature);

impl Debug for Feature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Feature")
            .field("id", &self.0.id)
            .finish_non_exhaustive()
    }
}

impl From<geojson::Feature> for Feature {
    fn from(value: geojson::Feature) -> Self {
        Feature(value)
    }
}

impl From<Feature> for geojson::Feature {
    fn from(value: Feature) -> Self {
        value.0
    }
}

impl Display for Feature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl FromStr for Feature {
    type Err = geojson::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<geojson::GeoJson>()
            .and_then(|geojson| geojson::Feature::try_from(geojson))
            .map(Feature)
    }
}

impl Encode for Feature {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> std::result::Result<(), bincode::error::EncodeError> {
        Encode::encode(&self.0.to_string(), encoder)
    }
}

impl<Context> Decode<Context> for Feature {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, bincode::error::DecodeError> {
        let str: String = Decode::decode(decoder)?;
        str.parse::<Feature>()
            .map_err(|_| bincode::error::DecodeError::Other("GeoJson decode error"))
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for Feature {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, bincode::error::DecodeError> {
        let str: String = BorrowDecode::borrow_decode(decoder)?;
        str.parse::<Feature>()
            .map_err(|_| bincode::error::DecodeError::Other("GeoJson decode error"))
    }
}

/// Newtype wrapper to enable codec on geojson properties.
#[derive(Debug)]
pub struct JsonValue(pub geojson::JsonValue);

impl From<geojson::JsonValue> for JsonValue {
    fn from(value: geojson::JsonValue) -> Self {
        JsonValue(value)
    }
}

impl From<JsonValue> for geojson::JsonValue {
    fn from(value: JsonValue) -> Self {
        value.0
    }
}

impl FromStr for JsonValue {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(s.parse::<geojson::JsonValue>().map(JsonValue)?)
    }
}

impl Display for JsonValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Encode for JsonValue {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> std::result::Result<(), bincode::error::EncodeError> {
        Encode::encode(&self.0.to_string(), encoder)
    }
}

impl<Context> Decode<Context> for JsonValue {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, bincode::error::DecodeError> {
        let str: String = Decode::decode(decoder)?;
        str.parse()
            .map_err(|_| bincode::error::DecodeError::Other("Json decode error"))
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for JsonValue {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, bincode::error::DecodeError> {
        let str: String = BorrowDecode::borrow_decode(decoder)?;
        str.parse()
            .map_err(|_| bincode::error::DecodeError::Other("Json decode error"))
    }
}
