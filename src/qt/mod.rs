pub mod datum;

mod csv;
mod geojson;
mod kml;
mod shapefile;

use std::path::PathBuf;

use geo::{Point, Rect};
use quadtree::{
    AsGeom, BoundsQuadTree, CalcMethod, GeometryRef, PointQuadTree, QuadTree as QT, QuadTreeSearch,
    ToRadians,
};

use crate::csv::reader::ParsedRecord;
use crate::error::Error;
use datum::*;

use self::csv::build_csv;
use self::geojson::{build_geojson, geojson_bbox};
use self::kml::build_kml;
use self::shapefile::{build_shp, shp_bbox};

pub struct QtData {
    pub is_point_qt: bool,
    pub bounds: Rect<f64>,
    pub depth: u8,
    pub max_children: usize,
}

impl QtData {
    pub fn new(
        is_bounds: bool,
        bounds: Rect,
        depth: Option<u8>,
        max_children: Option<usize>,
    ) -> Self {
        Self {
            is_point_qt: is_bounds,
            bounds,
            depth: depth.unwrap_or(10),
            max_children: max_children.unwrap_or(10),
        }
    }
}

/// Result type including the stored geometry, the matched index from the datum, the distance for
/// the match, and the extracted + harmonized metadata that abstracts from any sort of metadata
/// generic, original datum, extracted metdata, and the distance.
pub struct SearchResult<'a> {
    pub geom: GeometryRef<'a, f64>,
    pub index: usize,
    pub distance: f64,
    pub meta: Box<dyn Iterator<Item = String> + 'a>,
}

/// QuadTree implementation. This is a light wrapper around both the Point and Bounds versions that
/// implements runtime blocks to not insert invalid data into the Point version. We do not
/// implement the QuadTree traits as they require a Node type parameter.
pub enum Quadtree {
    Point(PointQuadTree<Datum, f64>),
    Bounds(BoundsQuadTree<Datum, f64>),
}

impl Quadtree {
    pub fn new(opts: QtData) -> Self {
        let QtData {
            is_point_qt,
            bounds,
            depth,
            max_children,
        } = opts;

        if is_point_qt {
            Self::Point(PointQuadTree::new(
                bounds,
                CalcMethod::Spherical,
                depth,
                max_children,
            ))
        } else {
            Self::Bounds(BoundsQuadTree::new(
                bounds,
                CalcMethod::Spherical,
                depth,
                max_children,
            ))
        }
    }

    pub fn from_path(path: PathBuf, opts: QtData) -> Result<Self, Error> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or(Error::CannotParseFileExtension(path.clone()))?
        {
            "shp" => build_shp(path, opts),
            "json" => build_geojson(path, opts),
            "kml" | "kmz" => build_kml(path, opts),
            "csv" => build_csv(path, opts),
            _ => Err(Error::UnsupportedFileType),
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Self::Bounds(b) => b.size(),
            Self::Point(p) => p.size(),
        }
    }

    pub fn insert(&mut self, datum: Datum) -> Result<(), Error> {
        let i = datum.index();
        match self {
            Self::Bounds(b) => b.insert(datum).map_err(|err| Error::InsertFailed(i, err)),
            Self::Point(p) => {
                if matches!(datum.as_geom(), GeometryRef::Point::<f64>(_)) {
                    p.insert(datum).map_err(|err| Error::InsertFailed(i, err))
                } else {
                    Err(Error::InsertFailedRequiresPoint(i))
                }
            }
        }
    }

    pub fn retrieve<'a>(&'a self, datum: &Datum) -> Box<dyn Iterator<Item = &Datum> + 'a> {
        match self {
            Self::Bounds(b) => Box::new(b.retrieve(datum)),
            Self::Point(p) => Box::new(p.retrieve(datum)),
        }
    }

    pub fn find<'a>(
        &'a self,
        record: &ParsedRecord,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<SearchResult<'a>, Error> {
        let (datum, distance) = match self {
            Self::Bounds(b) => b.find_r(&record.point, r.unwrap_or(f64::INFINITY)),
            Self::Point(p) => p.find_r(&record.point, r.unwrap_or(f64::INFINITY)),
        }
        .map_err(|err| Error::FindError(record.index, err))?;

        Ok(SearchResult {
            geom: datum.as_geom(),
            index: datum.index(),
            distance,
            meta: datum.meta_iter(fields),
        })
    }

    pub fn knn<'a>(
        &'a self,
        record: &ParsedRecord,
        k: usize,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<Vec<SearchResult<'a>>, Error> {
        let found = match self {
            Self::Bounds(b) => b.knn_r(&record.point, k, r.unwrap_or(f64::INFINITY)),
            Self::Point(p) => p.knn_r(&record.point, k, r.unwrap_or(f64::INFINITY)),
        }
        .map_err(|err| Error::FindError(record.index, err))?;

        Ok(found
            .into_iter()
            .map(|(datum, distance)| SearchResult {
                geom: datum.as_geom(),
                index: datum.index(),
                distance,
                meta: datum.meta_iter(fields),
            })
            .collect())
    }
}

impl std::fmt::Display for Quadtree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Quadtree::Bounds(b) => b.fmt(f),
            Quadtree::Point(p) => p.fmt(f),
        }
    }
}

/// Build the Bounding Box from provided arguments.
pub fn make_bbox(path: &PathBuf, sphere: bool, bbox: &Option<String>) -> Result<Rect, Error> {
    // Get the right bbox points given the argument values
    let (a, b) = if sphere {
        // Sphere option builds sphere bounds broken at the antimeridian
        (Point::new(-180.0, -90.0), Point::new(180.0, 90.0))
    } else if let Some(bbox_str) = &bbox {
        // Parse from the bbox_str
        let mut pts = bbox_str
            .split(',')
            .map(|s| s.parse::<f64>().map_err(|_| Error::InvalidBoundingBox));
        (
            Point::new(bbox_next(&mut pts)?, bbox_next(&mut pts)?),
            Point::new(bbox_next(&mut pts)?, bbox_next(&mut pts)?),
        )
    } else {
        // Default to the bbox available on the input file, matching on the file type
        match path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or(Error::CannotParseFileExtension(path.clone()))?
        {
            "shp" => shp_bbox(path)?,
            "json" => geojson_bbox(path)?,
            "kml" | "kmz" | "csv" => {
                // Appears to be no overall bbox embedded in kml files and csv files, so default to sphere
                (Point::new(-180.0, -90.0), Point::new(180.0, 90.0))
            }
            _ => Err(Error::UnsupportedFileType)?,
        }
    };

    let mut rect = Rect::new(a, b);
    rect.to_radians_in_place();
    eprintln!("{:?}", rect);
    Ok(rect)
}

fn bbox_next<'a>(pts: &mut dyn Iterator<Item = Result<f64, Error>>) -> Result<f64, Error> {
    pts.next().ok_or(Error::InvalidBoundingBox).and_then(|x| x)
}
