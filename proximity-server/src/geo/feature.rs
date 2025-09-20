//! Domain-specific feature types for gm-proximity.
//!
//! Contains the core [`Feature`] struct that represents a parsed spatial feature
//! with coordinates converted to radians for internal processing.

use anyhow::{anyhow, Result};
use geo::{Geometry, ToDegrees, ToRadians};
use protocol::prelude::*;

/// A parsed spatial feature without ID assignment, with coordinates in radians.
///
/// This represents a feature that has been parsed and had its coordinates
/// converted to radians, but doesn't yet have a server-assigned ID.
/// May contain an optional provided_key for client-specified primary keys.
#[derive(Debug, Clone)]
pub struct ParsedFeature {
    pub geometry: Geometry<f64>,
    pub properties: Option<Properties>,
    pub provided_key: Option<Uid>, // Optional client-provided primary key
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

        // Extract properties - GeoJSON doesn't have provided_key concept
        let properties = feature.properties.map(Properties::from);

        Ok(ParsedFeature {
            geometry,
            properties,
            provided_key: None, // GeoJSON doesn't support provided keys
        })
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
    use crate::geo::GeoStore;

    use super::*;

    #[test]
    fn test_geojson_feature_roundtrip() {
        // Create a test GeoJSON feature
        let geojson_feature = geojson::Feature {
            bbox: None,
            geometry: Some(geojson::Geometry::new(geojson::Value::Point(vec![
                45.0, 90.0,
            ]))),
            // FIX: Currently not handling geojson id, when protocol is updated, this behavior will need to be built
            // into the client
            id: Some(geojson::feature::Id::Number(123.into())),
            properties: None,
            foreign_members: None,
        };
        let parsed_feature = ParsedFeature::try_from(geojson_feature.clone()).unwrap();

        // Create a store and insert the feature, then extract it
        let store = GeoStore::default();
        store.insert(parsed_feature).unwrap();
        let geo_feature = &store.get(&0).unwrap().data;

        // Verify ID is correct
        assert_eq!(geo_feature.id, 0);

        // Verify geometry is converted to radians
        if let Geometry::Point(p) = &geo_feature.geometry {
            assert!((p.x() - 45.0f64.to_radians()).abs() < 1e-10);
            assert!((p.y() - 90.0f64.to_radians()).abs() < 1e-10);
        } else {
            panic!("Expected Point geometry");
        }

        // Convert back to GeoJSON
        let roundtrip_feature = geojson::Feature::from(geo_feature);

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
