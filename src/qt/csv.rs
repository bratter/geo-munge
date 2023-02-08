use std::{collections::HashMap, fs::File, io::BufReader, path::PathBuf};

use csv::{ReaderBuilder, StringRecord};
use geo::Point;
use quadtree::Geometry;

use crate::error::{Error, ParseType};

use super::{
    datum::{BaseData, Datum},
    QtData, Quadtree,
};

/// Test point, id field, and metadata extracted from an input comparison point.
pub struct ParsedRecord {
    /// The index of the input comparison point from the incoming csv.
    pub index: usize,

    /// The unparsed csv record.
    pub record: StringRecord,

    /// Comparison point pulled from the csv record.
    pub point: Point,

    /// If available, the value if the id field from the incoming csv, separated for easy tracking
    /// of promary keys through the putput data.
    pub id: Option<String>,
}

pub fn csv_field_val(record: &HashMap<String, String>, field: &String) -> String {
    record.get(field).map(|s| s.to_string()).unwrap_or_default()
}

// CSVs as input data only support points based on a case insensitive lat and lng field as column
// headers in the input file.
pub fn build_csv(path: PathBuf, opts: QtData) -> Result<Quadtree, Error> {
    let file = BufReader::new(File::open(path.clone()).map_err(|_| Error::CannotReadFile(path))?);
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        // TODO: Can we pass the delimiter from the args to this function?
        .delimiter(b',')
        .from_reader(file);

    // We need to store the headers with each record to ensure that we can extract any metadata on
    // retrieval, then get the indicies of the lng and the lat from these headers
    let headers = reader
        .headers()
        .map_err(|err| Error::CsvParseError(err))?
        .to_owned();
    let lng_lat_i = get_lng_lat_index(&headers)?;

    // Run through all the records producing datums for all valid data
    let mut qt = Quadtree::new(opts);
    let results = reader.into_records().enumerate().map(|(i, res)| {
        res.map_err(|_| Error::CannotParseRecord(i, ParseType::Csv))
            .and_then(|record| {
                Ok(Datum::new(
                    point_from_record(&record, i, lng_lat_i)?,
                    BaseData::Csv(make_record_map(&record, &headers)),
                    i,
                ))
            })
            .and_then(|datum| qt.insert(datum))
    });

    for res in results {
        match res {
            Err(err) => eprintln!("{err}"),
            Ok(_) => (),
        }
    }

    Ok(qt)
}

// Make sure we caputure the index of the lat and the lng fields, terminating if they are not
// present
fn get_lng_lat_index(headers: &StringRecord) -> Result<(usize, usize), Error> {
    let mut lng_index = None;
    let mut lat_index = None;

    for (index, header) in headers.iter().enumerate() {
        let header = header.to_lowercase();
        let header = header.as_str();

        if header == "lng" {
            lng_index = Some(index);
        } else if header == "lat" {
            lat_index = Some(index);
        }
    }

    if let (Some(lng_index), Some(lat_index)) = (lng_index, lat_index) {
        Ok((lng_index, lat_index))
    } else {
        Err(Error::MissingLatLngField)
    }
}

fn point_from_record(
    record: &StringRecord,
    index: usize,
    (lng_index, lat_index): (usize, usize),
) -> Result<Geometry<f64>, Error> {
    let lng = record
        .get(lng_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| Error::CannotParseRecord(index, ParseType::Lng))?;
    let lat = record
        .get(lat_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| Error::CannotParseRecord(index, ParseType::Lat))?;

    // Must convert to radians
    Ok(Geometry::Point(Point::new(lng, lat).to_radians()))
}

fn make_record_map(record: &StringRecord, headers: &StringRecord) -> HashMap<String, String> {
    HashMap::from_iter(
        headers
            .into_iter()
            .zip(record)
            .map(|(k, v)| (k.to_lowercase(), v.to_string())),
    )
}
