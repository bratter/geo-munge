use std::path::PathBuf;
use std::{fs::read_to_string, iter::once};

use geojson::GeoJson;
use quadtree::Geometry;

use crate::error::FiberError;

use super::{
    datum::IndexedDatum, make_dyn_qt, QtData, SearchResult, Searchable, SearchableWithMeta,
};

pub struct JsonWithMeta {
    qt: Box<dyn Searchable<IndexedDatum<Geometry<f64>>>>,
}

impl JsonWithMeta {
    fn make_search_result<'a>(
        &'a self,
        found: (&'a IndexedDatum<Geometry<f64>>, f64),
        _fields: &'a Option<Vec<String>>,
    ) -> SearchResult {
        let (datum, _distance) = found;
        let _meta = once(datum);

        // SearchResult {
        //     datum,
        //     distance,
        //     meta: todo!(),
        // }
        todo!()
    }
}

impl std::fmt::Display for JsonWithMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.qt.fmt(f)
    }
}

impl SearchableWithMeta for JsonWithMeta {
    fn size(&self) -> usize {
        self.qt.size()
    }

    fn find<'a>(
        &'a self,
        cmp: &geo::Point,
        r: Option<f64>,
        fields: &'a Option<Vec<String>>,
    ) -> Result<super::SearchResult<'a>, quadtree::Error> {
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

pub fn geojson_build(
    path: PathBuf,
    opts: QtData,
) -> Result<Box<dyn SearchableWithMeta>, FiberError> {
    let _geojson = read_to_string(&path)
        .map_err(|_| FiberError::IO("Cannot read GeoJson file"))?
        .parse::<GeoJson>()
        .map_err(|_| FiberError::IO("Cannot parse GeoJson file"))?;

    let qt = make_dyn_qt(&opts);
    // TODO: Need to match on the type of geojson here, different answer if it is a collection
    // TODO: How to manage the datum type? think the easiest way is to leave as is then walk the
    //       geojson that we'll just store in the JsonWithMeta, hopefully it is indexed
    // TODO: Will have to make a lib file for geojson where we pull common stuff from the meta work
    // TODO: Probably have to covert the geojson types into geotypes types for insertion
    // TODO: Is this the best way to manage the error?
    // match geojson {
    //     GeoJson::Geometry(ref g) => todo!(),
    //     GeoJson::Feature(ref f) => todo!(),
    //     GeoJson::FeatureCollection(ref fc) => todo!(),
    // }
    // .map_err(|_| FiberError::Arg("Couldn't insert into quadtree"))?;

    Ok(Box::new(JsonWithMeta { qt }))
}
