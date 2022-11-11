use csv::{Reader, ReaderBuilder, StringRecord};
use geo::Point;

use crate::error::FiberError;

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
        .from_reader("lat,lng,c\n1,2,3".as_bytes());

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

// TODO: Change String return to &str?
pub fn parse_record(
    record: Result<StringRecord, csv::Error>,
    settings: &CsvSettings,
) -> Result<(Point, Option<String>), FiberError> {
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
    let id = settings
        .id_index
        .and_then(|i| Some(record.get(i).unwrap().to_owned()));

    let test = Point::new(lng, lat);
    test.to_radians();

    Ok((test, id))
}
