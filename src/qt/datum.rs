use std::{collections::HashMap, iter::empty, rc::Rc};

use geo::Point;
use geojson::Feature;
use quadtree::{AsGeom, AsPoint, Geometry, GeometryRef};
use shapefile::dbase::Record;

use crate::kml::KmlItem;

use super::{
    csv::csv_field_val, geojson::json_field_val, kml::kml_field_val, shapefile::shp_field_val,
};

/// Datum to store in the quadtree, includes the index from the input file and the underlying data
/// to build output metadata fields as an enum by input file type.
pub struct Datum {
    geom: Geometry<f64>,
    base: BaseData,
    index: usize,
}

impl Datum {
    pub fn new(geom: Geometry<f64>, base: BaseData, index: usize) -> Self {
        Self { geom, base, index }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Pass through to the underlying meta implementation for building the field string.
    pub fn meta_iter<'a>(
        &'a self,
        fields: &'a Option<Vec<String>>,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        self.base.iter_str(fields)
    }
}

impl AsGeom<f64> for Datum {
    fn as_geom(&self) -> GeometryRef<f64> {
        self.geom.as_geom()
    }
}

impl AsPoint for Datum {
    fn as_point(&self) -> Point {
        // This is a somewhat hacky solution to get both points and bounds variants working for the
        // same datum type. Requires that insertion of non-point geometries into Point Quadtrees
        // fails so that while a Datum can be constructed, the quadtree itself won't be polluted by
        // invalid data
        match self.geom {
            Geometry::Point(p) => p,
            _ => unreachable!(),
        }
    }
}

/// Enum to capture underlying data to build metadata fields. Separate variant provided for each
/// input file type.
pub enum BaseData {
    Shp(Rc<Record>),
    Json(Rc<Feature>),
    // Kml doesn't need an RC because in this setup there are no mutligeometries with metadata that
    // get broken up with each requiring a reference to the KmlItem
    Kml(KmlItem),
    // Csvs only support points and therefore can never be spilt apart
    // But we have to store the records as a hasmap for efficient lookup later
    Csv(HashMap<String, String>),
    None,
}

impl BaseData {
    /// Iterate through the stored underlying data only retrieving a `String` version of metadata
    /// fields matching the keys provided in the `fields` vector.
    pub fn iter_str<'a>(
        &'a self,
        fields: &'a Option<Vec<String>>,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        // Deal with the case that there are no fields first
        if let Some(fields) = fields {
            Box::new(fields.iter().map(move |f| match self {
                Self::Shp(record) => shp_field_val(record, f),
                Self::Json(feature) => json_field_val(feature, f),
                Self::Kml(kml) => kml_field_val(kml, f),
                Self::Csv(record) => csv_field_val(record, f),
                Self::None => String::default(),
            }))
        } else {
            Box::new(empty())
        }
    }
}
