use std::iter::empty;
use std::path::PathBuf;

use shapefile::{dbase::Record, Reader, Shape};

use crate::shp::convert_shapes;
use crate::{error::FiberError, shp::convert_dbase_field_opt};

use super::{
    datum::IndexedDatum, make_dyn_qt, QtData, SearchResult, Searchable, SearchableWithMeta,
};

pub struct ShpWithMeta {
    qt: Box<dyn Searchable<IndexedDatum>>,

    records: Vec<Record>,
}

impl ShpWithMeta {
    /// Private function to make a search results struct from a single found item and the extra
    /// fields list. Used in both single result and knn form.
    fn make_search_result<'a>(
        &'a self,
        found: (&'a IndexedDatum, f64),
        fields: &'a Option<Vec<String>>,
    ) -> SearchResult {
        let (datum, distance) = found;
        let record = self.records.get(datum.index).unwrap();
        let meta: Box<dyn Iterator<Item = String>> = match fields.as_ref() {
            Some(fields) => Box::new(
                fields
                    .iter()
                    .map(|f| f.as_str())
                    .map(|f| convert_dbase_field_opt(record.get(f))),
            ),
            None => Box::new(empty()),
        };

        SearchResult {
            geom: &datum.geom,
            index: datum.index,
            distance,
            meta,
        }
    }
}

impl std::fmt::Display for ShpWithMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.qt.fmt(f)
    }
}

impl SearchableWithMeta for ShpWithMeta {
    fn size(&self) -> usize {
        self.qt.size()
    }

    fn find<'a>(
        &'a self,
        cmp: &geo::Point,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<SearchResult<'a>, quadtree::Error> {
        let item = self.qt.find(cmp, r)?;
        Ok(self.make_search_result(item, fields))
    }

    fn knn<'a>(
        &'a self,
        cmp: &geo::Point,
        k: usize,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<Vec<SearchResult<'a>>, quadtree::Error> {
        Ok(self
            .qt
            .knn(cmp, k, r)?
            .into_iter()
            .map(|item| self.make_search_result(item, fields))
            .collect())
    }
}

pub fn shp_build<'a>(
    path: PathBuf,
    opts: QtData,
) -> Result<Box<dyn SearchableWithMeta>, FiberError> {
    let mut shapefile = Reader::from_path(path).map_err(|_| {
        FiberError::IO("cannot read shapefile, check path and permissions and try again")
    })?;

    let mut records = Vec::new();
    let add_record = |res| -> Result<(Shape, usize), ()> {
        match res {
            Ok((s, r)) => {
                let i = records.len();
                records.push(r);

                Ok((s, i))
            }
            Err(_) => Err(()),
        }
    };

    // TODO: Convert this to inserting the meta record with the datum and generalize from the qt
    // type
    let mut qt = make_dyn_qt(&opts);
    for shp in shapefile
        .iter_shapes_and_records()
        .map(add_record)
        .flat_map(convert_shapes)
    {
        if let Ok((shape, i)) = shp {
            if qt.insert(IndexedDatum::without_meta(shape, i)).is_err() {
                eprintln!("Cannot insert datum at index {i} into qt")
            }
        } else {
            eprintln!("Could not read shape")
        }
    }

    Ok(Box::new(ShpWithMeta { qt, records }))
}
