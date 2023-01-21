use std::path::PathBuf;
use std::rc::Rc;

use geo::Point;
use shapefile::{dbase::Record, Reader};

use crate::error::{Error, ParseType};
use crate::shp::convert_dbase_field_opt;
use crate::shp::convert_shape;

use super::datum::{BaseData, Datum};
use super::QtData;
use super::Quadtree;

pub fn shp_field_val(record: &Record, field: &String) -> String {
    convert_dbase_field_opt(record.get(field))
}

pub fn build_shp(path: PathBuf, opts: QtData) -> Result<Quadtree, Error> {
    let mut shapefile = Reader::from_path(path.clone()).map_err(|_| Error::CannotReadFile(path))?;
    let shp_iter = shapefile
        .iter_shapes_and_records()
        .enumerate()
        .map(|(i, res)| {
            (
                i,
                res.map_err(|_| Error::CannotParseRecord(i, ParseType::Shapefile)),
            )
        });
    let mut qt = Quadtree::new(opts);

    for (index, shp) in shp_iter {
        match shp {
            Ok((shp, record)) => {
                // Use an RC here to simplify: we don't need to keep a master list around and manage
                // the references, but can still avoid duplicating the records
                let record = Rc::new(record);
                for geom in convert_shape(shp) {
                    if let Some(err) = geom
                        .map(|g| Datum::new(g, BaseData::Shp(Rc::clone(&record)), index))
                        .and_then(|datum| qt.insert(datum))
                        .err()
                    {
                        eprintln!("{err}");
                    }
                }
            }
            Err(err) => eprintln!("{err}"),
        }
    }

    Ok(qt)
}

pub fn shp_bbox(path: &PathBuf) -> Result<(Point, Point), Error> {
    let shp = Reader::from_path(&path).map_err(|_| Error::CannotReadFile(path.clone()))?;
    Ok((shp.header().bbox.min.into(), shp.header().bbox.max.into()))
}
