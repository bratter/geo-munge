use std::path::PathBuf;
use std::rc::Rc;

use geo::Point;
use shapefile::{dbase::Record, Reader};

use crate::shp::convert_shape;
use crate::{error::FiberError, shp::convert_dbase_field_opt};

use super::datum::{BaseData, Datum};
use super::QtData;
use super::Quadtree;

pub fn shp_field_val(record: &Record, field: &String) -> String {
    convert_dbase_field_opt(record.get(field))
}

pub fn build_shp(path: PathBuf, opts: QtData) -> Result<Quadtree, FiberError> {
    let mut shapefile = Reader::from_path(path).map_err(|_| {
        FiberError::IO("cannot read shapefile, check path and permissions and try again")
    })?;
    let mut qt = Quadtree::new(opts);

    for (index, shp) in shapefile.iter_shapes_and_records().enumerate() {
        if let Ok((shp, record)) = shp {
            // Use an RC here to simplify: we don't need to keep a master list around and manage
            // the references, but can still avoid duplicating the records
            let record = Rc::new(record);
            for geom in convert_shape(shp) {
                if geom
                    .map(|g| Datum::new(g, BaseData::Shp(Rc::clone(&record)), index))
                    // TODO: This should output a real error when fixing errors
                    //       and are the semantics correct for read vs insert errors?
                    .and_then(|datum| qt.insert(datum).map_err(|_| ()))
                    .is_err()
                {
                    eprintln!("Cannot insert datum at index {index} into qt");
                }
            }
        } else {
            eprintln!("Could not read shape at index {index}");
        }
    }

    Ok(qt)
}

pub fn shp_bbox(path: &PathBuf) -> Result<(Point, Point), FiberError> {
    let shp = Reader::from_path(&path).map_err(|_| {
        FiberError::IO("cannot read shapefile, check path and permissions and try again")
    })?;
    Ok((shp.header().bbox.min.into(), shp.header().bbox.max.into()))
}
