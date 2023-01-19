use std::{iter::empty, rc::Rc};

use geo::Point;

use geojson::Feature;
use quadtree::{Geometry, PointDatum};
use shapefile::dbase::Record;

use crate::kml::KmlItem;

use super::{geojson::json_field_val, kml::kml_field_val, shapefile::shp_field_val};

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

    // TODO: See note on geometry in datum below, but use this when we need references for now
    pub fn geom(&self) -> &Geometry<f64> {
        &self.geom
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

impl quadtree::Datum<f64> for Datum {
    // TODO: Can we change this in quadtree so that getting the geometry only returns a ref
    //       It is not that easy if we want to use the individual geo-types as datums as we won't
    //       be able to return references. Either have to change Geometry to take a ref or an rc
    fn geometry(&self) -> Geometry<f64> {
        self.geom.clone()
    }
}

impl PointDatum<f64> for Datum {
    fn point(&self) -> Point<f64> {
        // This is a somewhat hacky solution to get both points and bounds variants working for the
        // same datum type. Requires that insertion of non-point geometries into Point Quadtrees
        // fails so that while a Datum can be constructed, the quadtree itself won't be polluted by
        // invalid data
        match self.geom {
            Geometry::Point(p) => p.to_owned(),
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
    None,
}

impl BaseData {
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
                Self::None => String::default(),
            }))
        } else {
            Box::new(empty())
        }
    }
}
