use std::io::Stdout;

use csv::{Reader, ReaderBuilder, StringRecord, Writer, WriterBuilder};
use geo::Point;
use quadtree::{Geometry, ToRadians, MEAN_EARTH_RADIUS};

use crate::{error::FiberError, make_qt::IndexedDatum};

pub struct CsvSettings {
    pub lat_index: usize,
    pub lng_index: usize,
    pub id_index: Option<usize>,
    pub id_label: &'static str,
}

// TODO: Argument to determine the reader, then fix generic
pub(crate) fn build_csv_settings(
    id_label: Option<&'static str>,
) -> Result<(Reader<&[u8]>, CsvSettings), FiberError> {
    // Set up the reader based on the passed input
    // Note that the reader must have headers that contain a lat and lng field,
    // and can contain an optional, configurable id field for subsequent matching
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .delimiter(b',')
        .from_reader("lat,lng,id\n39,-77,name".as_bytes());

    // TODO: Semantics when it can't find? Auto numerically index? Fail silently?
    // Get the label to look for the id
    let id_label = id_label.unwrap_or("id");

    let mut id_index = None;
    let mut lat_index = None;
    let mut lng_index = None;

    // TODO: Make a reasonable Error type/message here
    for (i, label) in reader.headers().map_err(|_| FiberError)?.iter().enumerate() {
        let label = label.to_lowercase();
        let label = label.as_str();

        if label == id_label {
            id_index = Some(i);
        } else if label == "lat" {
            lat_index = Some(i);
        } else if label == "lng" {
            lng_index = Some(i);
        }
    }

    // Settings are only valid if we have an index for both the lat and lng
    if let (Some(lat_index), Some(lng_index)) = (lat_index, lng_index) {
        Ok((
            reader,
            CsvSettings {
                lat_index,
                lng_index,
                id_index,
                id_label,
            },
        ))
    } else {
        Err(FiberError)
    }
}

pub struct ParsedRecord<'a> {
    pub record: &'a StringRecord,
    pub point: Point,
    pub id: Option<&'a str>,
}

pub fn parse_record<'a>(
    record: Result<&'a StringRecord, &csv::Error>,
    settings: &CsvSettings,
) -> Result<ParsedRecord<'a>, FiberError> {
    let record = record.map_err(|_| FiberError)?;
    let lng = record
        .get(settings.lng_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| FiberError)?;
    let lat = record
        .get(settings.lat_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| FiberError)?;
    let id = settings.id_index.and_then(|i| Some(record.get(i).unwrap()));

    let mut point = Point::new(lng, lat);
    point.to_radians_in_place();

    Ok(ParsedRecord { record, point, id })
}

// TODO: Have to output the header row with other fields if capturing them from the metadata
//       Should output the index from the indexed datum if nothing else
pub fn make_csv_writer(id_label: &str) -> Result<Writer<Stdout>, FiberError> {
    let mut writer = WriterBuilder::new()
        .delimiter(b',')
        .from_writer(std::io::stdout());

    writer
        .write_record(&[id_label, "lng", "lat", "distance"])
        .map_err(|_| FiberError)?;

    Ok(writer)
}

pub struct WriteData<'a> {
    pub record: &'a StringRecord,
    pub datum: &'a IndexedDatum<Geometry<f64>>,
    pub dist: f64,
    pub id: Option<&'a str>,
    pub index: usize,
}

// TODO: This works, but can we avoid allocating strings?
pub fn write_line(w: &mut Writer<Stdout>, settings: &CsvSettings, data: WriteData) {
    // If we parsed an id from the input data, then use it here
    // otherwise use the record's index as a unique id.
    // This is useful as errors mean you may not be able to just line up the
    // output with the input
    let index_string = data.index.to_string();
    let id = if let Some(id) = data.id {
        id
    } else {
        &index_string[..]
    };

    // Convert the distance to meters and trucate at mm
    let dist = format!("{:.3}", data.dist * MEAN_EARTH_RADIUS);
    if w.write_record(&[
        id,
        data.record.get(settings.lng_index).unwrap(),
        data.record.get(settings.lat_index).unwrap(),
        dist.as_ref(),
    ])
    .is_err()
    {
        eprintln!(
            "Failed to write output line for record at index {}.",
            data.index
        );
    }
}
