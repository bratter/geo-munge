use std::path::Path;

use anyhow::{anyhow, bail, Context, Error, Result};
use kml::types::{Geometry as KmlGeom, *};

use crate::format::{ContentMode, GeoItem, Properties, Value};

/// Reader for KML and KMZ data.
pub struct KmlReader {
    kml: KmlIterator,
    mode: ContentMode,
}

impl KmlReader {
    /// Create a new [`KmlReader`].
    ///
    /// The KML reader reads both .kml and .kmz files.
    ///
    /// This method will eagerly load the file in `file` and parse it immediately in a blocking manner. Subsequent
    /// iteration through the contained items will be lazy, but not the initial creation of the KML tree.
    pub fn try_new(file: impl AsRef<Path>, mode: ContentMode) -> Result<Self> {
        let raw_kml = read_kml(file)?;
        let kml = KmlIterator::new(raw_kml);

        Ok(Self { kml, mode })
    }

    fn make_geoitem(&self, mut item: KmlItem) -> Result<GeoItem> {
        let geoitem = match self.mode {
            ContentMode::Geometry => GeoItem::without_props(geo::Geometry::try_from(item)?),
            ContentMode::Full => {
                let props = extract_properties(&mut item);
                GeoItem::with_props(geo::Geometry::try_from(item)?, props)
            }
            ContentMode::Properties => GeoItem::props_only(extract_properties(&mut item)),
        };

        Ok(geoitem)
    }
}

impl Iterator for KmlReader {
    type Item = Result<GeoItem>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.kml.next()?;
        Some(self.make_geoitem(next))
    }
}

/// The owned KML iterator.
///
/// This iterator only emits the kml components that are useful for proximity processing - i.e. the ones that contain
/// geometries. It attemps to flatten nested KML structures into a single stream of features in line with other formats
/// such as geojson and shapefiles.
#[derive(Default)]
enum KmlIterator {
    Iter(Box<dyn Iterator<Item = KmlItem>>),
    Once(KmlItem),
    #[default]
    Empty,
}

impl KmlIterator {
    /// Recursively build an iterator through the KML document.
    fn new(kml: kml::Kml) -> Self {
        Self::new_with_hierarchy(kml, Vec::new())
    }

    /// Recursively build an iterator with folder hierarchy context.
    fn new_with_hierarchy(kml: kml::Kml, hierarchy: Vec<FolderInfo>) -> Self {
        match kml {
            kml::Kml::KmlDocument(d) => Self::with_elements(d.elements, hierarchy),
            kml::Kml::Document { attrs: _, elements } => Self::with_elements(elements, hierarchy),
            kml::Kml::Folder(f) => {
                let mut folder_hierarchy = hierarchy;
                folder_hierarchy.push(FolderInfo {
                    name: f.name.clone(),
                    description: f.description.clone(),
                });
                Self::with_elements(f.elements, folder_hierarchy)
            }
            kml::Kml::MultiGeometry(d) => Self::Once(KmlItem::MultiGeometry(d, hierarchy)),
            kml::Kml::LinearRing(d) => Self::Once(KmlItem::LinearRing(d, hierarchy)),
            kml::Kml::LineString(d) => Self::Once(KmlItem::LineString(d, hierarchy)),
            kml::Kml::Location(d) => Self::Once(KmlItem::Location(d, hierarchy)),
            kml::Kml::Point(d) => Self::Once(KmlItem::Point(d, hierarchy)),
            kml::Kml::Placemark(d) => Self::Once(KmlItem::Placemark(d, hierarchy)),
            kml::Kml::Polygon(d) => Self::Once(KmlItem::Polygon(d, hierarchy)),
            // Ignore all else
            _ => KmlIterator::Empty,
        }
    }

    /// Create a [`KmlIterator::Iter`] variant from a vec of [`kml::Kml`]s that are present in documents and folders.
    fn with_elements(elements: Vec<kml::Kml>, hierarchy: Vec<FolderInfo>) -> Self {
        let flat_iter = elements
            .into_iter()
            .flat_map(move |k| KmlIterator::new_with_hierarchy(k, hierarchy.clone()));

        KmlIterator::Iter(Box::new(flat_iter))
    }
}

impl Iterator for KmlIterator {
    type Item = KmlItem;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            KmlIterator::Iter(iter) => iter.next(),
            // Swap out the borrowed IntoIter for the new empty state
            // But the compiler "forgets" that we've already matched,
            // hence requiring the if-let
            once @ KmlIterator::Once(_) => {
                if let KmlIterator::Once(item) = std::mem::take(once) {
                    Some(item)
                } else {
                    unreachable!()
                }
            }
            KmlIterator::Empty => None,
        }
    }
}

#[derive(Debug, Clone)]
struct FolderInfo {
    name: Option<String>,
    description: Option<String>,
}

/// Holds a subset of Kml members that might be emitted by the iterator.
#[derive(Debug)]
enum KmlItem {
    MultiGeometry(MultiGeometry, Vec<FolderInfo>),
    LinearRing(LinearRing, Vec<FolderInfo>),
    LineString(LineString, Vec<FolderInfo>),
    Location(Location, Vec<FolderInfo>),
    Placemark(Placemark, Vec<FolderInfo>),
    Point(Point, Vec<FolderInfo>),
    Polygon(Polygon, Vec<FolderInfo>),
}
const MULTIGEOMETRY_FAILURE: &str = "Could not convert kml MultiGeometry to a geometry collection";

impl TryFrom<KmlItem> for geo::Geometry {
    type Error = Error;

    fn try_from(value: KmlItem) -> Result<Self> {
        let geom = match value {
            KmlItem::Placemark(p, _) => match p
                .geometry
                .ok_or(anyhow!("Placemark doesn't have geometry"))?
            {
                KmlGeom::Point(pt) => geo::Geometry::Point(geo::Point::from(pt)),
                KmlGeom::LineString(l) => geo::Geometry::LineString(geo::LineString::from(l)),
                KmlGeom::LinearRing(l) => geo::Geometry::LineString(geo::LineString::from(l)),
                KmlGeom::Polygon(poly) => geo::Geometry::Polygon(geo::Polygon::from(poly)),
                KmlGeom::MultiGeometry(mg) => geo::Geometry::GeometryCollection(
                    geo::GeometryCollection::try_from(mg).context(MULTIGEOMETRY_FAILURE)?,
                ),
                _ => bail!("Extensions are not supported"),
            },
            KmlItem::Point(p, _) => geo::Geometry::Point(geo::Point::from(p)),
            KmlItem::Location(p, _) => {
                geo::Geometry::Point(geo::Point::new(p.longitude, p.latitude))
            }
            KmlItem::LineString(l, _) => geo::Geometry::LineString(geo::LineString::from(l)),
            KmlItem::LinearRing(l, _) => geo::Geometry::LineString(geo::LineString::from(l)),
            KmlItem::Polygon(p, _) => geo::Geometry::Polygon(geo::Polygon::from(p)),
            KmlItem::MultiGeometry(mg, _) => geo::Geometry::GeometryCollection(
                geo::GeometryCollection::try_from(mg).context(MULTIGEOMETRY_FAILURE)?,
            ),
        };

        Ok(geom)
    }
}

/// Return a [`kml::Kml`] object loaded from a `.kml` or `.kmz` file.
fn read_kml(file: impl AsRef<Path>) -> Result<kml::Kml> {
    let path = file.as_ref();
    let ext = path.extension().ok_or_else(|| {
        anyhow!(
            "Can't parse file extension for file {}",
            path.to_string_lossy()
        )
    })?;

    let kml = match ext.to_str() {
        Some("kml") => kml::KmlReader::<_, f64>::from_path(path)?.read()?,
        Some("kmz") => kml::KmlReader::<_, f64>::from_kmz_path(path)?.read()?,
        _ => bail!("Unsupported file extension for kml/kmz files"),
    };

    Ok(kml)
}

/// Convert properties of KML items into JSON values.
///
/// Only Placemarks have relevant property data that we want to extract, all others don't have any properties of their
/// own. However all items should emit their "Folder" structure if it exists.
///
/// All KML fields are typeless text, so are passed through as JSON [`Value::String`] types.
///
/// TODO: Autoparse option for this as well as csv, noting that description is explicitly allowed to contain html
/// TODO: Determine if we want to extract attr and style data from all items, consider keeping it as an option
/// Can search through git history for take_attrs function for methodology
fn extract_properties(item: &mut KmlItem) -> Properties {
    let mut props = Properties::new();

    match item {
        KmlItem::Placemark(p, hierarchy) => {
            // Extract standard KML fields using take to avoid clones
            if let Some(name) = p.name.take() {
                props.insert("name".to_string(), Value::String(name));
            }
            if let Some(description) = p.description.take() {
                props.insert("description".to_string(), Value::String(description));
            }

            // Extract custom elements from children
            for element in std::mem::take(&mut p.children) {
                if let Some(content) = element.content {
                    props.insert(element.name, Value::String(content));
                }
            }

            add_folder_hierarchy_to_props(&mut props, hierarchy);
        }

        // For geometry types, only add folder hierarchy if present
        KmlItem::Point(_, hierarchy)
        | KmlItem::LineString(_, hierarchy)
        | KmlItem::LinearRing(_, hierarchy)
        | KmlItem::Location(_, hierarchy)
        | KmlItem::Polygon(_, hierarchy)
        | KmlItem::MultiGeometry(_, hierarchy) => {
            add_folder_hierarchy_to_props(&mut props, hierarchy);
        }
    };

    props
}

fn add_folder_hierarchy_to_props(props: &mut Properties, hierarchy: &[FolderInfo]) {
    if !hierarchy.is_empty() {
        let folder_hierarchy: Vec<Value> = hierarchy
            .iter()
            .map(|folder| {
                let mut folder_obj = Properties::new();
                if let Some(name) = &folder.name {
                    folder_obj.insert("name".to_string(), Value::String(name.clone()));
                }
                if let Some(description) = &folder.description {
                    folder_obj.insert(
                        "description".to_string(),
                        Value::String(description.clone()),
                    );
                }
                Value::Object(folder_obj)
            })
            .collect();
        props.insert("folders".to_string(), Value::Array(folder_hierarchy));
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn kml_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/sample_kml/sample.kml")
    }

    #[test]
    fn kml_reader_emits_features_full_mode() {
        let kml_reader = KmlReader::try_new(kml_path(), ContentMode::Full).unwrap();
        let features: Vec<_> = kml_reader.map(|result| result.unwrap()).collect();

        assert_eq!(features.len(), 3);

        let first_feature = &features[0];
        assert!(matches!(
            first_feature.geom,
            Some(geo::Geometry::Point(geo::Point(_)))
        ));

        // Check properties - should have name, description, and custom randomProperty
        let first_props = first_feature.props.as_ref().unwrap();
        assert_eq!(first_props.len(), 3);
        assert_eq!(
            first_props.get("name").unwrap(),
            &Value::String("Time Square".to_string())
        );
        assert!(matches!(
            first_props.get("description").unwrap(),
            &Value::String(_)
        ));
        assert_eq!(
            first_props.get("randomProperty").unwrap(),
            &Value::String("42".to_string())
        );

        // Confirm that the second feature is correct too
        assert_eq!(features[1].props.as_ref().unwrap().len(), 2);

        // Third feature should be a LineString with no properties
        let third_feature = &features[2];
        assert!(matches!(
            third_feature.geom,
            Some(geo::Geometry::LineString(_))
        ));
        assert_eq!(third_feature.props.as_ref().unwrap().len(), 0);
    }

    #[test]
    fn kml_reader_emits_features_properties_mode() {
        let kml_reader = KmlReader::try_new(kml_path(), ContentMode::Properties).unwrap();
        let features: Vec<_> = kml_reader.map(|result| result.unwrap()).collect();

        assert_eq!(features.len(), 3);

        // First item should have no geometry but properties
        let first_feature = &features[0];
        assert!(first_feature.geom.is_none());

        // Check properties - should have name, description, and custom randomProperty
        let first_props = first_feature.props.as_ref().unwrap();
        assert_eq!(first_props.len(), 3);
        assert_eq!(
            first_props.get("name").unwrap(),
            &Value::String("Time Square".to_string())
        );
        assert!(matches!(
            first_props.get("description").unwrap(),
            &Value::String(_)
        ));
        assert_eq!(
            first_props.get("randomProperty").unwrap(),
            &Value::String("42".to_string())
        );

        // Confirm that the second feature is correct too
        assert_eq!(features[1].props.as_ref().unwrap().len(), 2);
    }

    // Test the folder structure. Note that the negative test - that there will be no folder key when there are no
    // folders, is tested in the above tests as we ensure that the property length is correct.
    #[test]
    fn kml_reader_folder_hierarchy() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/sample_kml/folders.kml");
        let kml_reader = KmlReader::try_new(path, ContentMode::Full).unwrap();
        let features: Vec<_> = kml_reader.map(|result| result.unwrap()).collect();

        assert_eq!(features.len(), 3);

        // First feature: raw Point geometry in top folder
        // Should have folder hierarchy even though it's not a Placemark
        let first_feature = &features[0];
        assert!(matches!(first_feature.geom, Some(geo::Geometry::Point(_))));
        let first_props = first_feature.props.as_ref().unwrap();

        // Check folder hierarchy for standalone geometry (should have one level)
        let hierarchy = first_props.get("folders").unwrap();
        if let Value::Array(folders) = hierarchy {
            assert_eq!(folders.len(), 1);
            if let Value::Object(folder) = &folders[0] {
                assert_eq!(
                    folder.get("name").unwrap(),
                    &Value::String("Top Level Folder".to_string())
                );
            } else {
                panic!("Expected folder to be an object");
            }
        } else {
            panic!("Expected folder_hierarchy to be an array");
        }

        // Second feature: Placemark in top folder
        let second_feature = &features[1];
        assert!(matches!(second_feature.geom, Some(geo::Geometry::Point(_))));
        let second_props = second_feature.props.as_ref().unwrap();
        assert_eq!(
            second_props.get("name").unwrap(),
            &Value::String("Folder Placemark".to_string())
        );
        assert_eq!(
            second_props.get("customField").unwrap(),
            &Value::String("folder_value".to_string())
        );

        // Check folder hierarchy for second feature (should have one level)
        let hierarchy = second_props.get("folders").unwrap();
        if let Value::Array(folders) = hierarchy {
            assert_eq!(folders.len(), 1);
            if let Value::Object(folder) = &folders[0] {
                assert_eq!(
                    folder.get("name").unwrap(),
                    &Value::String("Top Level Folder".to_string())
                );
            } else {
                panic!("Expected folder to be an object");
            }
        } else {
            panic!("Expected folder_hierarchy to be an array");
        }

        // Third feature: Placemark in nested folder
        let third_feature = &features[2];
        let third_props = third_feature.props.as_ref().unwrap();
        assert_eq!(
            third_props.get("name").unwrap(),
            &Value::String("Nested Placemark".to_string())
        );
        assert_eq!(
            third_props.get("nestedProperty").unwrap(),
            &Value::String("deep_value".to_string())
        );

        // Check folder hierarchy for third feature (should have two levels)
        let nested_hierarchy = third_props.get("folders").unwrap();
        if let Value::Array(folders) = nested_hierarchy {
            assert_eq!(folders.len(), 2);
            // First level (outermost)
            if let Value::Object(folder) = &folders[0] {
                assert_eq!(
                    folder.get("name").unwrap(),
                    &Value::String("Top Level Folder".to_string())
                );
            } else {
                panic!("Expected first folder to be an object");
            }
            // Second level (nested)
            if let Value::Object(folder) = &folders[1] {
                assert_eq!(
                    folder.get("name").unwrap(),
                    &Value::String("Nested Folder".to_string())
                );
            } else {
                panic!("Expected second folder to be an object");
            }
        } else {
            panic!("Expected folder_hierarchy to be an array");
        }
    }
}
