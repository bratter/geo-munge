use std::io::Stdin;

use csv::{Reader, ReaderBuilder, StringRecord};
use geo::Point;
use quadtree::ToRadians;

use crate::error::{Error, ParseType};

/// Index and label settings for the stream of test points.
pub struct InputSettings {
    pub lat_index: usize,
    pub lng_index: usize,
    pub id_index: Option<usize>,
    pub id_label: &'static str,
}

/// Test point, id field, and metadata extracted from an input record.
pub struct ParsedRecord {
    pub index: usize,
    pub record: StringRecord,
    pub point: Point,
    pub id: Option<String>,
}

pub fn build_input_settings(
    id_label: Option<&'static str>,
    delimiter: u8,
) -> Result<(Reader<Stdin>, InputSettings), Error> {
    // Set up the reader based on the passed input
    // Note that the reader must have headers that contain a lat and lng field,
    // and can contain an optional, configurable id field for subsequent matching
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .delimiter(delimiter)
        .from_reader(std::io::stdin());

    // Get the label to look for the id
    let id_label = id_label.unwrap_or("id");

    let mut id_index = None;
    let mut lat_index = None;
    let mut lng_index = None;

    for (i, label) in reader
        .headers()
        .map_err(|err| Error::CsvParseError(err))?
        .iter()
        .enumerate()
    {
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
            InputSettings {
                lat_index,
                lng_index,
                id_index,
                id_label,
            },
        ))
    } else {
        Err(Error::MissingLatLngField)
    }
}

pub fn parse_record<'a>(
    index: usize,
    record: Result<StringRecord, csv::Error>,
    settings: &InputSettings,
) -> Result<ParsedRecord, Error> {
    let record = record.map_err(|err| Error::CsvParseError(err))?;
    let lng = record
        .get(settings.lng_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| Error::CannotParseRecord(index, ParseType::Lng))?;
    let lat = record
        .get(settings.lat_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| Error::CannotParseRecord(index, ParseType::Lat))?;
    let id = settings
        .id_index
        .and_then(|i| Some(record.get(i).unwrap().to_owned()));

    let mut point = Point::new(lng, lat);
    point.to_radians_in_place();

    Ok(ParsedRecord {
        index,
        record,
        point,
        id,
    })
}
