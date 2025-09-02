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

mod batch;
mod encode;
mod feature;
mod request;
mod response;
pub use batch::dispatch_counted_batches;
use geojson::{JsonObject, JsonValue};

pub mod prelude {
    pub use super::encode::IoCodec;
    pub use super::feature::{Feature, KeyGenerator, ParsedFeature};
    pub use super::request::*;
    pub use super::response::*;
    pub use super::JsonFeature;
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

/// Newtype wrapper to enable codec on geojson Features.
pub struct JsonFeature(pub geojson::Feature);

impl Debug for JsonFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Feature")
            .field("id", &self.0.id)
            .finish_non_exhaustive()
    }
}

impl From<geojson::Feature> for JsonFeature {
    fn from(value: geojson::Feature) -> Self {
        JsonFeature(value)
    }
}

impl From<JsonFeature> for geojson::Feature {
    fn from(value: JsonFeature) -> Self {
        value.0
    }
}

impl Display for JsonFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl FromStr for JsonFeature {
    type Err = geojson::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<geojson::GeoJson>()
            .and_then(|geojson| geojson::Feature::try_from(geojson))
            .map(JsonFeature)
    }
}

impl Encode for JsonFeature {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> std::result::Result<(), bincode::error::EncodeError> {
        Encode::encode(&self.0.to_string(), encoder)
    }
}

impl<Context> Decode<Context> for JsonFeature {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, bincode::error::DecodeError> {
        let str: String = Decode::decode(decoder)?;
        str.parse::<JsonFeature>()
            .map_err(|_| bincode::error::DecodeError::Other("GeoJson decode error"))
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for JsonFeature {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, bincode::error::DecodeError> {
        let str: String = BorrowDecode::borrow_decode(decoder)?;
        str.parse::<JsonFeature>()
            .map_err(|_| bincode::error::DecodeError::Other("GeoJson decode error"))
    }
}

/// Stored property data as a newtype around a JsonObject for ergonomics.
///
/// TODO: Metadata is just JSON values, use JSON pointer syntax for extraction
/// https://datatracker.ietf.org/doc/html/rfc6901
#[derive(Clone, Debug)]
pub struct Properties(JsonValue);

impl Properties {
    pub fn set_property(&mut self, key: impl Into<String>, value: impl Into<JsonValue>) {
        match &mut self.0 {
            JsonValue::Object(obj) => obj.insert(key.into(), value.into()),
            _ => unreachable!(),
        };
    }

    /// Json pointer implementation for our properties type.
    ///
    /// The underlying JsonObject does not implement pointer itself, so we implement it manually, adapted from
    /// https://docs.rs/serde_json/1.0.143/src/serde_json/value/mod.rs.html#779.
    pub fn pointer(&self, pointer: &str) -> Option<&JsonValue> {
        self.0.pointer(pointer)
    }
}

impl Default for Properties {
    fn default() -> Self {
        Properties(JsonValue::Object(JsonObject::default()))
    }
}

impl From<JsonObject> for Properties {
    fn from(value: JsonObject) -> Self {
        Properties(JsonValue::Object(value))
    }
}

impl From<Properties> for JsonObject {
    fn from(value: Properties) -> Self {
        match value.0 {
            JsonValue::Object(obj) => obj,
            _ => unreachable!(),
        }
    }
}

impl Display for Properties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl FromStr for Properties {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = s.parse::<geojson::JsonValue>()?;
        if matches!(value, JsonValue::Object(_)) {
            Ok(Properties(value))
        } else {
            bail!("Properties must be a GeoJSON object")
        }
    }
}

impl Encode for Properties {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> std::result::Result<(), bincode::error::EncodeError> {
        Encode::encode(&self.0.to_string(), encoder)
    }
}

impl<Context> Decode<Context> for Properties {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, bincode::error::DecodeError> {
        let str: String = Decode::decode(decoder)?;
        str.parse()
            .map_err(|_| bincode::error::DecodeError::Other("Json decode error"))
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for Properties {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, bincode::error::DecodeError> {
        let str: String = BorrowDecode::borrow_decode(decoder)?;
        str.parse()
            .map_err(|_| bincode::error::DecodeError::Other("Json decode error"))
    }
}
