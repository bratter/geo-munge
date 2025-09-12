use std::{
    fmt::{Debug, Display},
    str::FromStr,
};

use bincode::{BorrowDecode, Decode, Encode};

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
