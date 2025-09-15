//! Data-related structures for the protocol.

use std::str::FromStr;

use anyhow::{bail, Error, Result};
use bincode::{Decode, Encode};
use geojson::JsonValue;

use crate::{feature::JsonFeature, properties::Properties};

/// Content mode for query response output.
#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub enum ContentMode {
    /// Return no additional content, IDs only.
    #[default]
    None,

    /// Return full GeoJSON features with properties and geometry.
    Full,

    /// Return GeoJSON geometry only, without properties.
    Geometry,

    /// Return properties only as JSON.
    Properties,
}

impl ContentMode {
    fn as_str(&self) -> &str {
        match self {
            Self::None => "None",
            Self::Full => "Full Feature",
            Self::Geometry => "Geometry",
            Self::Properties => "Properties",
        }
    }

    pub fn list() -> [&'static str; 4] {
        [
            Self::None.as_str(),
            Self::Full.as_str(),
            Self::Geometry.as_str(),
            Self::Properties.as_str(),
        ]
    }
}

impl FromStr for ContentMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "none" | "id" | "ids" => Ok(Self::None),
            "full" | "feature" => Ok(Self::Full),
            "geometry" | "geom" => Ok(Self::Geometry),
            "properties" | "props" | "meta" => Ok(Self::Properties),
            _ => bail!(
                "Invalid content mode '{}'. Valid options: full, geometry, properties, none",
                s
            ),
        }
    }
}

// TODO: Should these be moved into a newtype in the repl module?
impl TryFrom<usize> for ContentMode {
    type Error = Error;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Full),
            2 => Ok(Self::Geometry),
            3 => Ok(Self::Properties),
            _ => bail!("Invalid index for ContentMode"),
        }
    }
}
/// Content type for query results - determines what additional data is returned with the ID.
#[derive(Debug, Encode, Decode)]
pub enum ContentType {
    /// Full GeoJSON feature with properties and geometry.
    FullFeature(JsonFeature),

    /// GeoJSON geometry only, without properties.
    GeometryOnly(JsonFeature),

    /// Properties only as JSON value.
    PropertiesOnly(Properties),

    /// No additional content, ID only.
    None,
}

impl ContentType {
    /// Set a property on the content type.
    ///
    /// When the content type is None, this will change the content type to properties only, enabling the addition of
    /// properties in a mutable manner downstream from origination. This lets us inject results data in the response.
    pub fn set_property(&mut self, key: impl Into<String>, value: impl Into<JsonValue>) {
        match self {
            ContentType::FullFeature(feature) => feature.0.set_property(key, value),
            ContentType::GeometryOnly(feature) => feature.0.set_property(key, value),
            ContentType::PropertiesOnly(properties) => properties.set_property(key, value),
            ContentType::None => {
                let mut new_properties = Properties::default();
                new_properties.set_property(key, value);
                let _ = std::mem::replace(self, ContentType::PropertiesOnly(new_properties));
            }
        }
    }
}
