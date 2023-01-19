pub mod datum;
mod geojson;
mod kml;
mod shapefile;

use std::path::PathBuf;

use ::geojson::GeoJson;
use ::shapefile::Reader;
use geo::{Point, Rect};
use quadtree::*;

use crate::error::FiberError;
use datum::*;

use self::geojson::{build_geojson, read_geojson};
use self::kml::build_kml;
use self::shapefile::build_shp;

pub struct QtData {
    pub is_bounds: bool,
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
            is_bounds,
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
    pub geom: &'a Geometry<f64>,
    pub index: usize,
    pub distance: f64,
    pub meta: Box<dyn Iterator<Item = String> + 'a>,
}

/// Create the correct quadtree wrapper based on the input options and provided filetype.
pub fn make_qt(path: PathBuf, opts: QtData) -> Result<VarQt, FiberError> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or(FiberError::IO("Cannot parse file extension"))?
    {
        "shp" => build_shp(path, opts),
        "json" => build_geojson(path, opts),
        "kml" | "kmz" => build_kml(path, opts),
        _ => Err(FiberError::IO("Unsupported file type")),
    }
}

// TODO: This needs to be renamed
pub enum VarQt {
    Point(PointQuadTree<VarDatum, f64>),
    Bounds(BoundsQuadTree<VarDatum, f64>),
}

// TODO: Any chance of implementing QT and QTSearch for this, or is the type param insurmountable?
impl VarQt {
    pub fn new(opts: QtData) -> Self {
        let QtData {
            is_bounds,
            bounds,
            depth,
            max_children,
        } = opts;

        if is_bounds {
            Self::Bounds(BoundsQuadTree::new(bounds, depth, max_children))
        } else {
            Self::Point(PointQuadTree::new(bounds, depth, max_children))
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Self::Bounds(b) => b.size(),
            Self::Point(p) => p.size(),
        }
    }

    pub fn insert(&mut self, datum: VarDatum) -> Result<(), Error> {
        match self {
            Self::Bounds(b) => b.insert(datum),
            Self::Point(p) => {
                if matches!(datum.geometry(), Geometry::Point::<f64>(_)) {
                    p.insert(datum)
                } else {
                    // TODO: Fix the Error here - should say that only points can be added to a
                    // point quadtree
                    Err(Error::OutOfBounds)
                }
            }
        }
    }

    // TODO: Consider not collecting here and having a wrapper enum or a Box dyn
    pub fn retrieve(&self, datum: &VarDatum) -> Vec<&VarDatum> {
        match self {
            Self::Bounds(b) => b.retrieve(datum).collect(),
            Self::Point(p) => p.retrieve(datum).collect(),
        }
    }

    // TODO: Look at the sphere wrapper in QuadTree - it needs to be better
    pub fn find<'a>(
        &'a self,
        cmp: &Point,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<SearchResult<'a>, Error> {
        let (datum, distance) = match self {
            Self::Bounds(b) => b.find_r(&sphere(*cmp), r.unwrap_or(f64::INFINITY)),
            Self::Point(p) => p.find_r(&sphere(*cmp), r.unwrap_or(f64::INFINITY)),
        }?;

        Ok(SearchResult {
            geom: &datum.geom(),
            index: datum.index(),
            distance,
            meta: datum.meta_iter(fields),
        })
    }

    pub fn knn<'a>(
        &'a self,
        cmp: &Point,
        k: usize,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<Vec<SearchResult<'a>>, Error> {
        let found = match self {
            Self::Bounds(b) => b.knn_r(&sphere(*cmp), k, r.unwrap_or(f64::INFINITY)),
            Self::Point(p) => p.knn_r(&sphere(*cmp), k, r.unwrap_or(f64::INFINITY)),
        }?;

        Ok(found
            .into_iter()
            .map(|(datum, distance)| SearchResult {
                geom: &datum.geom(),
                index: datum.index(),
                distance,
                meta: datum.meta_iter(fields),
            })
            .collect())
    }
}

impl std::fmt::Display for VarQt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VarQt::Bounds(b) => b.fmt(f),
            VarQt::Point(p) => p.fmt(f),
        }
    }
}

/// Build the Bounding Box from provided arguments.
// TODO: Consider moving this into the make_qt_from_path function, and then avoiding the extra file
//       handle.
// TODO: Also move the file specific methods to their own lib files
//       The basic struct for each type of file can have a read and a bbox method
pub fn make_bbox(path: &PathBuf, sphere: bool, bbox: &Option<String>) -> Result<Rect, FiberError> {
    // Get the right bbox points given the argument values
    let (a, b) = if sphere {
        // Sphere option builds sphere bounds broken at the antimeridian
        (Point::new(-180.0, -90.0), Point::new(180.0, 90.0))
    } else if let Some(bbox_str) = &bbox {
        // Parse from the bbox_str
        let mut pts = bbox_str.split(',').map(|s| {
            s.parse::<f64>()
                .map_err(|_| FiberError::Arg("Cannot parse bbox input"))
        });
        (
            Point::new(bbox_next(&mut pts)?, bbox_next(&mut pts)?),
            Point::new(bbox_next(&mut pts)?, bbox_next(&mut pts)?),
        )
    } else {
        // Default to the bbox available on the input file, matching on the file type
        match path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or(FiberError::IO("Cannot parse file extension"))?
        {
            "shp" => {
                let shp = Reader::from_path(&path).map_err(|_| {
                    FiberError::IO(
                        "cannot read shapefile, check path and permissions and try again",
                    )
                })?;
                (shp.header().bbox.min.into(), shp.header().bbox.max.into())
            }
            "json" => {
                let json = read_geojson(path)?;
                let bbox = match json {
                    GeoJson::Feature(f) => f.bbox,
                    GeoJson::Geometry(g) => g.bbox,
                    GeoJson::FeatureCollection(fc) => fc.bbox,
                };
                let bbox = bbox.ok_or(FiberError::Arg("No bbox present on GeoJson"))?;

                // Ensure that the bounding box has length 4 to guarantee we can build a proper
                // bouding box
                if bbox.len() != 2 {
                    return Err(FiberError::Arg("Invalid bounding box present in GeoJson"));
                }

                (Point::new(bbox[0], bbox[1]), Point::new(bbox[2], bbox[3]))
            }
            "kml" | "kmz" => {
                // Appears to be no overall bbox embedded in kml files, so default to sphere
                (Point::new(-180.0, -90.0), Point::new(180.0, 90.0))
            }
            _ => Err(FiberError::IO("Unsupported file type"))?,
        }
    };

    let mut rect = Rect::new(a.0, b.0);
    rect.to_radians_in_place();
    Ok(rect)
}

fn bbox_next<'a>(
    pts: &mut dyn Iterator<Item = Result<f64, FiberError>>,
) -> Result<f64, FiberError> {
    pts.next()
        .ok_or(FiberError::Arg("Unexpected end of bbox input"))
        .and_then(|x| x)
}
