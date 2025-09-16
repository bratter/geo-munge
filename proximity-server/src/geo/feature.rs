//! Domain-specific feature types for gm-proximity.
//!
//! Contains the core [`Feature`] struct that represents a parsed spatial feature
//! with coordinates converted to radians for internal processing.

use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{anyhow, Result};
use geo::{Geometry, ToDegrees, ToRadians};
use protocol::prelude::*;

pub enum KeyGenerator {
    AutoIncrement(AtomicU32),
    U32Pointer(String),
    MetaPointer(AtomicU32, String),
    GeoJsonId,
}

// TODO: Where should these conversions sit? Maybe better in the Request module?
impl From<KeyMode> for KeyGenerator {
    fn from(value: KeyMode) -> Self {
        match value {
            KeyMode::AutoIncrement => Self::AutoIncrement(0.into()),
            KeyMode::U32Pointer(ptr) => Self::U32Pointer(ptr),
            KeyMode::MetaPointer(ptr) => Self::MetaPointer(0.into(), ptr),
            KeyMode::GeoJsonId => Self::GeoJsonId,
        }
    }
}

impl From<&KeyGenerator> for KeyMode {
    fn from(value: &KeyGenerator) -> Self {
        match value {
            KeyGenerator::AutoIncrement(_) => KeyMode::AutoIncrement,
            KeyGenerator::U32Pointer(ptr) => KeyMode::U32Pointer(ptr.clone()),
            KeyGenerator::MetaPointer(_, ptr) => KeyMode::MetaPointer(ptr.clone()),
            KeyGenerator::GeoJsonId => KeyMode::GeoJsonId,
        }
    }
}

impl Default for KeyGenerator {
    fn default() -> Self {
        Self::AutoIncrement(0.into())
    }
}

/// A parsed spatial feature without ID assignment, with coordinates in radians.
///
/// This represents a GeoJSON feature that has been parsed and had its coordinates
/// converted to radians, but doesn't yet have a server-assigned ID.
/// FIX: geojson shouldn't be in the server at all - check if this type is used on the server - it might be now, but
/// then should go away when we change the encoding protocol.
/// FIX: In the server at least, this concept should be called AnonymousFeature or something
#[derive(Debug, Clone)]
pub struct ParsedFeature {
    pub geometry: Geometry<f64>,
    pub properties: Option<Properties>,
    pub native_id: Option<geojson::feature::Id>,
}

/// A fully identified spatial feature with coordinates in radians.
///
/// This is the core data structure used throughout the system for storing
/// spatial features. Coordinates are converted from degrees (GeoJSON standard)
/// to radians during parsing for efficient distance calculations.
#[derive(Debug, Clone)]
pub struct Feature {
    pub id: Uid,
    pub geometry: Geometry<f64>,
    pub properties: Option<Properties>,
}

impl Feature {
    /// Convert a [`Feature`] to the correct [`ContentType`] for responses based on this mode.
    pub fn generate_content(&self, content_mode: ContentMode) -> ContentType {
        match content_mode {
            ContentMode::None => ContentType::None,
            ContentMode::Full => ContentType::FullFeature(geojson::Feature::from(self).into()),
            ContentMode::Geometry => {
                let geom: geojson::Feature = geojson::Geometry::from(self.as_ref()).into();
                ContentType::GeometryOnly(geom.into())
            }
            ContentMode::Properties => ContentType::PropertiesOnly(self.into()),
        }
    }
}

impl TryFrom<geojson::Feature> for ParsedFeature {
    type Error = anyhow::Error;

    fn try_from(feature: geojson::Feature) -> Result<Self> {
        // Extract geometry and convert coordinates from degrees to radians in-place
        let json_geometry = feature
            .geometry
            .ok_or_else(|| anyhow!("Feature must have geometry"))?;

        let mut geometry: Geometry<f64> = json_geometry.try_into()?;

        // Convert coordinates from degrees to radians in-place
        geometry.to_radians_in_place();

        // Extract properties and preserve native ID
        let properties = feature.properties.map(Properties::from);
        let native_id = feature.id;

        Ok(ParsedFeature {
            geometry,
            properties,
            native_id,
        })
    }
}

impl ParsedFeature {
    /// Convery into a Feature with an id given the passed [`KeyGenerator`].
    pub fn with_key_generator(self, key_gen: &KeyGenerator) -> Result<Feature> {
        let id = self.resolve_id(key_gen)?;

        Ok(Feature {
            id,
            geometry: self.geometry,
            properties: self.properties,
        })
    }

    /// Resolve the ID for this feature using the given KeyGenerator.
    ///
    /// This method handles all ID assignment logic including auto-increment.
    ///
    /// Because it handles side effects, this means that it should not be called arbitrarily and is therefore a private
    /// function.
    fn resolve_id(&self, key_gen: &KeyGenerator) -> Result<Uid> {
        match key_gen {
            KeyGenerator::AutoIncrement(counter) => Ok(counter.fetch_add(1, Ordering::Relaxed)),
            KeyGenerator::U32Pointer(pointer) => self.extract_u32_from_properties(&pointer),
            KeyGenerator::MetaPointer(counter, _) => Ok(counter.fetch_add(1, Ordering::Relaxed)),
            KeyGenerator::GeoJsonId => self.extract_from_native_id(),
        }
    }

    fn extract_u32_from_properties(&self, pointer: &str) -> Result<Uid> {
        let properties = self
            .properties
            .as_ref()
            .ok_or_else(|| anyhow!("Properties required for custom key extraction"))?;

        let value = properties
            .pointer(pointer)
            .ok_or_else(|| anyhow!("Value not available at pointer {}", pointer))?;

        match value {
            geojson::JsonValue::Number(n) => {
                let num = n
                    .as_u64()
                    .ok_or_else(|| anyhow!("Property value must be a positive integer"))?;
                Ok(u32::try_from(num)?)
            }
            geojson::JsonValue::String(s) => Ok(s.parse::<u32>()?),
            _ => Err(anyhow!("Property must be a number or string")),
        }
    }

    fn extract_from_native_id(&self) -> Result<Uid> {
        match &self.native_id {
            Some(geojson::feature::Id::Number(n)) => {
                let num = n
                    .as_u64()
                    .ok_or_else(|| anyhow!("GeoJSON ID must be a positive integer"))?;
                Ok(u32::try_from(num)?)
            }
            Some(geojson::feature::Id::String(s)) => Ok(s.parse::<u32>()?),
            None => Err(anyhow!("GeoJSON ID mode requires a feature ID")),
        }
    }
}

impl From<&Feature> for geojson::Feature {
    fn from(feature: &Feature) -> Self {
        // Convert geometry back to degrees for GeoJSON output
        // TODO: Can we do this with only a single copy
        let degrees_geometry = feature.geometry.to_degrees();
        let geojson_geometry = geojson::Geometry::from(&degrees_geometry);

        geojson::Feature {
            bbox: None,
            geometry: Some(geojson_geometry),
            id: Some(geojson::feature::Id::Number(feature.id.into())),
            properties: feature.properties.as_ref().map(|p| p.clone().into()),
            foreign_members: None,
        }
    }
}

impl From<&Feature> for geojson::Geometry {
    fn from(feature: &Feature) -> Self {
        let degrees_geometry = feature.geometry.to_degrees();
        geojson::Geometry::from(&degrees_geometry)
    }
}

impl From<Feature> for geojson::Geometry {
    fn from(feature: Feature) -> Self {
        let degrees_geometry = feature.geometry.to_degrees();
        geojson::Geometry::from(&degrees_geometry)
    }
}

impl From<&Feature> for Properties {
    fn from(feature: &Feature) -> Self {
        feature.properties.clone().unwrap_or_default()
    }
}

impl From<Feature> for Properties {
    fn from(feature: Feature) -> Self {
        feature.properties.unwrap_or_default()
    }
}

impl AsRef<Geometry<f64>> for Feature {
    fn as_ref(&self) -> &Geometry<f64> {
        &self.geometry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geojson_feature_roundtrip() {
        // Create a test GeoJSON feature
        let geojson_feature = geojson::Feature {
            bbox: None,
            geometry: Some(geojson::Geometry::new(geojson::Value::Point(vec![
                45.0, 90.0,
            ]))),
            id: Some(geojson::feature::Id::Number(123.into())),
            properties: None,
            foreign_members: None,
        };
        let key_gen = KeyGenerator::GeoJsonId;

        // Convert to our internal representation
        let parsed_feature = ParsedFeature::try_from(geojson_feature.clone()).unwrap();
        let geo_feature = parsed_feature.with_key_generator(&key_gen).unwrap();

        // Verify ID is correct
        assert_eq!(geo_feature.id, 123);

        // Verify geometry is converted to radians
        if let Geometry::Point(p) = &geo_feature.geometry {
            assert!((p.x() - 45.0f64.to_radians()).abs() < 1e-10);
            assert!((p.y() - 90.0f64.to_radians()).abs() < 1e-10);
        } else {
            panic!("Expected Point geometry");
        }

        // Convert back to GeoJSON
        let roundtrip_feature = geojson::Feature::from(&geo_feature);

        // Verify coordinates are back in degrees
        if let Some(geojson::Geometry {
            value: geojson::Value::Point(coords),
            ..
        }) = &roundtrip_feature.geometry
        {
            assert!((coords[0] - 45.0).abs() < 1e-10);
            assert!((coords[1] - 90.0).abs() < 1e-10);
        } else {
            panic!("Expected Point geometry in GeoJSON");
        }
    }
}
