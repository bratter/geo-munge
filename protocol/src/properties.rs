use std::{
    fmt::{Debug, Display},
    str::FromStr,
};

use anyhow::bail;
use bincode::{BorrowDecode, Decode, Encode};
use geojson::{JsonObject, JsonValue};

/// Stored property data as a newtype around a JsonObject for ergonomics.
///
/// Metadata is just JSON values, use JSON pointer syntax for extraction
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
