use std::io::Stdin;

use csv::{Reader, ReaderBuilder, StringRecord};
use geo::Point;
use quadtree::ToRadians;

use crate::error::FiberError;

/// Index and label settings for the stream of test points.
pub struct InputSettings {
    pub lat_index: usize,
    pub lng_index: usize,
    pub id_index: Option<usize>,
    pub id_label: &'static str,
}

/// Test point, id field, and metadata extracted from an input record.
pub struct ParsedRecord<'a> {
    pub record: &'a StringRecord,
    pub point: Point,
    pub id: Option<&'a str>,
}

pub fn build_input_settings(
    id_label: Option<&'static str>,
    delimiter: u8,
) -> Result<(Reader<Stdin>, InputSettings), FiberError> {
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

    // TODO: Make a reasonable Error type/message here
    for (i, label) in reader
        .headers()
        .map_err(|_| {
            FiberError::IO(
                "cannot read csv input headers, please check the stdin input and try again",
            )
        })?
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
        Err(FiberError::IO(
            "cannot find lng and lat fields in csv input headers",
        ))
    }
}

pub fn parse_record<'a>(
    i: usize,
    record: Result<&'a StringRecord, &csv::Error>,
    settings: &InputSettings,
) -> Result<ParsedRecord<'a>, FiberError<'a>> {
    let record = record.map_err(|_| FiberError::Parse(i, "cannot read input record"))?;
    let lng = record
        .get(settings.lng_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| FiberError::Parse(i, "cannot parse lng for input record"))?;
    let lat = record
        .get(settings.lat_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| FiberError::Parse(i, "cannot parse lat for input record"))?;
    let id = settings.id_index.and_then(|i| Some(record.get(i).unwrap()));

    let mut point = Point::new(lng, lat);
    point.to_radians_in_place();

    Ok(ParsedRecord { record, point, id })
}
